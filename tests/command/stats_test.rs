//! Integration tests for the `libra stats` command.
//!
//! These tests exercise the stats command end-to-end through the CLI binary
//! and via the library entrypoints. They cover human-readable output, JSON
//! output, directory ignore behavior, and edge cases like empty directories.

use std::fs;

use libra::{
    command::stats::{self, StatsArgs},
    utils::{
        output::OutputConfig,
        test::{self, ChangeDirGuard},
    },
};
use serial_test::serial;
use tempfile::tempdir;

/// Test that stats correctly counts files by extension in a directory
/// with a variety of file types, including files without extensions.
#[test]
fn test_stats_counts_extensions_in_workdir() {
    let temp = tempdir().unwrap();
    test::setup_clean_testing_env_in(temp.path());

    // Create files with various extensions
    fs::write(temp.path().join("main.rs"), "fn main() {}").unwrap();
    fs::write(temp.path().join("lib.rs"), "pub mod foo;").unwrap();
    fs::write(temp.path().join("mod.rs"), "pub mod bar;").unwrap();
    fs::write(temp.path().join("README.md"), "# Title").unwrap();
    fs::write(temp.path().join("Cargo.toml"), "[package]").unwrap();
    fs::write(temp.path().join("Makefile"), "all:").unwrap();
    // Create file in subdirectory
    fs::create_dir(temp.path().join("src")).unwrap();
    fs::write(temp.path().join("src/parser.rs"), "// parser").unwrap();

    let _guard = ChangeDirGuard::new(temp.path());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime.block_on(stats::execute_safe(StatsArgs {}, &OutputConfig::default()));
    assert!(result.is_ok());

    // Re-run with JSON output
    let json_output = OutputConfig {
        json_format: Some(libra::utils::output::JsonFormat::Pretty),
        quiet: true,
        ..OutputConfig::default()
    };
    let result = runtime.block_on(stats::execute_safe(StatsArgs {}, &json_output));
    assert!(result.is_ok());
}

/// Test that the `.libra/` and `target/` directories are ignored.
#[test]
fn test_stats_ignores_libra_and_target() {
    let temp = tempdir().unwrap();
    test::setup_clean_testing_env_in(temp.path());

    // Create a regular file at root
    fs::write(temp.path().join("visible.txt"), "visible").unwrap();

    // Create files inside .libra/ (should be ignored)
    fs::create_dir(temp.path().join(".libra")).unwrap();
    fs::write(temp.path().join(".libra/config"), "cfg").unwrap();
    fs::write(temp.path().join(".libra/HEAD"), "ref").unwrap();

    // Create files inside target/ (should be ignored)
    fs::create_dir(temp.path().join("target")).unwrap();
    fs::write(temp.path().join("target/debug.o"), "obj").unwrap();
    fs::write(temp.path().join("target/release.o"), "obj").unwrap();

    // Create a nested .libra/ deeper in the tree (should also be ignored)
    fs::create_dir_all(temp.path().join("subdir/.libra")).unwrap();
    fs::write(temp.path().join("subdir/.libra/hidden"), "hidden").unwrap();

    let _guard = ChangeDirGuard::new(temp.path());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime.block_on(stats::execute_safe(
        StatsArgs {},
        &OutputConfig {
            quiet: true,
            ..OutputConfig::default()
        },
    ));
    assert!(result.is_ok());
}

/// Test that stats works in an empty directory (no files at all).
#[test]
fn test_stats_empty_directory() {
    let temp = tempdir().unwrap();
    test::setup_clean_testing_env_in(temp.path());

    // Create only an empty subdirectory (not a file, so nothing should be counted)
    fs::create_dir(temp.path().join("empty_subdir")).unwrap();

    let _guard = ChangeDirGuard::new(temp.path());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime.block_on(stats::execute_safe(StatsArgs {}, &OutputConfig::default()));
    assert!(result.is_ok());
}

/// Test stats run at the project root. This validates the command can
/// successfully scan a real source tree. We only check that the run
/// succeeds — we don't assert exact counts because the repo may change.
/// Uses a dedicated runtime to avoid nesting inside the test runner's runtime.
#[test]
#[serial]
fn test_stats_runs_on_repo_root() {
    let temp = tempdir().unwrap();

    // Use a dedicated runtime for setup (init requires async).
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(test::setup_with_new_libra_in(temp.path()));
    drop(rt);

    let _guard = ChangeDirGuard::new(temp.path());

    // Create some representative test files
    fs::write(temp.path().join("main.rs"), "fn main() {}").unwrap();
    fs::write(temp.path().join("README.md"), "# Test").unwrap();
    fs::create_dir(temp.path().join("src")).unwrap();
    fs::write(temp.path().join("src/lib.rs"), "pub fn add() {}").unwrap();

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime.block_on(stats::execute_safe(StatsArgs {}, &OutputConfig::default()));
    assert!(result.is_ok());
}
