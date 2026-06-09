use std::fmt;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Severity
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Danger,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Warning => write!(f, "warning"),
            Severity::Danger => write!(f, "error"),
        }
    }
}

// ---------------------------------------------------------------------------
// Finding
// ---------------------------------------------------------------------------

/// A single lint finding attached to a file.
#[derive(Debug, Clone)]
pub struct Finding {
    /// Which file triggered this finding.
    #[allow(dead_code)]
    pub path: PathBuf,
    /// Danger vs Warning.
    pub severity: Severity,
    /// Short machine-readable rule identifier, e.g. `MODIFY_COLUMN`.
    pub rule: &'static str,
    /// One-line title shown in annotations and compact output.
    pub title: String,
    /// Full explanation: what the problem is, why it matters, what to do instead.
    pub detail: String,
    /// The SQL statement as printed by sqlparser (single-line).
    pub sql: String,
}

// ---------------------------------------------------------------------------
// FileResult
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct FileResult {
    pub path: PathBuf,
    pub findings: Vec<Finding>,
    /// Present when the file could not be read or parsed at all.
    pub parse_error: Option<String>,
}

impl FileResult {
    pub fn ok(path: PathBuf, findings: Vec<Finding>) -> Self {
        Self {
            path,
            findings,
            parse_error: None,
        }
    }

    pub fn error(path: PathBuf, msg: String) -> Self {
        Self {
            path,
            findings: vec![],
            parse_error: Some(msg),
        }
    }

    pub fn has_issues(&self) -> bool {
        self.parse_error.is_some() || !self.findings.is_empty()
    }
}
