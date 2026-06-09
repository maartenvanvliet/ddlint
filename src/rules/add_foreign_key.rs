use std::path::Path;

use sqlparser::ast::{AlterTableOperation, Statement};

use super::{DialectRule, RuleMeta};
use crate::finding::{Finding, Severity};

pub struct AddForeignKeyRule {
    pub severity: Severity,
    pub detail: fn(&str) -> String,
}

impl DialectRule for AddForeignKeyRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            id: "ADD_FOREIGN_KEY",
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
                let AlterTableOperation::AddConstraint { constraint, .. } = op else {
                    return None;
                };
                let sqlparser::ast::TableConstraint::ForeignKey(c) = constraint else {
                    return None;
                };
                let fk = c
                    .name
                    .as_ref()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "<unnamed>".into());
                Some(Finding {
                    path: path.to_path_buf(),
                    severity: self.severity.clone(),
                    rule: "ADD_FOREIGN_KEY",
                    title: format!("ADD FOREIGN KEY `{fk}` on `{table}` acquires a metadata lock"),
                    detail: (self.detail)(&table),
                    sql: stmt.to_string(),
                })
            })
            .collect()
    }
}
