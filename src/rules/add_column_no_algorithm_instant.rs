use std::path::Path;

use sqlparser::ast::{AlterTableAlgorithm, AlterTableOperation, Statement};

use super::{DialectRule, RuleMeta};
use crate::finding::{Finding, Severity};

pub struct AddColumnNoAlgorithmInstantRule {
    pub severity: Severity,
    pub detail: &'static str,
}

impl DialectRule for AddColumnNoAlgorithmInstantRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            id: "ADD_COLUMN_NO_ALGORITHM_INSTANT",
            default_severity: self.severity.clone(),
        }
    }
    fn check(&self, stmt: &Statement, path: &Path) -> Vec<Finding> {
        let Statement::AlterTable(alter) = stmt else {
            return vec![];
        };

        let has_add_column = alter
            .operations
            .iter()
            .any(|op| matches!(op, AlterTableOperation::AddColumn { .. }));
        if !has_add_column {
            return vec![];
        }

        let has_instant = alter.operations.iter().any(|op| {
            matches!(
                op,
                AlterTableOperation::Algorithm {
                    algorithm: AlterTableAlgorithm::Instant,
                    ..
                }
            )
        });
        if has_instant {
            return vec![];
        }

        let table = alter.name.to_string();
        vec![Finding {
            path: path.to_path_buf(),
            severity: self.severity.clone(),
            rule: "ADD_COLUMN_NO_ALGORITHM_INSTANT",
            title: format!("ADD COLUMN on `{table}` does not specify ALGORITHM=INSTANT"),
            detail: self.detail.to_string(),
            sql: stmt.to_string(),
        }]
    }
}
