//! File discovery and per-file analysis.
//!
//! Inputs are resolved in this order:
//!   - A glob pattern (`*`, `?`, `[`) → expanded by the `glob` crate
//!   - A directory → recursively collect all `.sql` files under it
//!   - A plain file path → taken directly
//!
//! This means all three invocation styles work:
//!
//!   ddlint migrations/                          # directory
//!   ddlint migrations/V1__init.sql             # single file
//!   ddlint migrations/V1*.sql migrations/V2*.sql  # multiple patterns
//!   ddlint 'migrations/**/*.sql'               # glob (quoted to avoid shell expansion)

use std::fs;
use std::path::{Path, PathBuf};

use glob::glob;
use sqlparser::parser::Parser;
use walkdir::WalkDir;

use crate::config::Config;
use crate::finding::FileResult;
use crate::rules::{check_file_rules, check_statement};

// ---------------------------------------------------------------------------
// Input resolution
// ---------------------------------------------------------------------------

/// Resolve a list of path-or-glob arguments into a sorted, deduplicated list
/// of `.sql` file paths.
///
/// Each element of `inputs` may be:
/// - A glob pattern (contains `*`, `?`, or `[`) — expanded via the `glob` crate
/// - A directory path — recursively walked for `.sql` files
/// - A file path — taken as-is (regardless of extension, so callers can be explicit)
///
/// Returns an error string per failed glob pattern; individual missing files
/// are returned as [`FileResult::error`] during analysis, not here.
pub fn resolve_inputs(inputs: &[String]) -> (Vec<PathBuf>, Vec<String>) {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for input in inputs {
        if is_glob(input) {
            expand_glob(input, &mut files, &mut errors);
        } else {
            let path = PathBuf::from(input);
            if path.is_dir() {
                collect_sql_under_dir(&path, &mut files);
            } else {
                // Plain file — add unconditionally; missing files surface during analyze_file
                files.push(path);
            }
        }
    }

    // Deduplicate (same file matched by multiple patterns) then sort
    files.sort();
    files.dedup();

    (files, errors)
}

fn is_glob(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[')
}

fn expand_glob(pattern: &str, files: &mut Vec<PathBuf>, errors: &mut Vec<String>) {
    match glob(pattern) {
        Err(e) => {
            errors.push(format!("Invalid glob pattern `{pattern}`: {e}"));
        }
        Ok(paths) => {
            let mut matched = false;
            for entry in paths {
                matched = true;
                match entry {
                    Ok(path) => {
                        if path.is_dir() {
                            collect_sql_under_dir(&path, files);
                        } else if path
                            .extension()
                            .map(|e| e.eq_ignore_ascii_case("sql"))
                            .unwrap_or(false)
                        {
                            files.push(path);
                        }
                        // Non-.sql files matched by the glob are silently skipped —
                        // the user asked for a pattern, not every file.
                    }
                    Err(e) => {
                        errors.push(format!("Cannot access path matched by `{pattern}`: {e}"));
                    }
                }
            }
            if !matched {
                errors.push(format!("Glob pattern `{pattern}` matched no files"));
            }
        }
    }
}

/// Recursively collect all `.sql` files under `dir`.
fn collect_sql_under_dir(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file()
                && e.path()
                    .extension()
                    .map(|x| x.eq_ignore_ascii_case("sql"))
                    .unwrap_or(false)
        })
        .map(|e| e.path().to_path_buf());
    out.extend(entries);
}

// ---------------------------------------------------------------------------
// Per-file analysis
// ---------------------------------------------------------------------------

