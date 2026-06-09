use std::path::Path;

use sqlparser::ast::{AlterTableOperation, ColumnOption, Statement};

use super::{DialectRule, RuleMeta};
use crate::finding::{Finding, Severity};

pub struct AddColumnNotNullNoDefaultRule {
    pub severity: Severity,
    pub detail: &'static str,
}

impl DialectRule for AddColumnNotNullNoDefaultRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            id: "ADD_COLUMN_NOT_NULL_NO_DEFAULT",
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
                let not_null = column_def
                    .options
                    .iter()
                    .any(|o| matches!(o.option, ColumnOption::NotNull));
                let has_default = column_def
                    .options
                    .iter()
                    .any(|o| matches!(o.option, ColumnOption::Default(_)));
                if not_null && !has_default {
                    Some(Finding {
                        path: path.to_path_buf(),
                        severity: self.severity.clone(),
                        rule: "ADD_COLUMN_NOT_NULL_NO_DEFAULT",
                        title: format!(
                            "ADD COLUMN `{col}` on `{table}` is NOT NULL with no DEFAULT"
                        ),
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
