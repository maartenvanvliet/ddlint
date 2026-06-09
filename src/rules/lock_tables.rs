use std::path::Path;

use sqlparser::ast::Statement;

use super::{DialectRule, RuleMeta};
use crate::finding::{Finding, Severity};

pub struct LockTablesRule {
    pub severity: Severity,
    pub detail: &'static str,
}

impl DialectRule for LockTablesRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            id: "LOCK_TABLES",
            default_severity: self.severity.clone(),
        }
    }
    fn check(&self, stmt: &Statement, path: &Path) -> Vec<Finding> {
        let Statement::LockTables { tables } = stmt else {
            return vec![];
        };
        let names: Vec<String> = tables.iter().map(|t| t.table.value.clone()).collect();
        let label = names.join(", ");
        vec![Finding {
            path: path.to_path_buf(),
            severity: self.severity.clone(),
            rule: "LOCK_TABLES",
            title: format!("LOCK TABLES `{label}` serialises all access"),
            detail: self.detail.to_string(),
            sql: stmt.to_string(),
        }]
    }
}
