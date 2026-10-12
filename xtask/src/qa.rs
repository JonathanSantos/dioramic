//! `cargo xtask qa <crate|all>`: fmt check, clippy `-D warnings` (all targets) and tests of one crate,
//! with one line per step and only the relevant excerpt of the output on failure.

use std::time::{Duration, Instant};

use anyhow::{Result, bail};

use crate::cargo::{self, CargoRun, format_duration};

/// Lines of output shown per failing step.
const MAX_EXCERPT_LINES: usize = 40;
/// Prefix of workspace crates, so `qa core` means `qa dioramic-core`.
const CRATE_PREFIX: &str = "dioramic-";
/// Selects every workspace member.
const ALL: &str = "all";

/// What to check.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Scope {
    Package(String),
    Workspace,
}

impl Scope {
    fn selector(&self) -> Vec<&str> {
        match self {
            Self::Package(name) => vec!["-p", name],
            Self::Workspace => vec!["--workspace"],
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Package(name) => name,
            Self::Workspace => ALL,
        }
    }
}

/// One QA step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Fmt,
    Clippy,
    Test,
}

impl Step {
    const ALL: [Step; 3] = [Step::Fmt, Step::Clippy, Step::Test];

    fn name(self) -> &'static str {
        match self {
            Self::Fmt => "fmt",
            Self::Clippy => "clippy",
            Self::Test => "test",
        }
    }

    fn arguments(self, scope: &Scope) -> Vec<String> {
        let mut arguments: Vec<&str> = match self {
            Self::Fmt => vec!["fmt"],
            Self::Clippy => vec!["clippy", "--all-targets", "--no-deps", "--quiet", "--message-format=short"],
            Self::Test => vec!["test", "--quiet", "--message-format=short"],
        };
        match (self, scope) {
            // `cargo fmt` has no `--workspace`.
            (Self::Fmt, Scope::Workspace) => arguments.push("--all"),
            _ => arguments.extend(scope.selector()),
        }
        let trailing: &[&str] = match self {
            Self::Fmt => &["--", "--check"],
            Self::Clippy => &["--", "-D", "warnings"],
            Self::Test => &[],
        };
        arguments.extend(trailing);
        arguments.into_iter().map(str::to_owned).collect()
    }

    /// Short success detail (test counts) or the failure excerpt.
    fn summarize(self, run: &CargoRun, scope: &Scope) -> String {
        match (self, run.success) {
            (Self::Test, true) => TestCounts::parse(&run.output).to_string(),
            (_, true) => String::new(),
            (Self::Fmt, false) => fmt_excerpt(&run.output, scope),
            (Self::Clippy, false) => diagnostic_excerpt(&run.output),
            (Self::Test, false) => test_excerpt(&run.output),
        }
    }
}

/// Runs the QA steps; `Ok(false)` when any step failed.
pub fn run(target: &str) -> Result<bool> {
    let started = Instant::now();
    let scope = resolve_scope(target)?;
    println!("qa {}", scope.name());
    let mut failed: Vec<&str> = Vec::new();
    for step in Step::ALL {
        let arguments = step.arguments(&scope);
        let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
        let run = cargo::run(&arguments, &[])?;
        print_step(step, &run, &scope);
        if !run.success {
            failed.push(step.name());
        }
    }
    print_summary(&scope, &failed, started.elapsed());
    Ok(failed.is_empty())
}

fn print_step(step: Step, run: &CargoRun, scope: &Scope) {
    let status = if run.success { "ok  " } else { "FAIL" };
    let detail = step.summarize(run, scope);
    let inline = if run.success && !detail.is_empty() { format!("  {detail}") } else { String::new() };
    println!("  {:<7}{status} {:>7}{inline}", step.name(), format_duration(run.elapsed));
    if !run.success {
        for line in detail.lines() {
            println!("      {line}");
        }
    }
}

fn print_summary(scope: &Scope, failed: &[&str], elapsed: Duration) {
    let total = format_duration(elapsed);
    if failed.is_empty() {
        println!("qa {}: ok ({} steps in {total})", scope.name(), Step::ALL.len());
    } else {
        println!("qa {}: FAILED ({}) in {total}", scope.name(), failed.join(", "));
    }
}

fn resolve_scope(target: &str) -> Result<Scope> {
    if target == ALL {
        return Ok(Scope::Workspace);
    }
    let packages = cargo::workspace_packages()?;
    let prefixed = format!("{CRATE_PREFIX}{target}");
    match [target, prefixed.as_str()].into_iter().find(|name| packages.iter().any(|package| package == name)) {
        Some(name) => Ok(Scope::Package(name.to_owned())),
        None => bail!("unknown crate '{target}'; known: {}, {ALL}", packages.join(", ")),
    }
}

/// Totals of every `test result:` line of a `cargo test` run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct TestCounts {
    passed: u64,
    failed: u64,
    ignored: u64,
}

impl TestCounts {
    fn parse(output: &str) -> Self {
        output.lines().filter_map(|line| line.trim().strip_prefix("test result:")).fold(Self::default(), |sum, line| {
            Self {
                passed: sum.passed + count_before(line, " passed"),
                failed: sum.failed + count_before(line, " failed"),
                ignored: sum.ignored + count_before(line, " ignored"),
            }
        })
    }
}

