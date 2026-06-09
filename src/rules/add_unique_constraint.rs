use std::path::Path;

use sqlparser::ast::{AlterTableOperation, Statement};

use super::{DialectRule, RuleMeta};
use crate::finding::{Finding, Severity};

pub struct AddUniqueConstraintRule {
    pub severity: Severity,
    pub detail: &'static str,
}

impl DialectRule for AddUniqueConstraintRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            id: "ADD_UNIQUE_CONSTRAINT",
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
                let sqlparser::ast::TableConstraint::Unique(c) = constraint else {
                    return None;
                };
                let idx = c
                    .name
                    .as_ref()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "<unnamed>".into());
                Some(Finding {
                    path: path.to_path_buf(),
                    severity: self.severity.clone(),
                    rule: "ADD_UNIQUE_CONSTRAINT",
                    title: format!(
                        "ADD UNIQUE KEY `{idx}` on `{table}` requires a full duplicate scan"
                    ),
                    detail: self.detail.to_string(),
                    sql: stmt.to_string(),
                })
            })
            .collect()
    }
}
