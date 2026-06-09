mod analyzer;
mod config;
mod dialect;
mod dialect_rules;
mod finding;
mod output;
mod rules;

use std::path::PathBuf;
use std::process;

use anyhow::Result;
use clap::Parser;

use analyzer::{analyze_file, resolve_inputs};
use config::load_config;
use dialect::Dialect;
use finding::Severity;
use output::{print_results, print_summary, OutputFormat};

/// Lint SQL migration files for zero-downtime safety.
///
/// INPUTS can be any mix of:
///   - A directory   → recursively collects all .sql files inside it
///   - A file path   → lints that file directly
///   - A glob pattern (quoted to prevent shell expansion) → expanded internally
///
/// Examples:
///   ddlint migrations/
///   ddlint migrations/V1__init.sql migrations/V2__add_index.sql
///   ddlint 'migrations/V*.sql'
///   ddlint --dialect mysql --config ddlint.yml migrations/
///   ddlint --print-config > ddlint.yml
///
/// Exits 0 when all migrations are clean, 1 when findings are present,
/// 2 on usage or I/O errors.
#[derive(Parser, Debug)]
#[command(
    name = "ddlint",
    version,
    about = "Lint SQL migrations for zero-downtime safety"
)]
struct Cli {
    /// One or more migration files, directories, or glob patterns.
    #[arg(required_unless_present = "print_config", value_name = "INPUT")]
    inputs: Vec<String>,

    /// Path to a config file (YAML). If omitted, looks for ddlint.yml
    /// in the current directory. If that also doesn't exist, all rules run
    /// at their default severity.
    #[arg(long, short, value_name = "FILE")]
    config: Option<PathBuf>,

    /// SQL dialect / database engine.
    /// Overrides the `dialect` field in the config file.
    /// Valid values: mysql
    #[arg(long, value_name = "ENGINE")]
    dialect: Option<Dialect>,

    /// Output format.
    ///
    /// `text` — coloured human-readable output (default).
    ///
    /// `gha`  — GitHub Actions workflow commands. Emits ::error and ::warning
    ///          annotations that appear inline on PR diffs.
    #[arg(long, short, default_value = "text", value_name = "FORMAT")]
    format: OutputFormat,

    /// Treat warnings as errors (exit 1 if any warnings are present).
    #[arg(long)]
    strict: bool,

    /// Print the default configuration YAML for the active dialect and exit.
    /// Redirect to a file to create a starter config:
    ///   ddlint --print-config > ddlint.yml
    #[arg(long)]
    print_config: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load config — hard error if an explicit --config path is missing or invalid
    let mut cfg = match load_config(cli.config.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    };

    // CLI --dialect overrides whatever the config file set
    if let Some(d) = cli.dialect {
        cfg.dialect = d;
    }

    if cli.print_config {
        print_default_config(&cfg.dialect);
        return Ok(());
    }

    let (files, glob_errors) = resolve_inputs(&cli.inputs);

    for err in &glob_errors {
        eprintln!("error: {err}");
    }
    if !glob_errors.is_empty() && files.is_empty() {
        process::exit(2);
    }

    if files.is_empty() {
        eprintln!("No .sql files found for the given inputs");
        process::exit(2);
    }

    if cli.format == OutputFormat::Text {
        println!("ddlint — {} migration file(s)\n", files.len());
    }

    let results: Vec<_> = files.iter().map(|p| analyze_file(p, &cfg)).collect();

    print_results(&results, &cli.format);
    print_summary(&results, &cli.format);

    let has_danger = results.iter().any(|r| {
        r.parse_error.is_some()
            || r.findings
                .iter()
                .any(|f| f.severity == finding::Severity::Danger)
    });
    let has_warning = results
        .iter()
        .flat_map(|r| &r.findings)
        .any(|f| f.severity == finding::Severity::Warning);

    if has_danger || (cli.strict && has_warning) {
        process::exit(1);
    }

    Ok(())
}

fn print_default_config(dialect: &Dialect) {
    println!("# ddlint default configuration — all rules at their built-in severities.");
    println!("# Adjust any rule level or set to \"ignore\" to suppress it entirely.");
    println!("# Valid levels: danger (= error), warn (= warning), ignore (= off).");
    println!("dialect: {dialect}");
    println!("rules:");
    let all_rules: Vec<_> = dialect
        .rules()
        .into_iter()
        .map(|r| r.meta())
        .chain(dialect.file_rules().into_iter().map(|r| r.meta()))
        .collect();
    for meta in all_rules {
        let level = match meta.default_severity {
            Severity::Danger => "danger",
            Severity::Warning => "warn",
        };
        println!("  {}: {}", meta.id, level);
    }
}
