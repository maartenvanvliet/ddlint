use std::path::Path;

use sqlparser::ast::{AlterTableOperation, Statement};

use super::{DialectRule, RuleMeta};
use crate::finding::{Finding, Severity};

pub struct AddColumnEnumRule {
    pub severity: Severity,
    pub detail: &'static str,
}

impl DialectRule for AddColumnEnumRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            id: "ADD_COLUMN_ENUM",
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
                let AlterTableOperation::AddColumn { column_def, .. } = op else {
                    return None;
                };
                let col = column_def.name.to_string();
                let is_enum = format!("{}", column_def.data_type)
                    .to_uppercase()
                    .starts_with("ENUM");
                if is_enum {
                    Some(Finding {
                        path: path.to_path_buf(),
                        severity: self.severity.clone(),
                        rule: "ADD_COLUMN_ENUM",
                        title: format!("ADD COLUMN `{col}` on `{table}` uses ENUM"),
                        detail: self.detail.to_string(),
                        sql: stmt.to_string(),
                    })
                } else {
                    None
                }
            })
            .collect()
    }
}
