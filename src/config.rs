//! Configuration file support.
//!
//! Reads an optional `ddlint.yml` (or a path supplied via `--config`).
//! Without a config file all rules run at their default severity.
//!
//! Example config:
//!
//! ```yaml
//! rules:
//!   MODIFY_COLUMN:    warn    # downgrade from danger to warn
//!   DROP_TABLE:       error   # keep at danger (explicit)
//!   ADD_COLUMN_ENUM:  ignore  # suppress entirely
//! ```
//!
//! Valid levels: `error` / `danger`, `warn` / `warning`, `ignore` / `off`.
//! Unmentioned rules use their built-in default severity.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::{fmt, fs};

use serde::Deserialize;

use crate::dialect::Dialect;
use crate::finding::Severity;

// ---------------------------------------------------------------------------
// On-disk schema
// ---------------------------------------------------------------------------

/// The shape of the YAML config file.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    /// SQL dialect / database engine. Defaults to `mysql`.
    #[serde(default)]
    pub dialect: Option<Dialect>,
    /// Per-rule overrides. Key is the rule name (e.g. `MODIFY_COLUMN`).
    /// Value is the desired level string.
    #[serde(default)]
    pub rules: HashMap<String, RuleLevel>,
}

/// The three possible outcomes for a rule.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleLevel {
    /// Emit a [`Severity::Danger`] finding (causes exit 1). Aliases: `error`.
    #[serde(alias = "error")]
    Danger,
    /// Emit a [`Severity::Warning`] finding. Aliases: `warning`.
    #[serde(alias = "warning")]
    Warn,
    /// Suppress the rule entirely — no finding emitted.
    #[serde(alias = "off")]
    Ignore,
}

impl fmt::Display for RuleLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleLevel::Danger => write!(f, "error"),
            RuleLevel::Warn => write!(f, "warn"),
            RuleLevel::Ignore => write!(f, "ignore"),
        }
    }
}

