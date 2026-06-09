mod add_column_enum;
mod add_column_no_algorithm_instant;
mod alter_foreign_key;
mod multi_statement_migration;
mod add_column_not_null_no_default;
mod add_foreign_key;
mod add_primary_key;
mod add_unique_constraint;
mod change_column;
mod change_column_enum;
mod create_unique_index;
mod drop_column;
mod drop_primary_key;
mod drop_table;
mod lock_tables;
mod modify_column;
mod modify_column_enum;
mod rename_column;
mod rename_table;
mod truncate;

pub use add_column_enum::AddColumnEnumRule;
pub use add_column_no_algorithm_instant::AddColumnNoAlgorithmInstantRule;
pub use alter_foreign_key::AlterForeignKeyRule;
pub use multi_statement_migration::MultiStatementMigrationRule;
pub use add_column_not_null_no_default::AddColumnNotNullNoDefaultRule;
pub use add_foreign_key::AddForeignKeyRule;
pub use add_primary_key::AddPrimaryKeyRule;
pub use add_unique_constraint::AddUniqueConstraintRule;
pub use change_column::ChangeColumnRule;
pub use change_column_enum::ChangeColumnEnumRule;
pub use create_unique_index::CreateUniqueIndexRule;
pub use drop_column::DropColumnRule;
pub use drop_primary_key::DropPrimaryKeyRule;
pub use drop_table::DropTableRule;
pub use lock_tables::LockTablesRule;
pub use modify_column::ModifyColumnRule;
pub use modify_column_enum::ModifyColumnEnumRule;
pub use rename_column::RenameColumnRule;
pub use rename_table::RenameTableRule;
pub use truncate::TruncateRule;

use std::path::Path;

use sqlparser::ast::Statement;

use crate::config::Config;
use crate::finding::{Finding, Severity};

pub struct RuleMeta {
    pub id: &'static str,
    pub default_severity: Severity,
}

pub trait DialectRule {
    fn meta(&self) -> RuleMeta;
    fn check(&self, stmt: &Statement, path: &Path) -> Vec<Finding>;
}

pub trait FileRule {
    fn meta(&self) -> RuleMeta;
    fn check_file(&self, stmts: &[Statement], path: &Path) -> Vec<Finding>;
}

pub fn check_statement(stmt: &Statement, path: &Path, config: &Config) -> Vec<Finding> {
    let raw: Vec<Finding> = config
        .dialect
        .rules()
        .iter()
        .flat_map(|rule| rule.check(stmt, path))
        .collect();

    apply_config(raw, config)
}

pub fn check_file_rules(stmts: &[Statement], path: &Path, config: &Config) -> Vec<Finding> {
    let raw: Vec<Finding> = config
        .dialect
        .file_rules()
        .iter()
        .flat_map(|rule| rule.check_file(stmts, path))
        .collect();

    apply_config(raw, config)
}

