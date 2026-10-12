//! Writes the planned changes: atomic replace (temp file + rename), `.bak` of files that existed,
//! permissions preserved. User-scope Claude Code entries go through `claude mcp add` when available.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::init::json_config::SERVER_NAME;
use crate::init::plan::{Change, ClaudeCliCall, Status};

/// Applies every change that writes; failures are recorded on the change itself.
pub fn apply(changes: &mut [Change]) {
    for change in changes.iter_mut().filter(|change| change.status.writes()) {
        if let Some(call) = change.claude_cli.clone() {
            match run_claude_cli(&call) {
                Ok(()) => continue,
                // Fall back to editing the file: same entry, same result.
                Err(reason) => {
                    change.note = Some(format!("`claude mcp add` failed ({reason}); edited the file instead"))
                }
            }
        }
        if let Err(error) = write_change(change) {
            change.status = Status::Failed;
            change.detail = format!("cannot write: {error}");
        }
    }
}

fn write_change(change: &Change) -> io::Result<()> {
    let (Some(path), Some(contents)) = (&change.path, &change.contents) else {
        return Ok(());
    };
    write_atomically(path, contents)
}

/// Temp file in the same directory + rename, so a crash never leaves a half-written config.
pub fn write_atomically(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let previous_permissions = fs::metadata(path).ok().map(|metadata| metadata.permissions());
    if previous_permissions.is_some() {
        fs::copy(path, sibling(path, "bak"))?;
    }
    let temporary = sibling(path, "tmp");
    fs::write(&temporary, contents)?;
    if let Some(permissions) = previous_permissions {
        fs::set_permissions(&temporary, permissions)?;
    }
    fs::rename(&temporary, path).inspect_err(|_| {
        let _ = fs::remove_file(&temporary);
    })
}

/// `config.json` → `config.json.<suffix>`.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().map(|name| name.to_os_string()).unwrap_or_default();
    name.push(format!(".{suffix}"));
    path.with_file_name(name)
}

fn run_claude_cli(call: &ClaudeCliCall) -> Result<(), String> {
    if call.replace {
        run(&call.program, &["mcp", "remove", "--scope", "user", SERVER_NAME])?;
    }
    run(
        &call.program,
        &["mcp", "add", "--scope", "user", "--transport", "stdio", SERVER_NAME, "--", &call.command, "mcp"],
    )
}

fn run(program: &Path, args: &[&str]) -> Result<(), String> {
    let output = Command::new(program).args(args).output().map_err(|error| error.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(stderr.lines().next().unwrap_or("non-zero exit").trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_creates_parents_and_backs_up() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("a/b/config.json");
        write_atomically(&path, "one").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "one");
        assert!(!sibling(&path, "bak").exists(), "no backup for new files");
        write_atomically(&path, "two").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        assert_eq!(fs::read_to_string(sibling(&path, "bak")).unwrap(), "one");
        assert!(!sibling(&path, "tmp").exists());
    }

    #[cfg(unix)]
    #[test]
    fn permissions_are_preserved() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("secret.json");
        fs::write(&path, "{}").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        write_atomically(&path, "{ }").unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