impl std::fmt::Display for TestCounts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} passed, {} failed, {} ignored", self.passed, self.failed, self.ignored)
    }
}

/// The number right before `label` in `"ok. 12 passed; 0 failed; …"`.
fn count_before(line: &str, label: &str) -> u64 {
    line.split(';')
        .find_map(|part| {
            part.trim().trim_start_matches("ok.").trim_start_matches("FAILED.").trim().strip_suffix(label.trim())
        })
        .and_then(|number| number.trim().parse().ok())
        .unwrap_or(0)
}

fn fmt_excerpt(output: &str, scope: &Scope) -> String {
    let files: Vec<&str> = output.lines().filter(|line| line.starts_with("Diff in")).collect();
    let fix = match scope {
        Scope::Package(name) => format!("cargo fmt -p {name}"),
        Scope::Workspace => "cargo fmt --all".to_owned(),
    };
    let mut lines: Vec<String> = deduplicated(files.into_iter()).into_iter().map(str::to_owned).collect();
    if lines.is_empty() {
        lines = tail(output);
    }
    lines.push(format!("fix: {fix}"));
    lines.join("\n")
}

/// `path:line:col: error: …` lines from `--message-format=short`, deduplicated. Warnings are skipped:
/// with `-D warnings` the crate's own ones are errors, the rest belong to dependencies.
fn diagnostic_excerpt(output: &str) -> String {
    let diagnostics = deduplicated(output.lines().filter(|line| line.contains(": error")));
    let lines: Vec<String> =
        if diagnostics.is_empty() { tail(output) } else { diagnostics.into_iter().map(str::to_owned).collect() };
    truncate(lines).join("\n")
}

/// Compile errors, then the `failures:` section (panic messages) and the result line.
fn test_excerpt(output: &str) -> String {
    let compile_errors: Vec<&str> = deduplicated(output.lines().filter(|line| line.contains(": error")));
    if !compile_errors.is_empty() {
        return truncate(compile_errors.into_iter().map(str::to_owned).collect()).join("\n");
    }
    let relevant: Vec<String> = output
        .lines()
        .skip_while(|line| line.trim() != "failures:")
        .filter(|line| !line.trim().is_empty() && !line.starts_with("note: run with `RUST_BACKTRACE"))
        .map(str::to_owned)
        .collect();
    truncate(if relevant.is_empty() { tail(output) } else { relevant }).join("\n")
}

fn deduplicated<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut seen = std::collections::BTreeSet::new();
    lines.filter(|line| seen.insert(*line)).collect()
}

fn tail(output: &str) -> Vec<String> {
    let lines: Vec<&str> = output.lines().filter(|line| !line.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(MAX_EXCERPT_LINES)..].iter().map(|line| (*line).to_owned()).collect()
}

fn truncate(mut lines: Vec<String>) -> Vec<String> {
    if lines.len() > MAX_EXCERPT_LINES {
        let hidden = lines.len() - MAX_EXCERPT_LINES;
        lines.truncate(MAX_EXCERPT_LINES);
        lines.push(format!("… {hidden} more line(s)"));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counts_sum_every_result_line() {
        let output = "test result: ok. 12 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s\n\
                      test result: FAILED. 3 passed; 2 failed; 0 ignored; 0 measured\n";
        assert_eq!(TestCounts::parse(output), TestCounts { passed: 15, failed: 2, ignored: 1 });
    }

    #[test]
    fn package_steps_select_the_crate() {
        let scope = Scope::Package("dioramic-spec".into());
        assert_eq!(Step::Fmt.arguments(&scope), ["fmt", "-p", "dioramic-spec", "--", "--check"]);
        assert_eq!(
            Step::Clippy.arguments(&scope),
            [
                "clippy",
                "--all-targets",
                "--no-deps",
                "--quiet",
                "--message-format=short",
                "-p",
                "dioramic-spec",
                "--",
                "-D",
                "warnings"
            ]
        );
        assert_eq!(Step::Test.arguments(&scope), ["test", "--quiet", "--message-format=short", "-p", "dioramic-spec"]);
    }

    #[test]
    fn workspace_fmt_uses_all() {
        assert_eq!(Step::Fmt.arguments(&Scope::Workspace), ["fmt", "--all", "--", "--check"]);
        assert!(Step::Test.arguments(&Scope::Workspace).contains(&"--workspace".to_owned()));
    }

    #[test]
    fn clippy_excerpt_keeps_only_diagnostics() {
        let output = "    Checking foo\ndep/src/lib.rs:1:1: warning: unused\nsrc/lib.rs:3:5: error: unused variable: `x`\nsrc/lib.rs:3:5: error: unused variable: `x`\nerror: could not compile\n";
        assert_eq!(diagnostic_excerpt(output), "src/lib.rs:3:5: error: unused variable: `x`");
    }

    #[test]
    fn test_excerpt_shows_the_failures_section() {
        let output = "running 2 tests\n..F\nfailures:\n\n---- tests::bad stdout ----\nassertion failed\n\nfailures:\n    tests::bad\n\ntest result: FAILED. 1 passed; 1 failed\n";
        let excerpt = test_excerpt(output);
        assert!(excerpt.starts_with("failures:"));
        assert!(excerpt.contains("assertion failed"));
        assert!(!excerpt.contains("running 2 tests"));
    }
}
