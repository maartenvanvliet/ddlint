use std::path::Path;

use sqlparser::ast::{AlterTableOperation, Statement};

use super::{DialectRule, RuleMeta};
use crate::finding::{Finding, Severity};

pub struct RenameColumnRule {
    pub severity: Severity,
    pub detail: fn(&str, &str) -> String,
}

impl DialectRule for RenameColumnRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            id: "RENAME_COLUMN",
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
                let AlterTableOperation::RenameColumn {
                    old_column_name,
                    new_column_name,
                } = op
                else {
                    return None;
                };
                Some(Finding {
                    path: path.to_path_buf(),
                    severity: self.severity.clone(),
                    rule: "RENAME_COLUMN",
                    title: format!(
                        "RENAME COLUMN `{old_column_name}` → `{new_column_name}` on `{table}`"
                    ),
                    detail: (self.detail)(
                        &old_column_name.to_string(),
                        &new_column_name.to_string(),
                    ),
                    sql: stmt.to_string(),
                })
            })
            .collect()
    }
}
