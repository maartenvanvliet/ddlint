use std::path::Path;

use sqlparser::ast::{AlterTableOperation, Statement};

use super::{DialectRule, RuleMeta};
use crate::finding::{Finding, Severity};

pub struct AddPrimaryKeyRule {
    pub severity: Severity,
    pub detail: &'static str,
}

impl DialectRule for AddPrimaryKeyRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            id: "ADD_PRIMARY_KEY",
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
                let sqlparser::ast::TableConstraint::PrimaryKey(_) = constraint else {
                    return None;
                };
                Some(Finding {
                    path: path.to_path_buf(),
                    severity: self.severity.clone(),
                    rule: "ADD_PRIMARY_KEY",
                    title: format!("ADD PRIMARY KEY on `{table}` requires a full table rebuild"),
                    detail: self.detail.to_string(),
                    sql: stmt.to_string(),
                })
            })
            .collect()
    }
}
