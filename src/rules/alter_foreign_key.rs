use std::path::Path;

use sqlparser::ast::{AlterTableOperation, Statement, TableConstraint};

use super::{FileRule, RuleMeta};
use crate::finding::{Finding, Severity};

pub struct AlterForeignKeyRule {
    pub severity: Severity,
    pub detail: &'static str,
}

impl FileRule for AlterForeignKeyRule {
    fn meta(&self) -> RuleMeta {
        RuleMeta {
            id: "ALTER_FOREIGN_KEY",
            default_severity: self.severity.clone(),
        }
    }

    fn check_file(&self, stmts: &[Statement], path: &Path) -> Vec<Finding> {
        // Collect positions of every ADD FOREIGN KEY in the file.
        let add_fk: Vec<(usize, &Statement)> = stmts
            .iter()
            .enumerate()
            .filter(|(_, s)| stmt_adds_foreign_key(s))
            .collect();

        if add_fk.is_empty() {
            return vec![];
        }

        // Only flag when this is an *alteration* (DROP FK present elsewhere in the file).
        // A fresh ADD FK with no matching DROP is already covered by ADD_FOREIGN_KEY.
        if !stmts.iter().any(stmt_drops_foreign_key) {
            return vec![];
        }

        let first_add_idx = add_fk[0].0;
        let last_add_idx = add_fk[add_fk.len() - 1].0;

        let checks_off_before = stmts[..first_add_idx].iter().any(is_fk_checks_off);
        let checks_on_after = stmts[last_add_idx + 1..].iter().any(is_fk_checks_on);

        if checks_off_before && checks_on_after {
            return vec![];
        }

        let (_, add_stmt) = add_fk[0];
        let table = if let Statement::AlterTable(a) = add_stmt {
            a.name.to_string()
        } else {
            String::new()
        };

        vec![Finding {
            path: path.to_path_buf(),
            severity: self.severity.clone(),
            rule: "ALTER_FOREIGN_KEY",
            title: format!(
                "FK constraint on `{table}` altered without SET FOREIGN_KEY_CHECKS=0/1"
            ),
            detail: self.detail.to_string(),
            sql: add_stmt.to_string(),
        }]
    }
}

fn stmt_adds_foreign_key(stmt: &Statement) -> bool {
    let Statement::AlterTable(alter) = stmt else {
        return false;
    };
    alter.operations.iter().any(|op| {
        let AlterTableOperation::AddConstraint { constraint, .. } = op else {
            return false;
        };
        matches!(constraint, TableConstraint::ForeignKey(_))
    })
}

fn stmt_drops_foreign_key(stmt: &Statement) -> bool {
    // "DROP FOREIGN KEY name" is MySQL syntax; normalise to lowercase for matching.
    stmt.to_string().to_lowercase().contains("drop foreign key")
}

fn is_fk_checks_off(stmt: &Statement) -> bool {
    let s = stmt.to_string().replace(' ', "").to_lowercase();
    s.contains("foreign_key_checks") && (s.contains("=0") || s.contains("=false"))
}

fn is_fk_checks_on(stmt: &Statement) -> bool {
    let s = stmt.to_string().replace(' ', "").to_lowercase();
    s.contains("foreign_key_checks") && (s.contains("=1") || s.contains("=true"))
}
