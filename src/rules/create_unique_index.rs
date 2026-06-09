use std::path::Path;

use sqlparser::ast::Statement;

use super::{DialectRule, RuleMeta};
use crate::finding::{Finding, Severity};

pub struct CreateUniqueIndexRule {
    pub severity: Severity,
    pub detail: &'static str,
}

impl DialectRule for CreateUniqueIndexRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            id: "CREATE_UNIQUE_INDEX",
            default_severity: self.severity.clone(),
        }
    }
    fn check(&self, stmt: &Statement, path: &Path) -> Vec<Finding> {
        let Statement::CreateIndex(ci) = stmt else {
            return vec![];
        };
        if !ci.unique {
            return vec![];
        }
        let table = ci.table_name.to_string();
        let idx = ci
            .name
            .as_ref()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "<unnamed>".into());
        vec![Finding {
            path: path.to_path_buf(),
            severity: self.severity.clone(),
            rule: "CREATE_UNIQUE_INDEX",
            title: format!("CREATE UNIQUE INDEX `{idx}` on `{table}` requires a duplicate scan"),
            detail: self.detail.to_string(),
            sql: stmt.to_string(),
        }]
    }
}