fn apply_config(findings: Vec<Finding>, config: &Config) -> Vec<Finding> {
    findings
        .into_iter()
        .filter_map(|mut f| {
            let level = config.effective_level(f.rule, &f.severity);
            match Config::to_severity(&level) {
                None => None,
                Some(sev) => {
                    f.severity = sev;
                    Some(f)
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use sqlparser::parser::Parser;
    use std::path::PathBuf;

    fn parse_and_check(sql: &str) -> Vec<Finding> {
        parse_and_check_with(sql, &crate::config::Config::default())
    }

    fn parse_and_check_with(sql: &str, config: &crate::config::Config) -> Vec<Finding> {
        let dialect = config.dialect.sql_dialect();
        let stmts = Parser::parse_sql(dialect.as_ref(), sql).expect("parse failed");
        let path = PathBuf::from("test.sql");
        let mut findings: Vec<Finding> = stmts
            .iter()
            .flat_map(|s| check_statement(s, &path, config))
            .collect();
        findings.extend(check_file_rules(&stmts, &path, config));
        findings
    }

    fn rules(findings: &[Finding]) -> Vec<&str> {
        findings.iter().map(|f| f.rule).collect()
    }

    // ── ADD COLUMN ───────────────────────────────────────────────────────────

    #[test]
    fn add_column_nullable_is_safe() {
        let f = parse_and_check("ALTER TABLE users ADD COLUMN notes TEXT, ALGORITHM=INSTANT");
        assert!(
            f.is_empty(),
            "nullable add column with ALGORITHM=INSTANT should be clean, got: {f:?}"
        );
    }

    #[test]
    fn add_column_not_null_with_default_is_safe() {
        let f = parse_and_check(
            "ALTER TABLE users ADD COLUMN status VARCHAR(50) NOT NULL DEFAULT 'active', ALGORITHM=INSTANT"
        );
        assert!(
            f.is_empty(),
            "NOT NULL with DEFAULT and ALGORITHM=INSTANT should be safe, got: {f:?}"
        );
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
        assert!(!rules(&f).contains(&"ADD_COLUMN_NOT_NULL_NO_DEFAULT"));
    }

    #[test]
    fn add_column_enum_not_null_no_default_gets_both_rules() {
        let f = parse_and_check("ALTER TABLE users ADD COLUMN kind ENUM('a','b') NOT NULL");
        let r = rules(&f);
        assert!(r.contains(&"ADD_COLUMN_NOT_NULL_NO_DEFAULT"));
        assert!(r.contains(&"ADD_COLUMN_ENUM"));
    }

    // ── ADD COLUMN NO ALGORITHM=INSTANT ──────────────────────────────────────

    #[test]
    fn add_column_without_algorithm_instant_is_warning() {
        let f = parse_and_check("ALTER TABLE users ADD COLUMN notes TEXT");
        assert!(rules(&f).contains(&"ADD_COLUMN_NO_ALGORITHM_INSTANT"));
        let finding = f
            .iter()
            .find(|f| f.rule == "ADD_COLUMN_NO_ALGORITHM_INSTANT")
            .unwrap();
        assert_eq!(finding.severity, crate::finding::Severity::Warning);
    }

    #[test]
    fn add_column_with_algorithm_instant_is_clean() {
        let f = parse_and_check("ALTER TABLE users ADD COLUMN notes TEXT, ALGORITHM=INSTANT");
        assert!(!rules(&f).contains(&"ADD_COLUMN_NO_ALGORITHM_INSTANT"));
    }

    #[test]
    fn add_column_with_algorithm_copy_still_warns() {
        let f = parse_and_check("ALTER TABLE users ADD COLUMN notes TEXT, ALGORITHM=COPY");
        assert!(rules(&f).contains(&"ADD_COLUMN_NO_ALGORITHM_INSTANT"));
    }

    #[test]
    fn add_column_with_algorithm_inplace_still_warns() {
        let f = parse_and_check("ALTER TABLE users ADD COLUMN notes TEXT, ALGORITHM=INPLACE");
        assert!(rules(&f).contains(&"ADD_COLUMN_NO_ALGORITHM_INSTANT"));
    }

    #[test]
    fn non_add_column_alter_does_not_trigger_algorithm_rule() {
        let f = parse_and_check("ALTER TABLE users DROP COLUMN legacy");
        assert!(!rules(&f).contains(&"ADD_COLUMN_NO_ALGORITHM_INSTANT"));
    }

    #[test]
    fn add_column_no_algorithm_instant_fires_once_per_statement() {
        // Two ADD COLUMNs in one statement → one warning, not two
        let f = parse_and_check("ALTER TABLE users ADD COLUMN x TEXT, ADD COLUMN y INT");
        assert_eq!(
            f.iter()
                .filter(|f| f.rule == "ADD_COLUMN_NO_ALGORITHM_INSTANT")
                .count(),
            1
        );
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
        let f =
            parse_and_check("ALTER TABLE users CHANGE COLUMN status account_status VARCHAR(50)");
        assert!(rules(&f).contains(&"CHANGE_COLUMN"));
    }

    #[test]
    fn change_column_enum_gets_both_rules() {
        let f =
            parse_and_check("ALTER TABLE users CHANGE COLUMN status account_status ENUM('a','b')");
        let r = rules(&f);
        assert!(r.contains(&"CHANGE_COLUMN"));
        assert!(r.contains(&"CHANGE_COLUMN_ENUM"));
    }

    // ── RENAME ───────────────────────────────────────────────────────────────

    #[test]
    fn rename_column_is_danger() {
        let f = parse_and_check("ALTER TABLE users RENAME COLUMN old_name TO new_name");
        assert!(rules(&f).contains(&"RENAME_COLUMN"));
    }

    #[test]
    fn rename_table_via_alter_is_danger() {
        let f = parse_and_check("ALTER TABLE users RENAME TO accounts");
        assert!(rules(&f).contains(&"RENAME_TABLE"));
    }

    #[test]
    fn rename_table_statement_is_danger() {
        let f = parse_and_check("RENAME TABLE users TO accounts");
        assert!(rules(&f).contains(&"RENAME_TABLE"));
    }

    #[test]
    fn rename_table_statement_multi_fires_per_pair() {
        let f = parse_and_check("RENAME TABLE a TO b, c TO d");
        assert_eq!(f.iter().filter(|f| f.rule == "RENAME_TABLE").count(), 2);
    }

    // ── DROP ─────────────────────────────────────────────────────────────────

    #[test]
    fn drop_column_is_danger() {
        let f = parse_and_check("ALTER TABLE users DROP COLUMN legacy_field");
        assert!(rules(&f).contains(&"DROP_COLUMN"));
    }

    #[test]
    fn drop_column_fires_per_column() {
        let f = parse_and_check("ALTER TABLE users DROP COLUMN a, DROP COLUMN b");
        assert_eq!(
            f.iter().filter(|f| f.rule == "DROP_COLUMN").count(),
            2,
            "should fire once per dropped column"
        );
    }

    #[test]
    fn add_primary_key_is_danger() {
        let f = parse_and_check("ALTER TABLE users ADD PRIMARY KEY (id)");
        assert!(rules(&f).contains(&"ADD_PRIMARY_KEY"));
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
        let f =
            parse_and_check("ALTER TABLE orders ADD FOREIGN KEY (user_id) REFERENCES users(id)");
        assert!(rules(&f).contains(&"ADD_FOREIGN_KEY"));
    }

    #[test]
    fn add_foreign_key_detail_names_table() {
        let f =
            parse_and_check("ALTER TABLE orders ADD FOREIGN KEY (user_id) REFERENCES users(id)");
        let finding = f.iter().find(|f| f.rule == "ADD_FOREIGN_KEY").unwrap();
        assert!(
            finding.detail.contains("orders"),
            "detail should reference table name"
        );
    }

    #[test]
    fn add_unique_constraint_is_warning() {
        let f = parse_and_check("ALTER TABLE users ADD UNIQUE KEY idx_email (email)");
        assert!(rules(&f).contains(&"ADD_UNIQUE_CONSTRAINT"));
        assert_eq!(f[0].severity, crate::finding::Severity::Warning);
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
        assert_eq!(f[0].severity, crate::finding::Severity::Warning);
    }

    // ── TRUNCATE ─────────────────────────────────────────────────────────────

    #[test]
    fn truncate_is_danger() {
        let f = parse_and_check("TRUNCATE TABLE audit_log");
        assert!(rules(&f).contains(&"TRUNCATE"));
    }

    #[test]
    fn lock_tables_is_danger() {
        let f = parse_and_check("LOCK TABLES users WRITE");
        assert!(rules(&f).contains(&"LOCK_TABLES"));
    }

    #[test]
    fn lock_tables_read_is_danger() {
        let f = parse_and_check("LOCK TABLES users READ");
        assert!(rules(&f).contains(&"LOCK_TABLES"));
    }

    #[test]
    fn lock_tables_multiple_tables_fires_once() {
        let f = parse_and_check("LOCK TABLES users WRITE, orders READ");
        assert_eq!(
            f.iter().filter(|f| f.rule == "LOCK_TABLES").count(),
            1,
            "LOCK TABLES with multiple tables should fire exactly once"
        );
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
        // Multiple DDL statements now trigger MULTI_STATEMENT_MIGRATION (warning).
        // Verify there are no *danger* findings — the only finding is the migration-level advisory.
        let f = parse_and_check(indoc! {"
            ALTER TABLE users ADD COLUMN notes TEXT, ALGORITHM=INSTANT;
            ALTER TABLE users ADD COLUMN status VARCHAR(50) NOT NULL DEFAULT 'active', ALGORITHM=INSTANT;
            CREATE INDEX idx_status ON users(status);
        "});
        let danger: Vec<_> = f
            .iter()
            .filter(|f| f.severity == crate::finding::Severity::Danger)
            .collect();
        assert!(danger.is_empty(), "no danger findings expected, got: {danger:?}");
        assert!(
            rules(&f).contains(&"MULTI_STATEMENT_MIGRATION"),
            "expected MULTI_STATEMENT_MIGRATION warning for multi-DDL file"
        );
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
        assert_eq!(
            r.iter()
                .filter(|&&r| r == "ADD_COLUMN_NOT_NULL_NO_DEFAULT")
                .count(),
            1
        );
    }

    // ── SEVERITY ─────────────────────────────────────────────────────────────

    #[test]
    fn danger_findings_have_danger_severity() {
        let f = parse_and_check("ALTER TABLE users MODIFY COLUMN email TEXT");
        assert!(f
            .iter()
            .any(|f| f.severity == crate::finding::Severity::Danger));
    }

    #[test]
    fn warning_findings_have_warning_severity() {
        let f = parse_and_check("CREATE UNIQUE INDEX idx ON users(email)");
        assert!(f
            .iter()
            .any(|f| f.severity == crate::finding::Severity::Warning));
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
        let cfg = config_from_yaml(
            "rules:
  MODIFY_COLUMN: ignore",
        );
        let f = parse_and_check_with("ALTER TABLE users MODIFY COLUMN email TEXT", &cfg);
        assert!(
            !f.iter().any(|f| f.rule == "MODIFY_COLUMN"),
            "MODIFY_COLUMN should be suppressed"
        );
    }

    #[test]
    fn config_ignore_only_suppresses_named_rule() {
        let cfg = config_from_yaml(
            "rules:
  MODIFY_COLUMN: ignore",
        );
        let f = parse_and_check_with(
            "ALTER TABLE users MODIFY COLUMN email TEXT, DROP COLUMN notes",
            &cfg,
        );
        assert!(!f.iter().any(|f| f.rule == "MODIFY_COLUMN"));
        assert!(f.iter().any(|f| f.rule == "DROP_COLUMN"));
    }

    #[test]
    fn config_downgrade_danger_to_warn() {
        let cfg = config_from_yaml(
            "rules:
  DROP_TABLE: warn",
        );
        let f = parse_and_check_with("DROP TABLE users", &cfg);
        let finding = f
            .iter()
            .find(|f| f.rule == "DROP_TABLE")
            .expect("no finding");
        assert_eq!(finding.severity, crate::finding::Severity::Warning);
    }

    #[test]
    fn config_upgrade_warn_to_error() {
        let cfg = config_from_yaml(
            "rules:
  ADD_COLUMN_ENUM: error",
        );
        let f = parse_and_check_with("ALTER TABLE t ADD COLUMN k ENUM('a','b') DEFAULT 'a'", &cfg);
        let finding = f
            .iter()
            .find(|f| f.rule == "ADD_COLUMN_ENUM")
            .expect("no finding");
        assert_eq!(finding.severity, crate::finding::Severity::Danger);
    }

    #[test]
    fn default_config_preserves_all_built_in_severities() {
        let f = parse_and_check("ALTER TABLE users MODIFY COLUMN email TEXT");
        let finding = f
            .iter()
            .find(|f| f.rule == "MODIFY_COLUMN")
            .expect("no finding");
        assert_eq!(finding.severity, crate::finding::Severity::Danger);
    }

    // ── ALTER FOREIGN KEY ────────────────────────────────────────────────────

    #[test]
    fn alter_fk_drop_and_add_without_fk_checks_is_danger() {
        let f = parse_and_check(indoc! {"
            ALTER TABLE orders DROP FOREIGN KEY fk_user;
            ALTER TABLE orders ADD CONSTRAINT fk_user FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE RESTRICT;
        "});
        assert!(
            rules(&f).contains(&"ALTER_FOREIGN_KEY"),
            "expected ALTER_FOREIGN_KEY, got: {f:?}"
        );
    }

    #[test]
    fn alter_fk_with_fk_checks_disabled_is_safe() {
        let f = parse_and_check(indoc! {"
            SET FOREIGN_KEY_CHECKS = 0;
            ALTER TABLE orders DROP FOREIGN KEY fk_user;
            ALTER TABLE orders ADD CONSTRAINT fk_user FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE RESTRICT;
            SET FOREIGN_KEY_CHECKS = 1;
        "});
        assert!(
            !rules(&f).contains(&"ALTER_FOREIGN_KEY"),
            "should be clean with FK checks disabled, got: {f:?}"
        );
    }

    #[test]
    fn alter_fk_missing_restore_is_danger() {
        let f = parse_and_check(indoc! {"
            SET FOREIGN_KEY_CHECKS = 0;
            ALTER TABLE orders DROP FOREIGN KEY fk_user;
            ALTER TABLE orders ADD CONSTRAINT fk_user FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE RESTRICT;
        "});
        assert!(
            rules(&f).contains(&"ALTER_FOREIGN_KEY"),
            "missing SET FK_CHECKS=1 should still fire, got: {f:?}"
        );
    }

    #[test]
    fn alter_fk_missing_disable_is_danger() {
        let f = parse_and_check(indoc! {"
            ALTER TABLE orders DROP FOREIGN KEY fk_user;
            ALTER TABLE orders ADD CONSTRAINT fk_user FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE RESTRICT;
            SET FOREIGN_KEY_CHECKS = 1;
        "});
        assert!(
            rules(&f).contains(&"ALTER_FOREIGN_KEY"),
            "missing SET FK_CHECKS=0 should still fire, got: {f:?}"
        );
    }

    // ── MULTI_STATEMENT_MIGRATION ────────────────────────────────────────────

    #[test]
    fn single_ddl_statement_is_clean() {
        let f = parse_and_check("ALTER TABLE users ADD COLUMN notes TEXT, ALGORITHM=INSTANT;");
        assert!(!rules(&f).contains(&"MULTI_STATEMENT_MIGRATION"));
    }

    #[test]
    fn two_ddl_statements_triggers_rule() {
        let f = parse_and_check(indoc! {"
            ALTER TABLE users ADD COLUMN notes TEXT, ALGORITHM=INSTANT;
            ALTER TABLE orders ADD COLUMN ref TEXT, ALGORITHM=INSTANT;
        "});
        assert!(
            rules(&f).contains(&"MULTI_STATEMENT_MIGRATION"),
            "two DDL stmts should fire MULTI_STATEMENT_MIGRATION, got: {f:?}"
        );
    }

    #[test]
    fn multi_statement_finding_is_warning() {
        let f = parse_and_check(indoc! {"
            ALTER TABLE users ADD COLUMN notes TEXT, ALGORITHM=INSTANT;
            ALTER TABLE orders ADD COLUMN ref TEXT, ALGORITHM=INSTANT;
        "});
        let finding = f
            .iter()
            .find(|f| f.rule == "MULTI_STATEMENT_MIGRATION")
            .unwrap();
        assert_eq!(finding.severity, crate::finding::Severity::Warning);
    }

    #[test]
    fn multi_statement_fires_once_not_per_statement() {
        let f = parse_and_check(indoc! {"
            ALTER TABLE a ADD COLUMN x TEXT, ALGORITHM=INSTANT;
            ALTER TABLE b ADD COLUMN y TEXT, ALGORITHM=INSTANT;
            ALTER TABLE c ADD COLUMN z TEXT, ALGORITHM=INSTANT;
        "});
        assert_eq!(
            f.iter()
                .filter(|f| f.rule == "MULTI_STATEMENT_MIGRATION")
                .count(),
            1,
            "should fire exactly once regardless of statement count"
        );
    }

    #[test]
    fn set_statement_not_counted_as_ddl() {
        // SET is not DDL — one ALTER TABLE flanked by SET statements should not trigger
        // MULTI_STATEMENT_MIGRATION (only 1 DDL statement).
        let f = parse_and_check(indoc! {"
            SET FOREIGN_KEY_CHECKS = 0;
            ALTER TABLE orders ADD CONSTRAINT fk_user FOREIGN KEY (user_id) REFERENCES users(id);
            SET FOREIGN_KEY_CHECKS = 1;
        "});
        assert!(
            !rules(&f).contains(&"MULTI_STATEMENT_MIGRATION"),
            "SET statements should not count toward DDL statement count"
        );
    }

    #[test]
    fn fresh_add_fk_without_drop_does_not_trigger_alter_rule() {
        // A brand-new FK (no matching DROP in the file) is covered by ADD_FOREIGN_KEY,
        // not ALTER_FOREIGN_KEY.
        let f = parse_and_check(
            "ALTER TABLE orders ADD CONSTRAINT fk_user FOREIGN KEY (user_id) REFERENCES users(id);",
        );
        assert!(
            !rules(&f).contains(&"ALTER_FOREIGN_KEY"),
            "fresh ADD FK should not trigger ALTER_FOREIGN_KEY"
        );
        assert!(
            rules(&f).contains(&"ADD_FOREIGN_KEY"),
            "fresh ADD FK should still trigger ADD_FOREIGN_KEY"
        );
    }
}
