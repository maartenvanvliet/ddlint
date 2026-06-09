use std::path::Path;

use sqlparser::ast::{AlterTableOperation, Statement};

use super::{DialectRule, RuleMeta};
use crate::finding::{Finding, Severity};

pub struct DropColumnRule {
    pub severity: Severity,
    pub detail: fn(&str) -> String,
}

impl DialectRule for DropColumnRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            id: "DROP_COLUMN",
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
            .flat_map(|op| {
                let AlterTableOperation::DropColumn { column_names, .. } = op else {
                    return vec![];
                };
                column_names
                    .iter()
                    .map(|col| Finding {
                        path: path.to_path_buf(),
                        severity: self.severity.clone(),
                        rule: "DROP_COLUMN",
                        title: format!("DROP COLUMN `{col}` on `{table}` is irreversible"),
                        detail: (self.detail)(&col.to_string()),
                        sql: stmt.to_string(),
                    })
                    .collect()
            })
            .collect()
    }
}
