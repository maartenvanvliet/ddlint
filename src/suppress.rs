//! Inline suppression directives.
//!
//! Supported syntax in `.sql` files:
//!
//! ```sql
//! -- ddlint:ignore RULE_ID[, RULE_ID...]
//! ALTER TABLE ...;
//!
//! ALTER TABLE ...; -- ddlint:ignore RULE_ID
//!
//! -- ddlint:ignore-file RULE_ID[, RULE_ID...]
//! ```
//!
//! `ignore` (above or inline) suppresses the named rules for the immediately
//! following or enclosing statement.  `ignore-file` suppresses the rules for
//! the entire file and is intended for file-level rules such as
//! `MULTI_STATEMENT_MIGRATION`.

use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct Suppressions {
    /// `per_stmt[i]` — rule IDs suppressed for the i-th parsed statement.
    pub per_stmt: Vec<HashSet<String>>,
    /// Rules suppressed for the entire file.
    pub file_wide: HashSet<String>,
}

impl Suppressions {
    pub fn is_suppressed_for_stmt(&self, rule: &str, stmt_idx: usize) -> bool {
        self.per_stmt
            .get(stmt_idx)
            .map(|s| s.contains(rule))
            .unwrap_or(false)
    }

    pub fn is_suppressed_file_wide(&self, rule: &str) -> bool {
        self.file_wide.contains(rule)
    }
}

/// Scan raw SQL text and collect all `-- ddlint:ignore` directives.
pub fn parse_suppressions(sql: &str) -> Suppressions {
    let mut per_stmt: Vec<HashSet<String>> = Vec::new();
    let mut file_wide: HashSet<String> = HashSet::new();
    let mut stmt_idx: usize = 0;
    // Rules from a preceding `-- ddlint:ignore` line, applied on the next SQL line.
    let mut pending: HashSet<String> = HashSet::new();

    for raw_line in sql.lines() {
        let line = raw_line.trim();

        // ── file-wide directive (anywhere in file) ────────────────────────────
        if let Some(rules) = directive_rules(line, "-- ddlint:ignore-file") {
            file_wide.extend(rules);
            // A directive-only line has no SQL; don't count semis.
            continue;
        }

        // ── above-statement directive (directive-only line) ───────────────────
        if let Some(rules) = directive_rules(line, "-- ddlint:ignore") {
            pending.extend(rules);
            continue;
        }

        // ── SQL content line (may carry an inline directive after a `--`) ─────
        let (sql_part, inline_rules) = split_sql_and_inline_directive(line);

        // Apply pending (from a preceding directive line) to this statement.
        if !pending.is_empty() && !sql_part.trim().is_empty() {
            ensure_len(&mut per_stmt, stmt_idx + 1);
            per_stmt[stmt_idx].extend(pending.drain());
        }

        // Apply inline directive (`...; -- ddlint:ignore RULE`) to same statement.
        if let Some(rules) = inline_rules {
            ensure_len(&mut per_stmt, stmt_idx + 1);
            per_stmt[stmt_idx].extend(rules);
        }

        stmt_idx += count_statement_ends(sql_part);
    }

    Suppressions { per_stmt, file_wide }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// If `line` starts with `prefix` (the directive keyword), return the parsed
/// rule list that follows.  Returns `None` if the line does not match.
fn directive_rules(line: &str, prefix: &str) -> Option<HashSet<String>> {
    let rest = line.strip_prefix(prefix)?;
    // Reject `-- ddlint:ignore` matching the longer `-- ddlint:ignore-file`.
    if prefix == "-- ddlint:ignore" && rest.starts_with('-') {
        return None;
    }
    Some(parse_rule_list(rest))
}

/// Split a SQL line into its SQL part and any trailing inline directive.
///
/// Handles `stmt; -- ddlint:ignore RULE` and plain `-- regular comment`.
fn split_sql_and_inline_directive(line: &str) -> (&str, Option<HashSet<String>>) {
    // Find the first `--` that is not inside a string literal.
    let comment_start = find_comment_start(line);

    let (sql_part, comment_part) = match comment_start {
        Some(pos) => (&line[..pos], &line[pos..]),
        None => (line, ""),
    };

    let inline = if let Some(rules) = directive_rules(comment_part.trim(), "-- ddlint:ignore") {
        Some(rules)
    } else {
        None
    };

    (sql_part, inline)
}

/// Find the byte offset of the first `--` comment marker outside a string.
fn find_comment_start(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut in_string = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' if !in_string => in_string = true,
            b'\'' if in_string => {
                // `''` is an escaped single quote inside a string
                if bytes.get(i + 1) == Some(&b'\'') {
                    i += 1;
                } else {
                    in_string = false;
                }
            }
            b'-' if !in_string && bytes.get(i + 1) == Some(&b'-') => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

/// Count the number of statement-ending semicolons in a SQL fragment,
/// ignoring those inside string literals.
fn count_statement_ends(sql: &str) -> usize {
    let mut count = 0;
    let mut in_string = false;
    let bytes = sql.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' if !in_string => in_string = true,
            b'\'' if in_string => {
                if bytes.get(i + 1) == Some(&b'\'') {
                    i += 1;
                } else {
                    in_string = false;
                }
            }
            b';' if !in_string => count += 1,
            _ => {}
        }
        i += 1;
    }
    count
}

