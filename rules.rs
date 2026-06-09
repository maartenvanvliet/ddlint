//! Lint rules for MySQL zero-downtime migration safety.
//!
//! Rule functions push findings at their built-in default severity.
//! `check_statement` applies the [`Config`] as a post-pass, overriding
//! severity or suppressing findings entirely.

use std::path::Path;

use sqlparser::ast::{
    AlterTableOperation, ColumnOption, ObjectType, Statement, TableConstraint,
};

use crate::config::Config;
use crate::finding::{Finding, Severity};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run all rules against a single parsed statement, applying `config` overrides.
pub fn check_statement(stmt: &Statement, path: &Path, config: &Config) -> Vec<Finding> {
    let mut raw = Vec::new();
    check_alter_table(stmt, path, &mut raw);
    check_create_index(stmt, path, &mut raw);
    check_drop(stmt, path, &mut raw);
    check_truncate(stmt, path, &mut raw);

    // Apply config: override severity or suppress entirely
    raw.into_iter()
        .filter_map(|mut f| {
            let level = config.effective_level(f.rule, &f.severity);
            match Config::to_severity(&level) {
                None      => None,
                Some(sev) => { f.severity = sev; Some(f) }
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// ALTER TABLE
// ---------------------------------------------------------------------------

fn check_alter_table(stmt: &Statement, path: &Path, out: &mut Vec<Finding>) {
    let Statement::AlterTable { name, operations, .. } = stmt else { return };
    let table = name.to_string();

    for op in operations {
        match op {
            AlterTableOperation::AddColumn { column_def, .. } => {
                let col = column_def.name.to_string();
                let not_null = column_def.options.iter().any(|o| matches!(o.option, ColumnOption::NotNull));
                let has_default = column_def.options.iter().any(|o| matches!(o.option, ColumnOption::Default(_)));
                let is_enum = format!("{}", column_def.data_type).to_uppercase().starts_with("ENUM");

                if not_null && !has_default {
                    out.push(Finding {
                        path: path.to_path_buf(),
                        severity: Severity::Danger,
                        rule: "ADD_COLUMN_NOT_NULL_NO_DEFAULT",
                        title: format!("ADD COLUMN `{col}` on `{table}` is NOT NULL with no DEFAULT"),
                        detail: format!(
                            "Adding a NOT NULL column without a DEFAULT requires MySQL to \
                             verify or backfill every existing row before the migration \
                             completes. On MySQL < 8.0 this takes an ACCESS EXCLUSIVE lock \
                             for the full duration — all reads and writes are blocked. On \
                             MySQL 8.0+ it runs INPLACE but is still slow on large tables.\n\
                             \n\
                             Fix: add a DEFAULT value (e.g. DEFAULT '' or DEFAULT 0) so \
                             MySQL can apply the change instantly, then tighten the \
                             constraint in a separate migration once the column is populated."
                        ),
                        sql: stmt.to_string(),
                    });
                }

                if is_enum {
                    out.push(Finding {
                        path: path.to_path_buf(),
                        severity: Severity::Warning,
                        rule: "ADD_COLUMN_ENUM",
                        title: format!("ADD COLUMN `{col}` on `{table}` uses ENUM"),
                        detail: format!(
                            "ENUM columns always use ALGORITHM=COPY in MySQL, which rebuilds \
                             the entire table and holds a write lock for the duration. This \
                             applies even when adding a new ENUM column, not just when \
                             modifying an existing one.\n\
                             \n\
                             Fix: use VARCHAR with an application-level or CHECK constraint \
                             instead, or add a separate lookup table with a foreign key. \
                             Either approach avoids the table rebuild."
                        ),
                        sql: stmt.to_string(),
                    });
                }
            }

            AlterTableOperation::ModifyColumn { col_name, data_type, .. } => {
                let is_enum = format!("{data_type}").to_uppercase().starts_with("ENUM");

                out.push(Finding {
                    path: path.to_path_buf(),
                    severity: Severity::Danger,
                    rule: "MODIFY_COLUMN",
                    title: format!("MODIFY COLUMN `{col_name}` on `{table}` may rebuild the table"),
                    detail: format!(
                        "MODIFY COLUMN changes the column definition in-place when MySQL \
                         determines the storage format is compatible (ALGORITHM=INPLACE), \
                         but falls back to a full table rebuild (ALGORITHM=COPY) for many \
                         common changes: type changes, charset changes, changing nullability \
                         without a DEFAULT, or reordering columns.\n\
                         \n\
                         A COPY rebuild holds a write lock for the entire duration and \
                         creates a full copy of the table on disk — dangerous for any table \
                         over a few hundred MB.\n\
                         \n\
                         Fix: add ALGORITHM=INPLACE, LOCK=NONE to fail fast if MySQL would \
                         fall back to COPY. For changes that genuinely require COPY, use \
                         pt-online-schema-change or gh-ost to do the rebuild online."
                    ),
                    sql: stmt.to_string(),
                });

                if is_enum {
                    out.push(Finding {
                        path: path.to_path_buf(),
                        severity: Severity::Danger,
                        rule: "MODIFY_COLUMN_ENUM",
                        title: format!("MODIFY COLUMN `{col_name}` on `{table}` is ENUM — always ALGORITHM=COPY"),
                        detail: format!(
                            "Any modification to an ENUM column forces ALGORITHM=COPY \
                             regardless of what changed. Even adding a new valid value \
                             triggers a full table rebuild with a write lock.\n\
                             \n\
                             Fix: migrate ENUM columns to VARCHAR. Use pt-online-schema-change \
                             to do the initial conversion without downtime."
                        ),
                        sql: stmt.to_string(),
                    });
                }
            }

            AlterTableOperation::ChangeColumn { old_name, new_name, data_type, .. } => {
                let is_enum = format!("{data_type}").to_uppercase().starts_with("ENUM");

                out.push(Finding {
                    path: path.to_path_buf(),
                    severity: Severity::Danger,
                    rule: "CHANGE_COLUMN",
                    title: format!("CHANGE COLUMN `{old_name}` → `{new_name}` on `{table}` renames a column"),
                    detail: format!(
                        "CHANGE COLUMN renames `{old_name}` to `{new_name}`. Any app code \
                         still using the old column name will fail immediately after the \
                         migration runs — before the new deployment is complete. In a \
                         rolling deploy this breaks in-flight requests and old pods.\n\
                         \n\
                         Fix: use the expand-contract (parallel change) pattern: \
                         (1) add `{new_name}` as a nullable column, \
                         (2) dual-write to both columns in the application, \
                         (3) backfill `{new_name}` from `{old_name}`, \
                         (4) switch reads to `{new_name}`, \
                         (5) drop `{old_name}` in a later migration."
                    ),
                    sql: stmt.to_string(),
                });

                if is_enum {
                    out.push(Finding {
                        path: path.to_path_buf(),
                        severity: Severity::Danger,
                        rule: "CHANGE_COLUMN_ENUM",
                        title: format!("CHANGE COLUMN `{old_name}` on `{table}` uses ENUM — always ALGORITHM=COPY"),
                        detail: "ENUM columns always force a full table rebuild. See CHANGE_COLUMN and ADD_COLUMN_ENUM findings.".to_string(),
                        sql: stmt.to_string(),
                    });
                }
            }

            AlterTableOperation::RenameColumn { old_column_name, new_column_name } => {
                out.push(Finding {
                    path: path.to_path_buf(),
                    severity: Severity::Danger,
                    rule: "RENAME_COLUMN",
                    title: format!("RENAME COLUMN `{old_column_name}` → `{new_column_name}` on `{table}`"),
                    detail: format!(
                        "Renaming `{old_column_name}` to `{new_column_name}` is atomic in \
                         MySQL 8.0+ (ALGORITHM=INSTANT) so it won't lock, but it immediately \
                         breaks any live app code still reading `{old_column_name}`. In a \
                         rolling deploy, old pods will start failing before new pods are up.\n\
                         \n\
                         Fix: use the expand-contract pattern — add the new column name, \
                         dual-write, backfill, switch reads, then drop the old column."
                    ),
                    sql: stmt.to_string(),
                });
            }

            AlterTableOperation::RenameTable { table_name } => {
                out.push(Finding {
                    path: path.to_path_buf(),
                    severity: Severity::Danger,
                    rule: "RENAME_TABLE",
                    title: format!("RENAME TABLE `{table}` → `{table_name}`"),
                    detail: format!(
                        "Renaming `{table}` to `{table_name}` breaks all live app code \
                         referencing the old table name immediately. Foreign keys pointing \
                         to `{table}` are also affected.\n\
                         \n\
                         Fix: if this is a permanent rename, use the expand-contract \
                         pattern with views as a compatibility shim: create `{table_name}`, \
                         replace `{table}` with a view, migrate the application, then \
                         drop the view."
                    ),
                    sql: stmt.to_string(),
                });
            }

            AlterTableOperation::DropColumn { column_name, .. } => {
                out.push(Finding {
                    path: path.to_path_buf(),
                    severity: Severity::Danger,
                    rule: "DROP_COLUMN",
                    title: format!("DROP COLUMN `{column_name}` on `{table}` is irreversible"),
                    detail: format!(
                        "Dropping `{column_name}` is instant in MySQL 8.0 (ALGORITHM=INSTANT) \
                         but permanently destroys the data and breaks any live app code still \
                         reading or writing it. In a rolling deploy, old pods will crash \
                         immediately.\n\
                         \n\
                         Fix: ensure all application code has been deployed without any \
                         reference to `{column_name}` before running this migration. The \
                         column should have been ignored by the ORM/queries for at least \
                         one full deploy cycle."
                    ),
                    sql: stmt.to_string(),
                });
            }

            AlterTableOperation::DropPrimaryKey => {
                out.push(Finding {
                    path: path.to_path_buf(),
                    severity: Severity::Danger,
                    rule: "DROP_PRIMARY_KEY",
                    title: format!("DROP PRIMARY KEY on `{table}` requires a full table rebuild"),
                    detail: "Dropping the primary key requires ALGORITHM=COPY — a full table \
                             rebuild with a write lock. InnoDB tables are clustered on the \
                             primary key, so removing it forces a complete reorganisation of \
                             the on-disk data structure.\n\
                             \n\
                             Fix: use pt-online-schema-change or gh-ost. Avoid dropping \
                             primary keys on large tables if at all possible.".to_string(),
                    sql: stmt.to_string(),
                });
            }

            AlterTableOperation::AddConstraint(constraint) => {
                match constraint {
                    TableConstraint::ForeignKey { name, .. } => {
                        let fk = name.as_ref().map(|n| n.to_string()).unwrap_or_else(|| "<unnamed>".into());
                        out.push(Finding {
                            path: path.to_path_buf(),
                            severity: Severity::Danger,
                            rule: "ADD_FOREIGN_KEY",
                            title: format!("ADD FOREIGN KEY `{fk}` on `{table}` acquires a metadata lock"),
                            detail: format!(
                                "Adding a foreign key causes MySQL to validate that every \
                                 existing row in `{table}` satisfies the constraint. During \
                                 this scan MySQL holds a metadata lock that blocks all DDL \
                                 on both the referencing and referenced tables. On large \
                                 tables this can take minutes.\n\
                                 \n\
                                 Fix: (1) add the supporting index as a separate migration \
                                 first (CREATE INDEX is online), (2) only add the FK \
                                 constraint once the index exists, preferably during a \
                                 low-traffic window. If referential integrity can be enforced \
                                 at the application layer, consider omitting the FK entirely."
                            ),
                            sql: stmt.to_string(),
                        });
                    }
                    TableConstraint::Unique { name, .. } => {
                        let idx = name.as_ref().map(|n| n.to_string()).unwrap_or_else(|| "<unnamed>".into());
                        out.push(Finding {
                            path: path.to_path_buf(),
                            severity: Severity::Warning,
                            rule: "ADD_UNIQUE_CONSTRAINT",
                            title: format!("ADD UNIQUE KEY `{idx}` on `{table}` requires a full duplicate scan"),
                            detail: format!(
                                "Building the unique index requires MySQL to read and sort \
                                 the entire table to check for duplicates. This is done \
                                 online (reads allowed) but writes are blocked once the \
                                 index build finishes and the lock is promoted. The migration \
                                 will also fail outright if any duplicate values exist.\n\
                                 \n\
                                 Fix: check for duplicates before running this migration \
                                 (SELECT {col}, COUNT(*) ... GROUP BY ... HAVING COUNT(*) > 1). \
                                 Run during a low-traffic window on large tables.",
                                col = "column_name"
                            ),
                            sql: stmt.to_string(),
                        });
                    }
                    _ => {}
                }
            }

            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// CREATE INDEX
// ---------------------------------------------------------------------------

fn check_create_index(stmt: &Statement, path: &Path, out: &mut Vec<Finding>) {
    let Statement::CreateIndex(ci) = stmt else { return };
    let table = ci.table_name.to_string();
    let idx   = ci.name.as_ref().map(|n| n.to_string()).unwrap_or_else(|| "<unnamed>".into());

    if ci.unique {
        out.push(Finding {
            path: path.to_path_buf(),
            severity: Severity::Warning,
            rule: "CREATE_UNIQUE_INDEX",
            title: format!("CREATE UNIQUE INDEX `{idx}` on `{table}` requires a duplicate scan"),
            detail: format!(
                "Building a unique index requires a full table read to verify there are \
                 no duplicate values. The index build itself is online, but the migration \
                 will fail if duplicates exist at the time it runs.\n\
                 \n\
                 Fix: run SELECT COUNT(*) vs SELECT COUNT(DISTINCT col) first to confirm \
                 uniqueness. If cleaning up duplicates, do that in a prior migration."
            ),
            sql: stmt.to_string(),
        });
    }
}

// ---------------------------------------------------------------------------
// DROP TABLE / DROP INDEX
// ---------------------------------------------------------------------------

fn check_drop(stmt: &Statement, path: &Path, out: &mut Vec<Finding>) {
    let Statement::Drop { object_type: ObjectType::Table, names, .. } = stmt else { return };

    for name in names {
        out.push(Finding {
            path: path.to_path_buf(),
            severity: Severity::Danger,
            rule: "DROP_TABLE",
            title: format!("DROP TABLE `{name}` is irreversible"),
            detail: format!(
                "Dropping `{name}` permanently destroys all data and the table definition. \
                 Any live app code referencing `{name}` will immediately start throwing \
                 errors. In a rolling deploy this affects old pods that haven't been \
                 restarted yet.\n\
                 \n\
                 Fix: ensure all application code referencing `{name}` has been removed \
                 and fully deployed before running this migration. Consider renaming the \
                 table first and waiting one deploy cycle to confirm nothing breaks."
            ),
            sql: stmt.to_string(),
        });
    }
}

// ---------------------------------------------------------------------------
// TRUNCATE
// ---------------------------------------------------------------------------

fn check_truncate(stmt: &Statement, path: &Path, out: &mut Vec<Finding>) {
    let Statement::Truncate { table_names, .. } = stmt else { return };

    for tbl in table_names {
        let name = &tbl.name;
        out.push(Finding {
            path: path.to_path_buf(),
            severity: Severity::Danger,
            rule: "TRUNCATE",
            title: format!("TRUNCATE `{name}` destroys all rows"),
            detail: format!(
                "TRUNCATE deletes every row from `{name}` and acquires a metadata lock \
                 for the duration. Unlike DELETE it cannot be rolled back in the same \
                 transaction in MySQL (it causes an implicit commit).\n\
                 \n\
                 This is almost always a mistake in a schema migration. If you need to \
                 clear data as part of a migration, use DELETE with a WHERE clause so \
                 the operation is scoped and transactional."
            ),
            sql: stmt.to_string(),
        });
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use sqlparser::dialect::MySqlDialect;
    use sqlparser::parser::Parser;
    use std::path::PathBuf;

    fn parse_and_check(sql: &str) -> Vec<Finding> {
        parse_and_check_with(sql, &crate::config::Config::default())
    }

    fn parse_and_check_with(sql: &str, config: &crate::config::Config) -> Vec<Finding> {
        let dialect = MySqlDialect {};
        let stmts = Parser::parse_sql(&dialect, sql).expect("parse failed");
        let path = PathBuf::from("test.sql");
        stmts.iter().flat_map(|s| check_statement(s, &path, config)).collect()
    }

    fn rules(findings: &[Finding]) -> Vec<&str> {
        findings.iter().map(|f| f.rule).collect()
    }

    // ── ADD COLUMN ───────────────────────────────────────────────────────────

    #[test]
    fn add_column_nullable_is_safe() {
        let f = parse_and_check("ALTER TABLE users ADD COLUMN notes TEXT");
        assert!(f.is_empty(), "nullable add column should be clean, got: {f:?}");
    }

    #[test]
    fn add_column_not_null_with_default_is_safe() {
        let f = parse_and_check(
            "ALTER TABLE users ADD COLUMN status VARCHAR(50) NOT NULL DEFAULT 'active'"
        );
        assert!(f.is_empty(), "NOT NULL with DEFAULT should be safe, got: {f:?}");
    }

    #[test]
    fn add_column_not_null_no_default_is_danger() {
        let f = parse_and_check("ALTER TABLE users ADD COLUMN role VARCHAR(50) NOT NULL");
        assert!(rules(&f).contains(&"ADD_COLUMN_NOT_NULL_NO_DEFAULT"));
    }

    #[test]
    fn add_column_enum_is_warning() {
        let f = parse_and_check("ALTER TABLE users ADD COLUMN kind ENUM('a','b') DEFAULT 'a'");
        assert!(rules(&f).contains(&"ADD_COLUMN_ENUM"));
        // has a DEFAULT so no NOT_NULL_NO_DEFAULT
        assert!(!rules(&f).contains(&"ADD_COLUMN_NOT_NULL_NO_DEFAULT"));
    }

    #[test]
    fn add_column_enum_not_null_no_default_gets_both_rules() {
        let f = parse_and_check("ALTER TABLE users ADD COLUMN kind ENUM('a','b') NOT NULL");
        let r = rules(&f);
        assert!(r.contains(&"ADD_COLUMN_NOT_NULL_NO_DEFAULT"));
        assert!(r.contains(&"ADD_COLUMN_ENUM"));
    }

    // ── MODIFY COLUMN ────────────────────────────────────────────────────────

    #[test]
    fn modify_column_is_danger() {
        let f = parse_and_check("ALTER TABLE users MODIFY COLUMN email TEXT NOT NULL");
        assert!(rules(&f).contains(&"MODIFY_COLUMN"));
    }

    #[test]
    fn modify_column_enum_gets_both_rules() {
        let f = parse_and_check("ALTER TABLE users MODIFY COLUMN status ENUM('a','b')");
        let r = rules(&f);
        assert!(r.contains(&"MODIFY_COLUMN"));
        assert!(r.contains(&"MODIFY_COLUMN_ENUM"));
    }

    // ── CHANGE COLUMN ────────────────────────────────────────────────────────

    #[test]
    fn change_column_is_danger() {
        let f = parse_and_check(
            "ALTER TABLE users CHANGE COLUMN status account_status VARCHAR(50)"
        );
        assert!(rules(&f).contains(&"CHANGE_COLUMN"));
    }

    #[test]
    fn change_column_enum_gets_both_rules() {
        let f = parse_and_check(
            "ALTER TABLE users CHANGE COLUMN status account_status ENUM('a','b')"
        );
        let r = rules(&f);
        assert!(r.contains(&"CHANGE_COLUMN"));
        assert!(r.contains(&"CHANGE_COLUMN_ENUM"));
    }

    // ── RENAME ───────────────────────────────────────────────────────────────

    #[test]
    fn rename_column_is_danger() {
        let f = parse_and_check(
            "ALTER TABLE users RENAME COLUMN old_name TO new_name"
        );
        assert!(rules(&f).contains(&"RENAME_COLUMN"));
    }

    #[test]
    fn rename_table_is_danger() {
        let f = parse_and_check("ALTER TABLE users RENAME TO accounts");
        assert!(rules(&f).contains(&"RENAME_TABLE"));
    }

    // ── DROP ─────────────────────────────────────────────────────────────────

    #[test]
    fn drop_column_is_danger() {
        let f = parse_and_check("ALTER TABLE users DROP COLUMN legacy_field");
        assert!(rules(&f).contains(&"DROP_COLUMN"));
    }

    #[test]
    fn drop_primary_key_is_danger() {
        let f = parse_and_check("ALTER TABLE users DROP PRIMARY KEY");
        assert!(rules(&f).contains(&"DROP_PRIMARY_KEY"));
    }

    #[test]
    fn drop_table_is_danger() {
        let f = parse_and_check("DROP TABLE legacy_tokens");
        assert!(rules(&f).contains(&"DROP_TABLE"));
    }

    #[test]
    fn drop_table_if_exists_is_danger() {
        let f = parse_and_check("DROP TABLE IF EXISTS legacy_tokens");
        assert!(rules(&f).contains(&"DROP_TABLE"));
    }

    // ── CONSTRAINTS ──────────────────────────────────────────────────────────

    #[test]
    fn add_foreign_key_is_danger() {
        let f = parse_and_check(
            "ALTER TABLE orders ADD FOREIGN KEY (user_id) REFERENCES users(id)"
        );
        assert!(rules(&f).contains(&"ADD_FOREIGN_KEY"));
    }

    #[test]
    fn add_unique_constraint_is_warning() {
        let f = parse_and_check(
            "ALTER TABLE users ADD UNIQUE KEY idx_email (email)"
        );
        assert!(rules(&f).contains(&"ADD_UNIQUE_CONSTRAINT"));
        assert_eq!(f[0].severity, Severity::Warning);
    }

    // ── CREATE INDEX ─────────────────────────────────────────────────────────

    #[test]
    fn create_index_non_unique_is_safe() {
        let f = parse_and_check("CREATE INDEX idx_email ON users(email)");
        assert!(f.is_empty(), "non-unique index should be clean, got: {f:?}");
    }

    #[test]
    fn create_unique_index_is_warning() {
        let f = parse_and_check("CREATE UNIQUE INDEX idx_email ON users(email)");
        assert!(rules(&f).contains(&"CREATE_UNIQUE_INDEX"));
        assert_eq!(f[0].severity, Severity::Warning);
    }

    // ── TRUNCATE ─────────────────────────────────────────────────────────────

    #[test]
    fn truncate_is_danger() {
        let f = parse_and_check("TRUNCATE TABLE audit_log");
        assert!(rules(&f).contains(&"TRUNCATE"));
    }

    // ── SAFE DDL ─────────────────────────────────────────────────────────────

    #[test]
    fn create_table_is_safe() {
        let f = parse_and_check(indoc! {"
            CREATE TABLE orders (
                id BIGINT NOT NULL AUTO_INCREMENT,
                user_id BIGINT NOT NULL,
                PRIMARY KEY (id)
            )
        "});
        assert!(f.is_empty(), "CREATE TABLE should be clean, got: {f:?}");
    }

    #[test]
    fn multiple_safe_statements_are_clean() {
        let f = parse_and_check(indoc! {"
            ALTER TABLE users ADD COLUMN notes TEXT;
            ALTER TABLE users ADD COLUMN status VARCHAR(50) NOT NULL DEFAULT 'active';
            CREATE INDEX idx_status ON users(status);
        "});
        assert!(f.is_empty(), "all safe statements should produce no findings, got: {f:?}");
    }

    // ── MULTI-OPERATION ALTER ────────────────────────────────────────────────

    #[test]
    fn multi_op_alter_catches_all_issues() {
        let f = parse_and_check(indoc! {"
            ALTER TABLE users
                ADD COLUMN notes TEXT,
                DROP COLUMN legacy,
                ADD COLUMN role VARCHAR(50) NOT NULL
        "});
        let r = rules(&f);
        assert!(r.contains(&"DROP_COLUMN"));
        assert!(r.contains(&"ADD_COLUMN_NOT_NULL_NO_DEFAULT"));
        // notes TEXT is nullable — should not appear
        assert_eq!(r.iter().filter(|&&r| r == "ADD_COLUMN_NOT_NULL_NO_DEFAULT").count(), 1);
    }

    // ── SEVERITY ─────────────────────────────────────────────────────────────

    #[test]
    fn danger_findings_have_danger_severity() {
        let f = parse_and_check("ALTER TABLE users MODIFY COLUMN email TEXT");
        assert!(f.iter().any(|f| f.severity == Severity::Danger));
    }

    #[test]
    fn warning_findings_have_warning_severity() {
        let f = parse_and_check("CREATE UNIQUE INDEX idx ON users(email)");
        assert!(f.iter().any(|f| f.severity == Severity::Warning));
    }

    // ── DETAIL TEXT ──────────────────────────────────────────────────────────

    #[test]
    fn findings_have_non_empty_detail() {
        let f = parse_and_check("DROP TABLE users");
        assert!(!f[0].detail.is_empty());
        assert!(!f[0].title.is_empty());
    }

    // ── CONFIG INTEGRATION ───────────────────────────────────────────────────

    fn config_from_yaml(yaml: &str) -> crate::config::Config {
        let file: crate::config::ConfigFile = serde_yaml::from_str(yaml).expect("yaml");
        crate::config::Config::from_file(file).expect("config")
    }

    #[test]
    fn config_ignore_suppresses_finding() {
        let cfg = config_from_yaml("rules:
  MODIFY_COLUMN: ignore");
        let f = parse_and_check_with("ALTER TABLE users MODIFY COLUMN email TEXT", &cfg);
        assert!(!f.iter().any(|f| f.rule == "MODIFY_COLUMN"),
            "MODIFY_COLUMN should be suppressed");
    }

    #[test]
    fn config_ignore_only_suppresses_named_rule() {
        let cfg = config_from_yaml("rules:
  MODIFY_COLUMN: ignore");
        // A multi-op ALTER — only MODIFY_COLUMN is ignored, DROP_COLUMN still fires
        let f = parse_and_check_with(
            "ALTER TABLE users MODIFY COLUMN email TEXT, DROP COLUMN notes",
            &cfg,
        );
        assert!(!f.iter().any(|f| f.rule == "MODIFY_COLUMN"));
        assert!(f.iter().any(|f| f.rule == "DROP_COLUMN"));
    }

    #[test]
    fn config_downgrade_danger_to_warn() {
        let cfg = config_from_yaml("rules:
  DROP_TABLE: warn");
        let f = parse_and_check_with("DROP TABLE users", &cfg);
        let finding = f.iter().find(|f| f.rule == "DROP_TABLE").expect("no finding");
        assert_eq!(finding.severity, Severity::Warning);
    }

    #[test]
    fn config_upgrade_warn_to_error() {
        let cfg = config_from_yaml("rules:
  ADD_COLUMN_ENUM: error");
        let f = parse_and_check_with(
            "ALTER TABLE t ADD COLUMN k ENUM('a','b') DEFAULT 'a'",
            &cfg,
        );
        let finding = f.iter().find(|f| f.rule == "ADD_COLUMN_ENUM").expect("no finding");
        assert_eq!(finding.severity, Severity::Danger);
    }

    #[test]
    fn default_config_preserves_all_built_in_severities() {
        // Without any overrides every rule should fire at its default severity
        let f = parse_and_check("ALTER TABLE users MODIFY COLUMN email TEXT");
        let finding = f.iter().find(|f| f.rule == "MODIFY_COLUMN").expect("no finding");
        assert_eq!(finding.severity, Severity::Danger);
    }
}
