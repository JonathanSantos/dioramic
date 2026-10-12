//! `cargo xtask golden [--update]`: runs every workspace test whose name contains `golden`
//! (convention of `test_support::golden`), optionally with `DIORAMIC_UPDATE_GOLDEN=1`, and reports which
//! files under `tests/golden/` were created or changed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use test_support::golden::UPDATE_ENV;

use crate::cargo::{self, format_duration};

/// Test-name filter selecting the golden tests.
const GOLDEN_TEST_FILTER: &str = "golden";
/// Lines of failing output shown.
const MAX_FAILURE_LINES: usize = 60;

/// Runs the golden tests; `Ok(false)` when they fail.
pub fn run(update: bool) -> Result<bool> {
    let directory = cargo::workspace_root().join("tests").join("golden");
    let before = snapshot(&directory);
    let environment: Vec<(&str, &str)> = if update { vec![(UPDATE_ENV, "1")] } else { Vec::new() };
    let mode = if update { "update" } else { "check" };
    println!("golden {mode}: cargo test --workspace -- {GOLDEN_TEST_FILTER}");
    let run = cargo::run(
        &["test", "--workspace", "--quiet", "--message-format=short", "--", GOLDEN_TEST_FILTER],
        &environment,
    )?;
    let after = snapshot(&directory);
    print_changes(&directory, &before, &after);
    let passed = count_passed(&run.output);
    if passed == 0 && run.success {
        println!("  no golden tests found (name tests with `golden` and call `test_support::golden!`)");
    }
    if !run.success {
        let failures: Vec<&str> = run.output.lines().filter(|line| !line.trim().is_empty()).collect();
        for line in &failures[failures.len().saturating_sub(MAX_FAILURE_LINES)..] {
            println!("    {line}");
        }
    }
    let status = if run.success { "ok" } else { "FAILED" };
    println!("golden {mode}: {status} ({passed} passed) in {}", format_duration(run.elapsed));
    Ok(run.success)
}

/// Contents of every file under `directory`, keyed by relative path.
fn snapshot(directory: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    collect(directory, directory, &mut files);
    files
}

fn collect(root: &Path, directory: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
    let Ok(entries) = std::fs::read_dir(directory) else { return };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, files);
        } else if let Ok(bytes) = std::fs::read(&path) {
            files.insert(path.strip_prefix(root).map(Path::to_path_buf).unwrap_or(path), bytes);
        }
    }
}

fn print_changes(directory: &Path, before: &BTreeMap<PathBuf, Vec<u8>>, after: &BTreeMap<PathBuf, Vec<u8>>) {
    let created: Vec<&PathBuf> = after.keys().filter(|path| !before.contains_key(*path)).collect();
    let changed: Vec<&PathBuf> = after
        .iter()
        .filter(|(path, bytes)| before.get(*path).is_some_and(|old| old != *bytes))
        .map(|(path, _)| path)
        .collect();
    let unchanged = after.len() - created.len() - changed.len();
    println!("  {}: {} created, {} changed, {unchanged} unchanged", directory.display(), created.len(), changed.len());
    for path in created {
        println!("    + {}", path.display());
    }
    for path in changed {
        println!("    ~ {}", path.display());
    }
}

fn count_passed(output: &str) -> u64 {
    output
        .lines()
        .filter_map(|line| line.trim().strip_prefix("test result:"))
        .filter_map(|line| line.split(';').next())
        .filter_map(|part| part.split_whitespace().rev().nth(1).and_then(|number| number.parse::<u64>().ok()))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passed_tests_are_summed() {
        let output = "test result: ok. 3 passed; 0 failed\ntest result: ok. 0 passed; 0 failed\ntest result: ok. 2 passed; 0 failed";
        assert_eq!(count_passed(output), 5);
    }
}
