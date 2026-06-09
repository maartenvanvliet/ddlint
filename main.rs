mod analyzer;
mod config;
mod finding;
mod output;
mod rules;

use std::path::PathBuf;
use std::process;

use anyhow::Result;
use clap::Parser;

use analyzer::{analyze_file, resolve_inputs};
use config::load_config;
use output::{print_results, print_summary, OutputFormat};

/// Lint Flyway SQL migration files for MySQL zero-downtime safety.
///
/// INPUTS can be any mix of:
///   - A directory   → recursively collects all .sql files inside it
///   - A file path   → lints that file directly
///   - A glob pattern (quoted to prevent shell expansion) → expanded internally
///
/// Examples:
///   flyway-linter migrations/
///   flyway-linter migrations/V1__init.sql migrations/V2__add_index.sql
///   flyway-linter 'migrations/V*.sql'
///   flyway-linter --config linter.yml migrations/
///
/// Exits 0 when all migrations are clean, 1 when findings are present,
/// 2 on usage or I/O errors.
#[derive(Parser, Debug)]
#[command(
    name = "flyway-linter",
    version,
    about = "Lint Flyway SQL migrations for MySQL zero-downtime safety",
)]
struct Cli {
    /// One or more migration files, directories, or glob patterns.
    #[arg(required = true, value_name = "INPUT")]
    inputs: Vec<String>,

    /// Path to a config file (YAML). If omitted, looks for flyway-linter.yml
    /// in the current directory. If that also doesn't exist, all rules run
    /// at their default severity.
    #[arg(long, short, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Output format.
    ///
    /// `text` — coloured human-readable output (default).
    ///
    /// `gha`  — GitHub Actions workflow commands. Emits ::error and ::warning
    ///          annotations that appear inline on PR diffs.
    #[arg(long, short, default_value = "text", value_name = "FORMAT")]
    format: OutputFormat,

    /// Treat warnings as errors (exit 1 if any warnings are present).
    #[arg(long)]
    strict: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load config — hard error if an explicit --config path is missing or invalid
    let cfg = match load_config(cli.config.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    };

    let (files, glob_errors) = resolve_inputs(&cli.inputs);

    for err in &glob_errors {
        eprintln!("error: {err}");
    }
    if !glob_errors.is_empty() && files.is_empty() {
        process::exit(2);
    }

    if files.is_empty() {
        eprintln!("No .sql files found for the given inputs");
        process::exit(2);
    }

    if cli.format == OutputFormat::Text {
        println!("flyway-linter — {} migration file(s)\n", files.len());
    }

    let results: Vec<_> = files.iter().map(|p| analyze_file(p, &cfg)).collect();

    print_results(&results, &cli.format);
    print_summary(&results, &cli.format);

    let has_danger = results.iter().any(|r| r.has_issues());
    let has_warning = results
        .iter()
        .flat_map(|r| &r.findings)
        .any(|f| f.severity == finding::Severity::Warning);

    if has_danger || (cli.strict && has_warning) {
        process::exit(1);
    }

    Ok(())
}
