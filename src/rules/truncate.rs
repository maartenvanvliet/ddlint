use std::path::Path;

use sqlparser::ast::Statement;

use super::{DialectRule, RuleMeta};
use crate::finding::{Finding, Severity};

pub struct TruncateRule {
    pub severity: Severity,
    pub detail: fn(&str) -> String,
}

impl DialectRule for TruncateRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            id: "TRUNCATE",
            default_severity: self.severity.clone(),
        }
    }
    fn check(&self, stmt: &Statement, path: &Path) -> Vec<Finding> {
        let Statement::Truncate(truncate) = stmt else {
            return vec![];
        };
        truncate
            .table_names
            .iter()
            .map(|tbl| {
                let name = tbl.name.to_string();
                Finding {
                    path: path.to_path_buf(),
                    severity: self.severity.clone(),
                    rule: "TRUNCATE",
                    title: format!("TRUNCATE `{name}` destroys all rows"),
                    detail: (self.detail)(&name),
                    sql: stmt.to_string(),
                }
            })
            .collect()
    }
}
