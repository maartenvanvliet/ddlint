use std::fs;
use std::process::Command;

use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ddlint() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ddlint"));
    // Disable ANSI colour codes so we can match plain text
    cmd.env("NO_COLOR", "1");
    cmd
}

fn write_sql(dir: &TempDir, name: &str, sql: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, sql).unwrap();
    path
}

// ---------------------------------------------------------------------------
// Exit codes
// ---------------------------------------------------------------------------

#[test]
fn clean_file_exits_zero() {
    let dir = TempDir::new().unwrap();
    let f = write_sql(&dir, "safe.sql", "ALTER TABLE users ADD COLUMN notes TEXT;");
    let status = ddlint().arg(f).status().unwrap();
    assert_eq!(status.code(), Some(0));
}

#[test]
fn dangerous_sql_exits_one() {
    let dir = TempDir::new().unwrap();
    let f = write_sql(&dir, "danger.sql", "DROP TABLE legacy;");
    let status = ddlint().arg(f).status().unwrap();
    assert_eq!(status.code(), Some(1));
}

#[test]
fn warning_sql_exits_zero_without_strict() {
    let dir = TempDir::new().unwrap();
    let f = write_sql(&dir, "warn.sql", "CREATE UNIQUE INDEX idx ON users(email);");
    let status = ddlint().arg(f).status().unwrap();
    assert_eq!(status.code(), Some(0));
}

#[test]
fn warning_sql_exits_one_with_strict() {
    let dir = TempDir::new().unwrap();
    let f = write_sql(&dir, "warn.sql", "CREATE UNIQUE INDEX idx ON users(email);");
    let status = ddlint().arg("--strict").arg(f).status().unwrap();
    assert_eq!(status.code(), Some(1));
}

#[test]
fn missing_file_exits_one() {
    // A path that resolves but cannot be read is reported as a parse error → exit 1.
    // Exit 2 is reserved for glob/arg errors where no files could be resolved at all.
    let status = ddlint().arg("/nonexistent/path/file.sql").status().unwrap();
    assert_eq!(status.code(), Some(1));
}

#[test]
fn nonexistent_explicit_config_exits_two() {
    let dir = TempDir::new().unwrap();
    let f = write_sql(&dir, "safe.sql", "ALTER TABLE t ADD COLUMN x TEXT;");
    let status = ddlint()
        .arg("--config")
        .arg("/nonexistent/ddlint.yml")
        .arg(f)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
}

// ---------------------------------------------------------------------------
// Text output
// ---------------------------------------------------------------------------

#[test]
fn text_output_shows_ok_for_clean_file() {
    let dir = TempDir::new().unwrap();
    let f = write_sql(
        &dir,
        "safe.sql",
        "ALTER TABLE users ADD COLUMN notes TEXT, ALGORITHM=INSTANT;",
    );
    let out = ddlint().arg(f).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("OK"),
        "expected OK in output, got:\n{stdout}"
    );
}

#[test]
fn text_output_shows_rule_name_for_danger() {
    let dir = TempDir::new().unwrap();
    let f = write_sql(&dir, "danger.sql", "DROP TABLE legacy;");
    let out = ddlint().arg(f).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("DROP_TABLE"),
        "expected DROP_TABLE in output, got:\n{stdout}"
    );
}

#[test]
fn text_output_shows_danger_label() {
    let dir = TempDir::new().unwrap();
    let f = write_sql(&dir, "danger.sql", "DROP TABLE legacy;");
    let out = ddlint().arg(f).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("DANGER"),
        "expected DANGER label in output, got:\n{stdout}"
    );
}

#[test]
fn text_output_shows_warn_label_for_warning() {
    let dir = TempDir::new().unwrap();
    let f = write_sql(&dir, "warn.sql", "CREATE UNIQUE INDEX idx ON users(email);");
    let out = ddlint().arg(f).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("WARN"),
        "expected WARN label in output, got:\n{stdout}"
    );
}

#[test]
fn text_output_includes_summary_line() {
    let dir = TempDir::new().unwrap();
    let f = write_sql(&dir, "danger.sql", "DROP TABLE legacy;");
    let out = ddlint().arg(f).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Summary always ends with migration(s) / file(s) with issues counts
    assert!(
        stdout.contains("migration(s)"),
        "expected summary in output, got:\n{stdout}"
    );
}