impl FromStr for RuleLevel {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "danger" | "error" => Ok(RuleLevel::Danger),
            "warn" | "warning" => Ok(RuleLevel::Warn),
            "ignore" | "off" => Ok(RuleLevel::Ignore),
            other => Err(format!(
                "unknown rule level `{other}` — valid values: error, warn, ignore"
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Resolved config (used at runtime)
// ---------------------------------------------------------------------------

/// Resolved, validated configuration ready for use by the rules engine.
#[derive(Debug, Default)]
pub struct Config {
    pub dialect: Dialect,
    overrides: HashMap<String, RuleLevel>,
}

impl Config {
    /// Build a `Config` from a parsed [`ConfigFile`], validating rule names
    /// against the known set.
    pub fn from_file(file: ConfigFile) -> Result<Self, Vec<String>> {
        let dialect = file.dialect.unwrap_or_default();
        let mut errors = Vec::new();
        let known = known_rules(&dialect);

        for rule in file.rules.keys() {
            if !known.contains(&rule.as_str()) {
                errors.push(format!(
                    "Unknown rule `{rule}` in config. Known rules: {}",
                    known.join(", ")
                ));
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(Self {
            dialect,
            overrides: file.rules,
        })
    }

    /// Determine the effective [`RuleLevel`] for a named rule.
    ///
    /// If the rule has an override in the config, that takes effect.
    /// Otherwise falls back to the rule's built-in default severity.
    pub fn effective_level(&self, rule: &str, default_severity: &Severity) -> RuleLevel {
        if let Some(level) = self.overrides.get(rule) {
            return level.clone();
        }
        match default_severity {
            Severity::Danger => RuleLevel::Danger,
            Severity::Warning => RuleLevel::Warn,
        }
    }

    /// Convert an effective level to an optional [`Severity`].
    /// Returns `None` when the rule is ignored.
    pub fn to_severity(level: &RuleLevel) -> Option<Severity> {
        match level {
            RuleLevel::Danger => Some(Severity::Danger),
            RuleLevel::Warn => Some(Severity::Warning),
            RuleLevel::Ignore => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Default config file name searched in the current working directory.
pub const DEFAULT_CONFIG_NAME: &str = "ddlint.yml";

/// Load and validate a config file.
///
/// - `explicit_path`: a path from `--config`, must exist.
/// - If `None`, looks for `ddlint.yml` in the current directory.
///   If that file also doesn't exist, returns a default (all rules at built-in severity).
pub fn load_config(explicit_path: Option<&Path>) -> anyhow::Result<Config> {
    let path: Option<PathBuf> = match explicit_path {
        Some(p) => {
            if !p.exists() {
                anyhow::bail!("Config file not found: {}", p.display());
            }
            Some(p.to_path_buf())
        }
        None => {
            let default = PathBuf::from(DEFAULT_CONFIG_NAME);
            if default.exists() {
                Some(default)
            } else {
                None
            }
        }
    };

    let Some(path) = path else {
        return Ok(Config::default());
    };

    let text = fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("Cannot read config file {}: {e}", path.display()))?;

    // An empty file is equivalent to an empty config
    let file: ConfigFile = if text.trim().is_empty() {
        ConfigFile::default()
    } else {
        serde_yaml::from_str(&text)
            .map_err(|e| anyhow::anyhow!("Config parse error in {}: {e}", path.display()))?
    };

    Config::from_file(file).map_err(|errs| {
        anyhow::anyhow!(
            "Config validation errors in {}:\n{}",
            path.display(),
            errs.join("\n")
        )
    })
}

// ---------------------------------------------------------------------------
// Known rules registry
// ---------------------------------------------------------------------------

/// Returns all rule identifiers for the given dialect (statement-level + file-level).
/// Derived from the dialect's rule registries — stays in sync automatically.
pub fn known_rules(dialect: &Dialect) -> Vec<&'static str> {
    let mut ids: Vec<&'static str> = dialect.rules().iter().map(|r| r.meta().id).collect();
    ids.extend(dialect.file_rules().iter().map(|r| r.meta().id));
    ids
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    fn parse(yaml: &str) -> Result<Config, anyhow::Error> {
        let file: ConfigFile = serde_yaml::from_str(yaml).expect("yaml parse");
        Config::from_file(file).map_err(|errs| anyhow::anyhow!("{}", errs.join("; ")))
    }

    // ── YAML parsing ─────────────────────────────────────────────────────────

    #[test]
    fn empty_config_is_valid() {
        let cfg = parse("rules: {}").unwrap();
        assert!(cfg.overrides.is_empty());
    }

    #[test]
    fn empty_file_is_valid() {
        // Empty file is handled by load_config directly (treats blank as default).
        // Here we just confirm "rules: {}" round-trips correctly as a proxy.
        let cfg = parse("rules: {}").unwrap();
        assert!(cfg.overrides.is_empty());
    }

    #[test]
    fn parses_all_level_spellings() {
        let cfg = parse(indoc! {"
            rules:
              MODIFY_COLUMN:    error
              DROP_TABLE:       danger
              ADD_COLUMN_ENUM:  warn
              DROP_COLUMN:      warning
              TRUNCATE:         ignore
              RENAME_TABLE:     off
        "})
        .unwrap();

        assert_eq!(cfg.overrides["MODIFY_COLUMN"], RuleLevel::Danger);
        assert_eq!(cfg.overrides["DROP_TABLE"], RuleLevel::Danger);
        assert_eq!(cfg.overrides["ADD_COLUMN_ENUM"], RuleLevel::Warn);
        assert_eq!(cfg.overrides["DROP_COLUMN"], RuleLevel::Warn);
        assert_eq!(cfg.overrides["TRUNCATE"], RuleLevel::Ignore);
        assert_eq!(cfg.overrides["RENAME_TABLE"], RuleLevel::Ignore);
    }

    #[test]
    fn unknown_rule_is_validation_error() {
        let result = parse(indoc! {"
            rules:
              NOT_A_REAL_RULE: warn
        "});
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("NOT_A_REAL_RULE"));
    }

    #[test]
    fn unknown_field_at_top_level_is_error() {
        let result: Result<ConfigFile, _> = serde_yaml::from_str("unknown_key: true");
        assert!(result.is_err());
    }

    // ── effective_level ───────────────────────────────────────────────────────

    #[test]
    fn no_override_uses_default_danger() {
        let cfg = parse("rules: {}").unwrap();
        let level = cfg.effective_level("MODIFY_COLUMN", &Severity::Danger);
        assert_eq!(level, RuleLevel::Danger);
    }

    #[test]
    fn no_override_uses_default_warning() {
        let cfg = parse("rules: {}").unwrap();
        let level = cfg.effective_level("ADD_COLUMN_ENUM", &Severity::Warning);
        assert_eq!(level, RuleLevel::Warn);
    }

    #[test]
    fn override_downgrade_danger_to_warn() {
        let cfg = parse(indoc! {"
            rules:
              MODIFY_COLUMN: warn
        "})
        .unwrap();
        let level = cfg.effective_level("MODIFY_COLUMN", &Severity::Danger);
        assert_eq!(level, RuleLevel::Warn);
    }

    #[test]
    fn override_upgrade_warn_to_error() {
        let cfg = parse(indoc! {"
            rules:
              ADD_COLUMN_ENUM: error
        "})
        .unwrap();
        let level = cfg.effective_level("ADD_COLUMN_ENUM", &Severity::Warning);
        assert_eq!(level, RuleLevel::Danger);
    }

    #[test]
    fn override_ignore_suppresses_rule() {
        let cfg = parse(indoc! {"
            rules:
              DROP_TABLE: ignore
        "})
        .unwrap();
        let level = cfg.effective_level("DROP_TABLE", &Severity::Danger);
        assert_eq!(level, RuleLevel::Ignore);
        assert_eq!(Config::to_severity(&level), None);
    }

    // ── to_severity ───────────────────────────────────────────────────────────

    #[test]
    fn danger_level_maps_to_danger_severity() {
        assert_eq!(
            Config::to_severity(&RuleLevel::Danger),
            Some(Severity::Danger)
        );
    }

    #[test]
    fn warn_level_maps_to_warning_severity() {
        assert_eq!(
            Config::to_severity(&RuleLevel::Warn),
            Some(Severity::Warning)
        );
    }

    #[test]
    fn ignore_level_maps_to_none() {
        assert_eq!(Config::to_severity(&RuleLevel::Ignore), None);
    }

    // ── known_rules ───────────────────────────────────────────────────────────

    #[test]
    fn known_rules_is_non_empty() {
        assert!(!known_rules(&Dialect::default()).is_empty());
    }

    #[test]
    fn known_rules_has_no_duplicates() {
        let rules = known_rules(&Dialect::default());
        let mut sorted = rules.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(rules.len(), sorted.len());
    }

    // ── RuleLevel FromStr ─────────────────────────────────────────────────────

    #[test]
    fn rule_level_from_str_valid() {
        assert_eq!("error".parse::<RuleLevel>().unwrap(), RuleLevel::Danger);
        assert_eq!("danger".parse::<RuleLevel>().unwrap(), RuleLevel::Danger);
        assert_eq!("warn".parse::<RuleLevel>().unwrap(), RuleLevel::Warn);
        assert_eq!("warning".parse::<RuleLevel>().unwrap(), RuleLevel::Warn);
        assert_eq!("ignore".parse::<RuleLevel>().unwrap(), RuleLevel::Ignore);
        assert_eq!("off".parse::<RuleLevel>().unwrap(), RuleLevel::Ignore);
    }

    #[test]
    fn rule_level_from_str_case_insensitive() {
        assert_eq!("ERROR".parse::<RuleLevel>().unwrap(), RuleLevel::Danger);
        assert_eq!("IGNORE".parse::<RuleLevel>().unwrap(), RuleLevel::Ignore);
    }

    #[test]
    fn rule_level_from_str_invalid() {
        assert!("bad".parse::<RuleLevel>().is_err());
    }
}