/// Parse a single `.sql` file and run all rules against every statement.
pub fn analyze_file(path: &Path, config: &Config) -> FileResult {
    let sql = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => return FileResult::error(path.to_path_buf(), format!("Cannot read file: {e}")),
    };

    let dialect = config.dialect.sql_dialect();
    match Parser::parse_sql(dialect.as_ref(), &sql) {
        Ok(stmts) => {
            let mut findings: Vec<_> = stmts
                .iter()
                .flat_map(|s| check_statement(s, path, config))
                .collect();
            findings.extend(check_file_rules(&stmts, path, config));
            FileResult::ok(path.to_path_buf(), findings)
        }
        Err(e) => FileResult::error(path.to_path_buf(), format!("SQL parse error: {e}")),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_tmp() -> TempDir {
        tempfile::tempdir().expect("tmpdir")
    }

    fn write(dir: &Path, name: &str, content: &str) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, content).unwrap();
        p
    }

    // ── is_glob ──────────────────────────────────────────────────────────────

    #[test]
    fn is_glob_star() {
        assert!(is_glob("migrations/*.sql"));
    }

    #[test]
    fn is_glob_question() {
        assert!(is_glob("migrations/V?.sql"));
    }

    #[test]
    fn is_glob_bracket() {
        assert!(is_glob("migrations/V[12].sql"));
    }

    #[test]
    fn is_glob_plain_path_is_not_glob() {
        assert!(!is_glob("migrations/V1__init.sql"));
        assert!(!is_glob("migrations/"));
    }

    // ── resolve_inputs: directory ─────────────────────────────────────────────

    #[test]
    fn resolve_directory_collects_sql_files() {
        let tmp = make_tmp();
        write(tmp.path(), "V1__a.sql", "SELECT 1");
        write(tmp.path(), "V2__b.sql", "SELECT 2");
        write(tmp.path(), "README.md", "docs");

        let (files, errors) = resolve_inputs(&[tmp.path().to_str().unwrap().to_string()]);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| f.extension().unwrap() == "sql"));
    }

    #[test]
    fn resolve_directory_recurses() {
        let tmp = make_tmp();
        let sub = tmp.path().join("subdir");
        fs::create_dir(&sub).unwrap();
        write(tmp.path(), "V1__root.sql", "SELECT 1");
        write(&sub, "V2__nested.sql", "SELECT 2");

        let (files, errors) = resolve_inputs(&[tmp.path().to_str().unwrap().to_string()]);
        assert!(errors.is_empty());
        assert_eq!(files.len(), 2);
    }

    // ── resolve_inputs: plain file ────────────────────────────────────────────

    #[test]
    fn resolve_single_file() {
        let tmp = make_tmp();
        let f = write(tmp.path(), "V1__init.sql", "SELECT 1");

        let (files, errors) = resolve_inputs(&[f.to_str().unwrap().to_string()]);
        assert!(errors.is_empty());
        assert_eq!(files, vec![f]);
    }

    #[test]
    fn resolve_multiple_explicit_files() {
        let tmp = make_tmp();
        let a = write(tmp.path(), "V1__a.sql", "SELECT 1");
        let b = write(tmp.path(), "V2__b.sql", "SELECT 2");

        let inputs = vec![
            a.to_str().unwrap().to_string(),
            b.to_str().unwrap().to_string(),
        ];
        let (files, errors) = resolve_inputs(&inputs);
        assert!(errors.is_empty());
        assert_eq!(files.len(), 2);
    }

    // ── resolve_inputs: glob ──────────────────────────────────────────────────

    #[test]
    fn resolve_glob_matches_sql_files() {
        let tmp = make_tmp();
        write(tmp.path(), "V1__a.sql", "SELECT 1");
        write(tmp.path(), "V2__b.sql", "SELECT 2");
        write(tmp.path(), "notes.txt", "ignore me");

        let pattern = format!("{}/*.sql", tmp.path().display());
        let (files, errors) = resolve_inputs(&[pattern]);
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn resolve_glob_no_match_returns_error() {
        let (files, errors) = resolve_inputs(&["/nonexistent/path/*.sql".to_string()]);
        assert!(files.is_empty());
        assert!(!errors.is_empty());
        assert!(errors[0].contains("matched no files"));
    }

    #[test]
    fn resolve_invalid_glob_returns_error() {
        // A pattern that the glob crate considers invalid
        let (files, errors) = resolve_inputs(&["[invalid".to_string()]);
        assert!(files.is_empty());
        assert!(!errors.is_empty());
    }

    // ── deduplication ─────────────────────────────────────────────────────────

    #[test]
    fn resolve_deduplicates_overlapping_inputs() {
        let tmp = make_tmp();
        let f = write(tmp.path(), "V1__a.sql", "SELECT 1");

        // Pass the same file twice: once explicitly, once via glob
        let pattern = format!("{}/*.sql", tmp.path().display());
        let inputs = vec![f.to_str().unwrap().to_string(), pattern];
        let (files, errors) = resolve_inputs(&inputs);
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(files.len(), 1, "should deduplicate: {files:?}");
    }

    // ── mixed inputs ──────────────────────────────────────────────────────────

    #[test]
    fn resolve_mixed_file_dir_and_glob() {
        let tmp = make_tmp();
        let sub = tmp.path().join("sub");
        fs::create_dir(&sub).unwrap();

        let explicit = write(tmp.path(), "V1__explicit.sql", "SELECT 1");
        write(&sub, "V2__in_subdir.sql", "SELECT 2");
        let glob_target = write(tmp.path(), "V3__glob.sql", "SELECT 3");

        let pattern = format!("{}/*.sql", tmp.path().display());
        let inputs = vec![
            explicit.to_str().unwrap().to_string(), // explicit file (also matched by glob)
            sub.to_str().unwrap().to_string(),      // directory
            pattern,                                // glob
        ];
        let (files, errors) = resolve_inputs(&inputs);
        assert!(errors.is_empty(), "errors: {errors:?}");
        // explicit + glob_target from root (glob), V2 from subdir — explicit deduped
        assert!(files.contains(&explicit));
        assert!(files.contains(&glob_target));
        assert!(files
            .iter()
            .any(|f| f.file_name().unwrap() == "V2__in_subdir.sql"));
        // No duplicates
        let mut sorted = files.clone();
        sorted.dedup();
        assert_eq!(files.len(), sorted.len(), "duplicates found: {files:?}");
    }
}
