use std::path::Path;

use sqlparser::ast::{AlterTableOperation, Statement};

use super::{DialectRule, RuleMeta};
use crate::finding::{Finding, Severity};

pub struct ChangeColumnRule {
    pub severity: Severity,
    pub detail: fn(&str, &str) -> String,
}

impl DialectRule for ChangeColumnRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            id: "CHANGE_COLUMN",
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
                let AlterTableOperation::ChangeColumn {
                    old_name, new_name, ..
                } = op
                else {
                    return None;
                };
                Some(Finding {
                    path: path.to_path_buf(),
                    severity: self.severity.clone(),
                    rule: "CHANGE_COLUMN",
                    title: format!(
                        "CHANGE COLUMN `{old_name}` → `{new_name}` on `{table}` renames a column"
                    ),
                    detail: (self.detail)(&old_name.to_string(), &new_name.to_string()),
                    sql: stmt.to_string(),
                })
            })
            .collect()
    }
}
