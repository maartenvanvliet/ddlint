use std::path::Path;

use sqlparser::ast::Statement;

use super::{FileRule, RuleMeta};
use crate::finding::{Finding, Severity};

pub struct MultiStatementMigrationRule {
    pub severity: Severity,
    pub detail: &'static str,
}

impl FileRule for MultiStatementMigrationRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            id: "MULTI_STATEMENT_MIGRATION",
            default_severity: self.severity.clone(),
        }
    }

    fn check_file(&self, stmts: &[Statement], path: &Path) -> Vec<Finding> {
        let ddl: Vec<&Statement> = stmts.iter().filter(|s| is_ddl(s)).collect();

        if ddl.len() < 2 {
            return vec![];
        }

        vec![Finding {
            path: path.to_path_buf(),
            severity: self.severity.clone(),
            rule: "MULTI_STATEMENT_MIGRATION",
            title: format!(
                "Migration contains {} DDL statements — MySQL has no transactional DDL",
                ddl.len()
            ),
            detail: self.detail.to_string(),
            sql: ddl[0].to_string(),
        }]
    }
}

fn is_ddl(stmt: &Statement) -> bool {
    matches!(
        stmt,
        Statement::AlterTable(_)
            | Statement::CreateTable(_)
            | Statement::Drop { .. }
            | Statement::CreateIndex(_)
            | Statement::Truncate { .. }
            | Statement::RenameTable(_)
    )
}
