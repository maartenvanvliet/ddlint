use std::path::Path;

use sqlparser::ast::{ObjectType, Statement};

use super::{DialectRule, RuleMeta};
use crate::finding::{Finding, Severity};

pub struct DropTableRule {
    pub severity: Severity,
    pub detail: fn(&str) -> String,
}

impl DialectRule for DropTableRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            id: "DROP_TABLE",
            default_severity: self.severity.clone(),
        }
    }
    fn check(&self, stmt: &Statement, path: &Path) -> Vec<Finding> {
        let Statement::Drop {
            object_type: ObjectType::Table,
            names,
            ..
        } = stmt
        else {
            return vec![];
        };
        names
            .iter()
            .map(|name| Finding {
                path: path.to_path_buf(),
                severity: self.severity.clone(),
                rule: "DROP_TABLE",
                title: format!("DROP TABLE `{name}` is irreversible"),
                detail: (self.detail)(&name.to_string()),
                sql: stmt.to_string(),
            })
            .collect()
    }
}
