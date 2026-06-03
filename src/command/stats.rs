//! Stats command for counting files by extension in the working directory.
//!
//! This module implements a `libra stats` command that scans the current
//! working directory recursively, counts files grouped by their file extension,
//! and reports the distribution in human-readable or JSON format. It is a
//! read-only command that does not require a Libra repository.
//!
//! - **Argument parsing** is handled by [`StatsArgs`], which currently has no
//!   command-specific flags — the global `--json` flag is handled by the
//!   CLI dispatcher via [`OutputConfig`].
//!
//! - **Execution entrypoints**:
//!   - [`execute`] is the user-facing async entrypoint used by the CLI
//!     dispatcher for non-structured invocations.
//!   - [`execute_safe`] is the structured handler that respects
//!     [`OutputConfig`] and returns a [`CliResult`].
//!
//! - **Collection**:
//!   - [`collect_stats`] walks the current directory recursively, skipping
//!     `.libra/` and `target/` directories.
//!   - Files without an extension are grouped under `"no_extension"`.
//!   - Results are sorted by count descending, then by extension name
//!     ascending for stable output.

use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::Path,
};

use clap::Parser;
use serde::Serialize;

use crate::utils::{
    error::{CliError, CliResult},
    output::{OutputConfig, emit_json_data},
};

const STATS_EXAMPLES: &str = "\
EXAMPLES:
    libra stats                     Count files grouped by extension
    libra stats --json              JSON output for agents";

/// Directories that are skipped during the file scan.
const IGNORED_DIRS: &[&str] = &[".libra", "target"];

#[derive(Parser, Debug)]
#[command(after_help = STATS_EXAMPLES)]
pub struct StatsArgs {
    // No command-specific flags — the global --json flag is handled by the
    // CLI dispatcher through OutputConfig.
}

/// A single extension group with its file count.
#[derive(Debug, Clone, Serialize)]
struct ExtensionGroup {
    extension: String,
    count: usize,
}

/// Aggregated stats output, used for both human and JSON rendering.
#[derive(Debug, Clone, Serialize)]
struct StatsOutput {
    total_files: usize,
    groups: Vec<ExtensionGroup>,
}

/// User-facing entrypoint used by the CLI dispatcher for non-structured
/// invocations. Prints errors to stderr and exits.
pub async fn execute(_args: StatsArgs) {
    if let Err(e) = execute_safe(StatsArgs {}, &OutputConfig::default()).await {
        e.print_stderr();
    }
}

/// Structured entrypoint that respects [`OutputConfig`].
///
/// # Side Effects
/// - Reads the filesystem starting from the current working directory.
/// - Writes human-readable output to stdout when not in JSON/quiet mode.
/// - Emits a JSON envelope to stdout when `--json` is active.
///
/// # Errors
/// - Returns [`CliError`] when the current directory cannot be read or when
///   stdout cannot be written to.
pub async fn execute_safe(_args: StatsArgs, output: &OutputConfig) -> CliResult<()> {
    let stats = collect_stats()?;

    if output.is_json() {
        emit_json_data("stats", &stats, output)?;
    } else if !output.quiet {
        let mut stdout = io::stdout();
        render_stats(&stats, &mut stdout)?;
    }

    Ok(())
}

/// Walk the current working directory and collect per-extension file counts.
///
/// Skips directories whose name matches [`IGNORED_DIRS`]. Files without an
/// extension are counted under `"no_extension"`. Symlinks are followed by
/// `fs::read_dir` (which does not follow symlinks to directories by default
/// on most platforms) — only regular files are counted.
fn collect_stats() -> CliResult<StatsOutput> {
    let current_dir = std::env::current_dir()
        .map_err(|e| CliError::fatal(format!("failed to get current directory: {e}")))?;

    let mut extension_counts: BTreeMap<String, usize> = BTreeMap::new();

    walk_dir(&current_dir, &mut extension_counts).map_err(|e| {
        CliError::fatal(format!(
            "failed to scan directory '{}': {e}",
            current_dir.display()
        ))
    })?;

    let mut groups: Vec<ExtensionGroup> = extension_counts
        .into_iter()
        .map(|(ext, count)| ExtensionGroup {
            extension: ext,
            count,
        })
        .collect();

    // Sort by count descending, then by extension name ascending for stability.
    groups.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.extension.cmp(&b.extension))
    });

    let total_files: usize = groups.iter().map(|g| g.count).sum();

    Ok(StatsOutput {
        total_files,
        groups,
    })
}

/// Recursively walk a directory, accumulating per-extension file counts into
/// `counts`. Directories whose bare name matches an entry in [`IGNORED_DIRS`]
/// are skipped entirely.
fn walk_dir(dir: &Path, counts: &mut BTreeMap<String, usize>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if IGNORED_DIRS.contains(&file_name) {
                continue;
            }
            walk_dir(&path, counts)?;
        } else if path.is_file() {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("no_extension")
                .to_string();
            *counts.entry(ext).or_insert(0) += 1;
        }
    }
    Ok(())
}

