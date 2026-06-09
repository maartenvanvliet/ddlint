use std::path::Path;

use sqlparser::ast::{AlterTableOperation, Statement};

use super::{DialectRule, RuleMeta};
use crate::finding::{Finding, Severity};

pub struct DropPrimaryKeyRule {
    pub severity: Severity,
    pub detail: &'static str,
}

impl DialectRule for DropPrimaryKeyRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            id: "DROP_PRIMARY_KEY",
            default_severity: self.severity.clone(),
        }
    }
    fn check(&self, stmt: &Statement, path: &Path) -> Vec<Finding> {
        let Statement::AlterTable(alter) = stmt else {
            return vec![];
        };
        let table = alter.name.to_string();
        alter
            .operations
            .iter()
            .filter_map(|op| {
                let AlterTableOperation::DropPrimaryKey { .. } = op else {
                    return None;
                };
                Some(Finding {
                    path: path.to_path_buf(),
                    severity: self.severity.clone(),
                    rule: "DROP_PRIMARY_KEY",
                    title: format!("DROP PRIMARY KEY on `{table}` requires a full table rebuild"),
                    detail: self.detail.to_string(),
                    sql: stmt.to_string(),
                })
            })
            .collect()
    }
}