/// Parse a comma-separated list of rule IDs from the text that follows
/// a directive keyword.
fn parse_rule_list(s: &str) -> HashSet<String> {
    s.split(',')
        .map(|r| r.trim().to_uppercase())
        .filter(|r| !r.is_empty())
        .collect()
}

fn ensure_len(v: &mut Vec<HashSet<String>>, len: usize) {
    while v.len() < len {
        v.push(HashSet::new());
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn suppressed_for(sql: &str, stmt_idx: usize) -> Vec<String> {
        let s = parse_suppressions(sql);
        let mut rules: Vec<_> = s
            .per_stmt
            .get(stmt_idx)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect();
        rules.sort();
        rules
    }

    fn file_wide(sql: &str) -> Vec<String> {
        let s = parse_suppressions(sql);
        let mut rules: Vec<_> = s.file_wide.into_iter().collect();
        rules.sort();
        rules
    }

    #[test]
    fn above_statement_directive_applies_to_next_statement() {
        let sql = "-- ddlint:ignore DROP_TABLE\nDROP TABLE t;";
        assert_eq!(suppressed_for(sql, 0), vec!["DROP_TABLE"]);
    }

    #[test]
    fn inline_directive_applies_to_same_statement() {
        let sql = "DROP TABLE t; -- ddlint:ignore DROP_TABLE";
        assert_eq!(suppressed_for(sql, 0), vec!["DROP_TABLE"]);
    }

    #[test]
    fn file_wide_directive_is_collected() {
        let sql = "-- ddlint:ignore-file MULTI_STATEMENT_MIGRATION\nDROP TABLE t;";
        assert_eq!(file_wide(sql), vec!["MULTI_STATEMENT_MIGRATION"]);
    }

    #[test]
    fn multiple_rules_in_one_directive() {
        let sql = "-- ddlint:ignore DROP_TABLE, RENAME_TABLE\nDROP TABLE t;";
        let rules = suppressed_for(sql, 0);
        assert!(rules.contains(&"DROP_TABLE".to_string()));
        assert!(rules.contains(&"RENAME_TABLE".to_string()));
    }

    #[test]
    fn rule_ids_are_uppercased() {
        let sql = "-- ddlint:ignore drop_table\nDROP TABLE t;";
        assert_eq!(suppressed_for(sql, 0), vec!["DROP_TABLE"]);
    }

    #[test]
    fn directive_does_not_bleed_to_subsequent_statements() {
        let sql = "-- ddlint:ignore DROP_TABLE\nDROP TABLE t;\nDROP TABLE t2;";
        assert_eq!(suppressed_for(sql, 0), vec!["DROP_TABLE"]);
        assert!(suppressed_for(sql, 1).is_empty());
    }

    #[test]
    fn ignore_file_does_not_affect_per_stmt() {
        let sql = "-- ddlint:ignore-file MULTI_STATEMENT_MIGRATION\nDROP TABLE t;";
        assert!(suppressed_for(sql, 0).is_empty());
    }

    #[test]
    fn directive_on_second_statement() {
        let sql = "ALTER TABLE a ADD COLUMN x TEXT;\n-- ddlint:ignore DROP_TABLE\nDROP TABLE t;";
        assert!(suppressed_for(sql, 0).is_empty());
        assert_eq!(suppressed_for(sql, 1), vec!["DROP_TABLE"]);
    }

    #[test]
    fn semicolon_in_string_not_counted_as_statement_end() {
        // The ';' inside the string should not be counted as a statement boundary.
        let sql = "INSERT INTO t VALUES ('a;b');\n-- ddlint:ignore DROP_TABLE\nDROP TABLE t;";
        assert!(suppressed_for(sql, 0).is_empty());
        assert_eq!(suppressed_for(sql, 1), vec!["DROP_TABLE"]);
    }
}
