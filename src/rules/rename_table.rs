use std::path::Path;

use sqlparser::ast::{AlterTableOperation, Statement};

use super::{DialectRule, RuleMeta};
use crate::finding::{Finding, Severity};

pub struct RenameTableRule {
    pub severity: Severity,
    pub detail: fn(&str, &str) -> String,
}

impl DialectRule for RenameTableRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            id: "RENAME_TABLE",
            default_severity: self.severity.clone(),
        }
    }
    fn check(&self, stmt: &Statement, path: &Path) -> Vec<Finding> {
        match stmt {
            // ALTER TABLE old RENAME TO new
            Statement::AlterTable(alter) => {
                let table = alter.name.to_string();
                alter
                    .operations
                    .iter()
                    .filter_map(|op| {
                        let AlterTableOperation::RenameTable { table_name } = op else {
                            return None;
                        };
                        Some(Finding {
                            path: path.to_path_buf(),
                            severity: self.severity.clone(),
                            rule: "RENAME_TABLE",
                            title: format!("RENAME TABLE `{table}` → `{table_name}`"),
                            detail: (self.detail)(&table, &table_name.to_string()),
                            sql: stmt.to_string(),
                        })
                    })
                    .collect()
            }
            // RENAME TABLE old TO new [, old2 TO new2, ...]
            Statement::RenameTable(renames) => renames
                .iter()
                .map(|r| {
                    let old = r.old_name.to_string();
                    let new = r.new_name.to_string();
                    Finding {
                        path: path.to_path_buf(),
                        severity: self.severity.clone(),
                        rule: "RENAME_TABLE",
                        title: format!("RENAME TABLE `{old}` → `{new}`"),
                        detail: (self.detail)(&old, &new),
                        sql: stmt.to_string(),
                    }
                })
                .collect(),
            _ => vec![],
        }
    }
}