/// Render the stats as a human-readable table.
///
/// Output format:
/// ```text
/// Total files: <N>
///
///   <count>  <extension>
///   ...
/// ```
fn render_stats(stats: &StatsOutput, writer: &mut impl Write) -> CliResult<()> {
    let max_count = stats.groups.iter().map(|g| g.count).max().unwrap_or(0);
    let count_width = std::cmp::max(4, max_count.to_string().len());

    writeln!(writer, "Total files: {}", stats.total_files)
        .map_err(|e| CliError::io(format!("failed to write stats output: {e}")))?;
    writeln!(writer).map_err(|e| CliError::io(format!("failed to write stats output: {e}")))?;

    for group in &stats.groups {
        writeln!(
            writer,
            "{:>width$}  {}",
            group.count,
            group.extension,
            width = count_width
        )
        .map_err(|e| CliError::io(format!("failed to write stats output: {e}")))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::utils::{
        output::OutputConfig,
        test::{self, ChangeDirGuard},
    };

    #[test]
    fn test_parse_args() {
        let args = StatsArgs::parse_from(["stats"]);
        // StatsArgs has no fields — just verify parsing succeeds.
        let _ = args;
    }

    #[test]
    fn test_collect_stats_in_tempdir() {
        let temp = tempdir().unwrap();
        test::setup_clean_testing_env_in(temp.path());

        // Create files with various extensions
        fs::write(temp.path().join("main.rs"), "fn main() {}").unwrap();
        fs::write(temp.path().join("lib.rs"), "pub mod foo;").unwrap();
        fs::write(temp.path().join("README.md"), "# Title").unwrap();
        fs::write(temp.path().join("Makefile"), "all:").unwrap();
        fs::write(temp.path().join("config.toml"), "[core]").unwrap();
        fs::write(temp.path().join("script"), "#!/bin/sh").unwrap();
        fs::create_dir(temp.path().join("src")).unwrap();
        fs::write(temp.path().join("src/parser.rs"), "// parser").unwrap();

        let _guard = ChangeDirGuard::new(temp.path());
        let stats = collect_stats().unwrap();

        assert_eq!(stats.total_files, 7);
        assert_eq!(stats.groups.len(), 4); // rs, md, toml, no_extension

        // rs should have the most (3 files: main.rs, lib.rs, src/parser.rs)
        let rs_group = stats.groups.iter().find(|g| g.extension == "rs").unwrap();
        assert_eq!(rs_group.count, 3);

        // Verify sort order: rs (3) first, then alphabetically by extension
        assert_eq!(stats.groups[0].extension, "rs");
    }

    #[test]
    fn test_stats_ignores_libra_and_target_dirs() {
        let temp = tempdir().unwrap();
        test::setup_clean_testing_env_in(temp.path());

        fs::write(temp.path().join("root_file.txt"), "root").unwrap();
        fs::create_dir(temp.path().join(".libra")).unwrap();
        fs::write(temp.path().join(".libra/config"), "config").unwrap();
        fs::write(temp.path().join(".libra/HEAD"), "ref").unwrap();
        fs::create_dir(temp.path().join("target")).unwrap();
        fs::write(temp.path().join("target/debug.o"), "obj").unwrap();

        let _guard = ChangeDirGuard::new(temp.path());
        let stats = collect_stats().unwrap();

        // Only root_file.txt should be counted
        assert_eq!(stats.total_files, 1);
        assert_eq!(stats.groups.len(), 1);
        assert_eq!(stats.groups[0].extension, "txt");
        assert_eq!(stats.groups[0].count, 1);
    }

    #[test]
    fn test_render_stats_output() {
        let stats = StatsOutput {
            total_files: 5,
            groups: vec![
                ExtensionGroup {
                    extension: "rs".to_string(),
                    count: 3,
                },
                ExtensionGroup {
                    extension: "md".to_string(),
                    count: 1,
                },
                ExtensionGroup {
                    extension: "no_extension".to_string(),
                    count: 1,
                },
            ],
        };

        let mut buf = Vec::new();
        render_stats(&stats, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("Total files: 5"));
        assert!(output.contains("rs"));
        assert!(output.contains("md"));
        assert!(output.contains("no_extension"));
        // rs group count of 3 should appear in the output
        assert!(output.contains("3"));
    }

    #[tokio::test]
    async fn test_execute_safe_quiet_mode() {
        let temp = tempdir().unwrap();
        test::setup_clean_testing_env_in(temp.path());
        fs::write(temp.path().join("hello.rs"), "fn main() {}").unwrap();

        let _guard = ChangeDirGuard::new(temp.path());
        let output = OutputConfig {
            quiet: true,
            ..OutputConfig::default()
        };

        let result = execute_safe(StatsArgs {}, &output).await;
        assert!(result.is_ok());
    }
}
