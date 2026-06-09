use std::path::Path;

use sqlparser::ast::{AlterTableOperation, Statement};

use super::{DialectRule, RuleMeta};
use crate::finding::{Finding, Severity};

pub struct ModifyColumnRule {
    pub severity: Severity,
    pub detail: &'static str,
}

impl DialectRule for ModifyColumnRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            id: "MODIFY_COLUMN",
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
                let AlterTableOperation::ModifyColumn { col_name, .. } = op else {
                    return None;
                };
                Some(Finding {
                    path: path.to_path_buf(),
                    severity: self.severity.clone(),
                    rule: "MODIFY_COLUMN",
                    title: format!("MODIFY COLUMN `{col_name}` on `{table}` may rebuild the table"),
                    detail: self.detail.to_string(),
                    sql: stmt.to_string(),
                })
            })
            .collect()
    }
}
