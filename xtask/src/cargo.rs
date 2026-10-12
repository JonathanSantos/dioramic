//! Running nested `cargo` commands with captured output.

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

/// Result of a captured cargo invocation.
#[derive(Debug)]
pub struct CargoRun {
    /// Exit status was success.
    pub success: bool,
    /// stdout followed by stderr.
    pub output: String,
    /// Wall time.
    pub elapsed: Duration,
}

/// Workspace root (parent of `xtask/`).
pub fn workspace_root() -> PathBuf {
    test_support::workspace_root()
}

/// Runs `cargo <args>` in the workspace root with extra environment variables, capturing everything.
/// `CARGO_TARGET_DIR` and friends are inherited, so parallel agents keep their own build directories.
pub fn run(args: &[&str], environment: &[(&str, &str)]) -> Result<CargoRun> {
    let started = Instant::now();
    let mut command = Command::new(cargo_binary());
    command.args(args).current_dir(workspace_root());
    for (key, value) in environment {
        command.env(key, value);
    }
    let output = command.output().with_context(|| format!("failed to start `cargo {}`", args.join(" ")))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(CargoRun { success: output.status.success(), output: text, elapsed: started.elapsed() })
}

/// Names of the workspace packages (`cargo metadata --no-deps`).
pub fn workspace_packages() -> Result<Vec<String>> {
    let run = run(&["metadata", "--no-deps", "--format-version", "1", "--quiet"], &[])?;
    if !run.success {
        bail!("cargo metadata failed:\n{}", run.output);
    }
    let json_start = run.output.find('{').unwrap_or(0);
    let metadata: serde_json::Value = serde_json::from_str(run.output[json_start..].lines().next().unwrap_or_default())
        .context("cargo metadata output")?;
    let mut names: Vec<String> = metadata["packages"]
        .as_array()
        .map(|packages| packages.iter().filter_map(|package| package["name"].as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    names.sort();
    Ok(names)
}

/// `1.2s` / `340ms`.
pub fn format_duration(duration: Duration) -> String {
    if duration < Duration::from_secs(1) {
        format!("{}ms", duration.as_millis())
    } else {
        format!("{:.1}s", duration.as_secs_f64())
    }
}

fn cargo_binary() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_are_short_and_readable() {
        assert_eq!(format_duration(Duration::from_millis(340)), "340ms");
        assert_eq!(format_duration(Duration::from_millis(12_345)), "12.3s");
    }
}