#[test]
fn text_output_summary_counts_danger_and_warning() {
    let dir = TempDir::new().unwrap();
    // Single statement: ADD_COLUMN_NOT_NULL_NO_DEFAULT (danger) +
    // ADD_COLUMN_NO_ALGORITHM_INSTANT (warning) — avoids MULTI_STATEMENT_MIGRATION.
    let f = write_sql(
        &dir,
        "mixed.sql",
        "ALTER TABLE users ADD COLUMN role VARCHAR(50) NOT NULL;",
    );
    let out = ddlint().arg(f).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("1 danger"),
        "expected '1 danger' in summary, got:\n{stdout}"
    );
    assert!(
        stdout.contains("1 warning"),
        "expected '1 warning' in summary, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// GHA format
// ---------------------------------------------------------------------------

#[test]
fn gha_danger_emits_error_annotation() {
    let dir = TempDir::new().unwrap();
    let f = write_sql(&dir, "danger.sql", "DROP TABLE legacy;");
    let out = ddlint().arg("--format").arg("gha").arg(f).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("::error "),
        "expected ::error annotation, got:\n{stdout}"
    );
    assert!(
        stdout.contains("DROP_TABLE"),
        "expected DROP_TABLE rule in annotation, got:\n{stdout}"
    );
}

#[test]
fn gha_warning_emits_warning_annotation() {
    let dir = TempDir::new().unwrap();
    let f = write_sql(&dir, "warn.sql", "CREATE UNIQUE INDEX idx ON users(email);");
    let out = ddlint().arg("--format").arg("gha").arg(f).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("::warning "),
        "expected ::warning annotation, got:\n{stdout}"
    );
}

#[test]
fn gha_clean_file_produces_no_annotations() {
    let dir = TempDir::new().unwrap();
    let f = write_sql(
        &dir,
        "safe.sql",
        "ALTER TABLE users ADD COLUMN notes TEXT, ALGORITHM=INSTANT;",
    );
    let out = ddlint().arg("--format").arg("gha").arg(f).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("::error"),
        "unexpected ::error for clean file, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("::warning"),
        "unexpected ::warning for clean file, got:\n{stdout}"
    );
}

#[test]
fn gha_annotation_includes_file_path() {
    let dir = TempDir::new().unwrap();
    let f = write_sql(&dir, "danger.sql", "DROP TABLE legacy;");
    let out = ddlint()
        .arg("--format")
        .arg("gha")
        .arg(&f)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("danger.sql"),
        "expected file name in annotation, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[test]
fn config_ignore_suppresses_rule() {
    let dir = TempDir::new().unwrap();
    let cfg = dir.path().join("ddlint.yml");
    fs::write(&cfg, "rules:\n  DROP_TABLE: ignore\n").unwrap();
    let f = write_sql(&dir, "drop.sql", "DROP TABLE legacy;");
    let out = ddlint().arg("--config").arg(&cfg).arg(f).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("DROP_TABLE"),
        "rule should be suppressed, got:\n{stdout}"
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "exit should be 0 when only rule is suppressed"
    );
}

#[test]
fn config_downgrade_sets_warn_severity() {
    let dir = TempDir::new().unwrap();
    let cfg = dir.path().join("ddlint.yml");
    fs::write(&cfg, "rules:\n  DROP_TABLE: warn\n").unwrap();
    let f = write_sql(&dir, "drop.sql", "DROP TABLE legacy;");
    let out = ddlint().arg("--config").arg(&cfg).arg(f).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("WARN"),
        "expected WARN after downgrade, got:\n{stdout}"
    );
    // Without --strict a warning exits 0
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn config_upgrade_promotes_to_danger() {
    let dir = TempDir::new().unwrap();
    let cfg = dir.path().join("ddlint.yml");
    fs::write(&cfg, "rules:\n  CREATE_UNIQUE_INDEX: error\n").unwrap();
    let f = write_sql(&dir, "idx.sql", "CREATE UNIQUE INDEX idx ON users(email);");
    let out = ddlint().arg("--config").arg(&cfg).arg(f).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "upgraded rule should exit 1");
}

#[test]
fn config_dialect_field_accepted() {
    let dir = TempDir::new().unwrap();
    let cfg = dir.path().join("ddlint.yml");
    fs::write(&cfg, "dialect: mysql\nrules: {}\n").unwrap();
    let f = write_sql(&dir, "safe.sql", "ALTER TABLE t ADD COLUMN x TEXT;");
    let status = ddlint().arg("--config").arg(&cfg).arg(f).status().unwrap();
    assert_eq!(status.code(), Some(0));
}

// ---------------------------------------------------------------------------
// Dialect flag
// ---------------------------------------------------------------------------

#[test]
fn dialect_flag_mysql_accepted() {
    let dir = TempDir::new().unwrap();
    let f = write_sql(&dir, "safe.sql", "ALTER TABLE t ADD COLUMN x TEXT;");
    let status = ddlint()
        .arg("--dialect")
        .arg("mysql")
        .arg(f)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(0));
}

#[test]
fn dialect_flag_unknown_exits_two() {
    let dir = TempDir::new().unwrap();
    let f = write_sql(&dir, "safe.sql", "ALTER TABLE t ADD COLUMN x TEXT;");
    let status = ddlint()
        .arg("--dialect")
        .arg("oracle")
        .arg(f)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
}

// ---------------------------------------------------------------------------
// Directory and glob inputs
// ---------------------------------------------------------------------------

