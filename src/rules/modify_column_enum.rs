use std::path::Path;

use sqlparser::ast::{AlterTableOperation, Statement};

use super::{DialectRule, RuleMeta};
use crate::finding::{Finding, Severity};

pub struct ModifyColumnEnumRule {
    pub severity: Severity,
    pub detail: &'static str,
}

impl DialectRule for ModifyColumnEnumRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            id: "MODIFY_COLUMN_ENUM",
            default_severity: self.severity.clone(),
        }
    }
    fn check(&self, stmt: &Statement, path: &Path) -> Vec<Finding> {
        let Statement::AlterTable(alter) = stmt else {
            return vec![];
        };
        let table = alter.name.to_string();
        alter.operations.iter().filter_map(|op| {
            let AlterTableOperation::ModifyColumn { col_name, data_type, .. } = op else { return None };
            let is_enum = format!("{data_type}").to_uppercase().starts_with("ENUM");
            if is_enum {
                Some(Finding {
                    path: path.to_path_buf(),
                    severity: self.severity.clone(),
                    rule: "MODIFY_COLUMN_ENUM",
                    title: format!("MODIFY COLUMN `{col_name}` on `{table}` is ENUM — always ALGORITHM=COPY"),
                    detail: self.detail.to_string(),
                    sql: stmt.to_string(),
                })
            } else {
                None
            }
        }).collect()
    }
}
