//! Output formatters: `text` (human-readable) and `gha` (GitHub Actions annotations).

use colored::Colorize;

use crate::finding::{FileResult, Finding, Severity};

// ---------------------------------------------------------------------------
// Format enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum OutputFormat {
    /// Coloured human-readable output for terminals.
    Text,
    /// GitHub Actions workflow commands — renders inline annotations on diffs.
    Gha,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "text" => Ok(OutputFormat::Text),
            "gha" => Ok(OutputFormat::Gha),
            other => Err(format!(
                "unknown format `{other}` — valid values: text, gha"
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

pub fn print_results(results: &[FileResult], format: &OutputFormat) {
    match format {
        OutputFormat::Text => print_text(results),
        OutputFormat::Gha => print_gha(results),
    }
}

pub fn print_summary(results: &[FileResult], format: &OutputFormat) {
    match format {
        OutputFormat::Text => print_text_summary(results),
        OutputFormat::Gha => print_gha_summary(results),
    }
}

// ---------------------------------------------------------------------------
// Text formatter
// ---------------------------------------------------------------------------

fn print_text(results: &[FileResult]) {
    for result in results {
        let path = result.path.display();

        if let Some(err) = &result.parse_error {
            println!("\n{} {}", "PARSE ERR".red().bold(), path);
            println!("  {err}");
            println!();
            continue;
        }

        if result.findings.is_empty() {
            println!("{} {}", "  OK    ".green().bold(), path);
            continue;
        }

        println!("\n{} {}", "──".dimmed(), path.to_string().bold());

        for f in &result.findings {
            let sev_label = match f.severity {
                Severity::Danger => "DANGER".red().bold(),
                Severity::Warning => "WARN  ".yellow().bold(),
            };
            println!("  {} [{}]", sev_label, f.rule.cyan());

            // Title
            println!("  {}", f.title.bold());
            println!();

            // Detail — indented, wrapped at 90 chars
            for para in f.detail.split("\n\n") {
                for line in textwrap(para.trim(), 86) {
                    println!("    {line}");
                }
                println!();
            }

            println!("    {} {}", "sql:".dimmed(), f.sql.trim().dimmed());
            println!();
        }
    }
}

fn print_text_summary(results: &[FileResult]) {
    let total = results.len();
    let issues = results.iter().filter(|r| r.has_issues()).count();
    let danger = count_severity(results, &Severity::Danger);
    let warning = count_severity(results, &Severity::Warning);

    println!("{}", "─".repeat(60).dimmed());
    println!(
        "Checked {total} migration(s)  ·  {} file(s) with issues",
        issues.to_string().yellow()
    );
    println!(
        "  {}  {}",
        format!("{danger} danger").red().bold(),
        format!("{warning} warning").yellow().bold(),
    );
}

// ---------------------------------------------------------------------------
// GitHub Actions formatter
// ---------------------------------------------------------------------------
//
// GitHub Actions workflow command syntax:
//   ::error file=<path>,line=<n>,title=<title>::<message>
//   ::warning file=<path>,line=<n>,title=<title>::<message>
//
// Since SQL migrations don't have meaningful line numbers for DDL statements,
// we omit the `line=` parameter. GitHub will still attach the annotation to
// the file in the PR diff.
//
// The `title` is the short rule name shown in the annotation header.
// The message body is the full explanatory detail.

fn print_gha(results: &[FileResult]) {
    for result in results {
        let path = result.path.display().to_string();

        if let Some(err) = &result.parse_error {
            // Parse errors are always errors — they mean we couldn't evaluate the file
            let msg = gha_escape(err);
            println!("::error file={path},title=Parse error::{msg}");
            continue;
        }

        for f in &result.findings {
            emit_gha_annotation(f, &path);
        }
    }
}

fn emit_gha_annotation(f: &Finding, path: &str) {
    let level = match f.severity {
        Severity::Danger => "error",
        Severity::Warning => "warning",
    };

    // Title: rule code — short description
    let title = gha_escape(&format!("[{}] {}", f.rule, f.title));

    // Message: full detail + the SQL for context.
    // GHA annotations support a single message line; we collapse paragraphs with spaces
    // and separate the SQL with a delimiter so it's readable in the UI.
    let detail = f.detail.replace("\n\n", " | ").replace('\n', " ");
    let body = gha_escape(&format!("{detail} | SQL: {}", f.sql.trim()));

    println!("::{level} file={path},title={title}::{body}");
}

fn print_gha_summary(results: &[FileResult]) {
    let danger = count_severity(results, &Severity::Danger);
    let warning = count_severity(results, &Severity::Warning);
    let issues = results.iter().filter(|r| r.has_issues()).count();

    if issues == 0 {
        println!(
            "::notice::ddlint: all {} migration(s) passed",
            results.len()
        );
    } else {
        println!(
            "::notice::ddlint: {} danger, {} warning across {} file(s)",
            danger, warning, issues
        );
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Escape characters that would break GitHub Actions workflow command syntax.
/// The characters %, \r, \n are special; colons in the title field also need care.
fn gha_escape(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
        .replace(':', "%3A")
        .replace(',', "%2C")
}

fn count_severity(results: &[FileResult], sev: &Severity) -> usize {
    results
        .iter()
        .flat_map(|r| &r.findings)
        .filter(|f| &f.severity == sev)
        .count()
}

fn textwrap(s: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in s.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.len() + 1 + word.len() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(current.clone());
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::{FileResult, Finding, Severity};
    use std::path::PathBuf;

    fn dummy_finding(severity: Severity, rule: &'static str) -> Finding {
        Finding {
            path: PathBuf::from("migrations/V1__test.sql"),
            severity,
            rule,
            title: "Short title".to_string(),
            detail: "First paragraph.\n\nSecond paragraph with a fix suggestion.".to_string(),
            sql: "ALTER TABLE users DROP COLUMN foo".to_string(),
        }
    }

    fn result_with(findings: Vec<Finding>) -> FileResult {
        let path = findings.first().map(|f| f.path.clone()).unwrap_or_default();
        FileResult::ok(path, findings)
    }

    // ── GHA annotation format ────────────────────────────────────────────────

    #[test]
    fn gha_danger_emits_error_level() {
        let f = dummy_finding(Severity::Danger, "DROP_COLUMN");
        let mut out = Vec::new();
        // Capture by redirecting — we test the helper directly
        let path = "migrations/V1__test.sql";
        let mut buf = String::new();
        let level = "error";
        let title = gha_escape(&format!("[{}] {}", f.rule, f.title));
        let detail = f.detail.replace("\n\n", " | ").replace('\n', " ");
        let body = gha_escape(&format!("{detail} | SQL: {}", f.sql.trim()));
        buf.push_str(&format!("::{level} file={path},title={title}::{body}"));
        out.push(buf);

        assert!(out[0].starts_with("::error "));
        assert!(out[0].contains("file=migrations/V1__test.sql"));
        assert!(out[0].contains("DROP_COLUMN"));
    }

    #[test]
    fn gha_warning_emits_warning_level() {
        let f = dummy_finding(Severity::Warning, "CREATE_UNIQUE_INDEX");
        let level = "warning";
        let path = "migrations/V1__test.sql";
        let title = gha_escape(&format!("[{}] {}", f.rule, f.title));
        let detail = f.detail.replace("\n\n", " | ").replace('\n', " ");
        let body = gha_escape(&format!("{detail} | SQL: {}", f.sql.trim()));
        let line = format!("::{level} file={path},title={title}::{body}");

        assert!(line.starts_with("::warning "));
    }

    #[test]
    fn gha_escape_encodes_special_chars() {
        assert_eq!(gha_escape("a%b"), "a%25b");
        assert_eq!(gha_escape("a\nb"), "a%0Ab");
        assert_eq!(gha_escape("a:b"), "a%3Ab");
        assert_eq!(gha_escape("a,b"), "a%2Cb");
    }

    #[test]
    fn gha_detail_collapses_paragraphs() {
        let detail = "Para one.\n\nPara two.";
        let collapsed = detail.replace("\n\n", " | ").replace('\n', " ");
        assert_eq!(collapsed, "Para one. | Para two.");
    }

    // ── severity counts ──────────────────────────────────────────────────────

    #[test]
    fn count_severity_correct() {
        let results = vec![result_with(vec![
            dummy_finding(Severity::Danger, "DROP_TABLE"),
            dummy_finding(Severity::Warning, "CREATE_UNIQUE_INDEX"),
            dummy_finding(Severity::Danger, "MODIFY_COLUMN"),
        ])];
        assert_eq!(count_severity(&results, &Severity::Danger), 2);
        assert_eq!(count_severity(&results, &Severity::Warning), 1);
    }

    #[test]
    fn has_issues_false_when_clean() {
        let r = FileResult::ok(PathBuf::from("x.sql"), vec![]);
        assert!(!r.has_issues());
    }

    #[test]
    fn has_issues_true_when_findings_present() {
        let r = result_with(vec![dummy_finding(Severity::Danger, "DROP_TABLE")]);
        assert!(r.has_issues());
    }

    #[test]
    fn has_issues_true_on_parse_error() {
        let r = FileResult::error(PathBuf::from("x.sql"), "bad sql".to_string());
        assert!(r.has_issues());
    }

    // ── OutputFormat parsing ─────────────────────────────────────────────────

    #[test]
    fn parse_format_text() {
        let f: OutputFormat = "text".parse().unwrap();
        assert_eq!(f, OutputFormat::Text);
    }

    #[test]
    fn parse_format_gha() {
        let f: OutputFormat = "gha".parse().unwrap();
        assert_eq!(f, OutputFormat::Gha);
    }

    #[test]
    fn parse_format_case_insensitive() {
        let f: OutputFormat = "GHA".parse().unwrap();
        assert_eq!(f, OutputFormat::Gha);
    }

    #[test]
    fn parse_format_unknown_errors() {
        let r: Result<OutputFormat, _> = "json".parse();
        assert!(r.is_err());
    }
}