#[test]
fn directory_input_recurses_for_sql_files() {
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("v1");
    fs::create_dir(&sub).unwrap();
    fs::write(
        sub.join("migration.sql"),
        "ALTER TABLE t ADD COLUMN x TEXT;",
    )
    .unwrap();
    let out = ddlint().arg(dir.path()).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("1 migration(s)"),
        "expected 1 migration found, got:\n{stdout}"
    );
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn directory_with_no_sql_files_exits_two() {
    let dir = TempDir::new().unwrap();
    let status = ddlint().arg(dir.path()).status().unwrap();
    assert_eq!(status.code(), Some(2));
}

#[test]
fn glob_input_matches_sql_files() {
    let dir = TempDir::new().unwrap();
    write_sql(&dir, "v1.sql", "ALTER TABLE t ADD COLUMN x TEXT;");
    write_sql(&dir, "v2.sql", "ALTER TABLE t ADD COLUMN y TEXT;");
    let pattern = dir.path().join("*.sql").to_string_lossy().to_string();
    let out = ddlint().arg(pattern).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("2 migration(s)"),
        "expected 2 migrations, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// Multiple files
// ---------------------------------------------------------------------------

#[test]
fn multiple_explicit_files_all_checked() {
    let dir = TempDir::new().unwrap();
    let f1 = write_sql(&dir, "v1.sql", "ALTER TABLE t ADD COLUMN x TEXT;");
    let f2 = write_sql(&dir, "v2.sql", "DROP TABLE legacy;");
    let out = ddlint().arg(&f1).arg(&f2).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("2 migration(s)"),
        "expected 2 migrations, got:\n{stdout}"
    );
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn one_clean_one_dirty_shows_correct_counts() {
    let dir = TempDir::new().unwrap();
    let f1 = write_sql(
        &dir,
        "v1.sql",
        "ALTER TABLE t ADD COLUMN x TEXT, ALGORITHM=INSTANT;",
    );
    let f2 = write_sql(&dir, "v2.sql", "DROP TABLE legacy;");
    let out = ddlint().arg(&f1).arg(&f2).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("1 file(s) with issues"),
        "expected 1 file with issues, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// --print-config
// ---------------------------------------------------------------------------

#[test]
fn print_config_outputs_yaml() {
    let out = ddlint().arg("--print-config").output().unwrap();
    assert!(
        out.status.success(),
        "expected exit 0, got {:?}",
        out.status.code()
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("dialect:"),
        "expected 'dialect:' in output, got:\n{stdout}"
    );
    assert!(
        stdout.contains("rules:"),
        "expected 'rules:' in output, got:\n{stdout}"
    );
}

#[test]
fn print_config_requires_no_inputs() {
    // --print-config should succeed without any INPUT arguments
    let out = ddlint().arg("--print-config").output().unwrap();
    assert!(
        out.status.success(),
        "expected exit 0 with no inputs, got {:?}",
        out.status.code()
    );
}

#[test]
fn print_config_lists_all_rules() {
    let out = ddlint().arg("--print-config").output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    for rule in &[
        "ADD_COLUMN_NOT_NULL_NO_DEFAULT",
        "ADD_COLUMN_NO_ALGORITHM_INSTANT",
        "ADD_COLUMN_ENUM",
        "MODIFY_COLUMN",
        "MODIFY_COLUMN_ENUM",
        "CHANGE_COLUMN",
        "CHANGE_COLUMN_ENUM",
        "RENAME_COLUMN",
        "RENAME_TABLE",
        "DROP_COLUMN",
        "ADD_PRIMARY_KEY",
        "DROP_PRIMARY_KEY",
        "ADD_FOREIGN_KEY",
        "ADD_UNIQUE_CONSTRAINT",
        "CREATE_UNIQUE_INDEX",
        "DROP_TABLE",
        "TRUNCATE",
        "LOCK_TABLES",
    ] {
        assert!(
            stdout.contains(rule),
            "expected rule '{rule}' in --print-config output, got:\n{stdout}"
        );
    }
}

#[test]
fn print_config_dialect_flag_changes_dialect_field() {
    let out = ddlint()
        .arg("--dialect")
        .arg("mysql")
        .arg("--print-config")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("dialect: mysql"),
        "expected 'dialect: mysql', got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// Parse errors
// ---------------------------------------------------------------------------

#[test]
fn unparseable_sql_exits_one() {
    let dir = TempDir::new().unwrap();
    let f = write_sql(&dir, "bad.sql", "THIS IS NOT SQL @@@@");
    let status = ddlint().arg(f).status().unwrap();
    assert_eq!(status.code(), Some(1));
}

#[test]
fn gha_parse_error_emits_error_annotation() {
    let dir = TempDir::new().unwrap();
    let f = write_sql(&dir, "bad.sql", "THIS IS NOT SQL @@@@");
    let out = ddlint().arg("--format").arg("gha").arg(f).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("::error"),
        "expected ::error for parse error, got:\n{stdout}"
    );
}
