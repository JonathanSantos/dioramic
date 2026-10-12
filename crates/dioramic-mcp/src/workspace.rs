//! Where the server may read specs and write outputs (`docs/05-mcp.md#segurança`): only inside the roots
//! the client announced (MCP `roots`), or the working directory when it announced none.

use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};

use thiserror::Error;

/// Why a path was refused.
#[derive(Debug, Error)]
pub enum PathError {
    /// The path resolves outside every allowed root.
    #[error("path '{path}' is outside the allowed roots ({roots}); use a path inside the project")]
    OutsideRoots {
        /// Requested path (absolute, normalized).
        path: String,
        /// Allowed roots, comma separated.
        roots: String,
    },
    /// No usable root (no client roots and no working directory).
    #[error("no workspace root available: {0}")]
    NoRoot(String),
    /// Filesystem error while resolving.
    #[error("cannot resolve '{path}': {source}")]
    Io {
        /// Path being resolved.
        path: String,
        /// Cause.
        source: io::Error,
    },
}

/// Allowed roots, canonicalized. The first one is the base for relative paths and `./diagrams/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    roots: Vec<PathBuf>,
}

impl Workspace {
    /// Workspace from explicit directories (client roots). Directories that do not exist are skipped.
    pub fn from_dirs(dirs: impl IntoIterator<Item = PathBuf>) -> Result<Self, PathError> {
        let roots: Vec<PathBuf> = dirs.into_iter().filter_map(|dir| dir.canonicalize().ok()).collect();
        if roots.is_empty() {
            return Err(PathError::NoRoot("none of the client roots exists on disk".to_owned()));
        }
        Ok(Self { roots })
    }

    /// Workspace from `file://` root URIs (MCP `roots/list`). Other schemes are ignored.
    pub fn from_root_uris<'a>(uris: impl IntoIterator<Item = &'a str>) -> Result<Self, PathError> {
        Self::from_dirs(uris.into_iter().filter_map(file_uri_to_path))
    }

    /// The process working directory. A server started with `/` as cwd (GUI clients such as Claude
    /// Desktop) would otherwise allow the whole disk, so `/` falls back to the home directory.
    pub fn current_dir() -> Result<Self, PathError> {
        let cwd = std::env::current_dir().map_err(|error| PathError::NoRoot(error.to_string()))?;
        if cwd.parent().is_some() {
            return Self::from_dirs([cwd]);
        }
        let home = home_dir().ok_or_else(|| PathError::NoRoot("cwd is '/' and HOME is not set".to_owned()))?;
        Self::from_dirs([home])
    }

    /// Base directory for relative paths.
    pub fn base(&self) -> &Path {
        // `from_dirs` guarantees at least one root.
        self.roots.first().map_or(Path::new("."), PathBuf::as_path)
    }

    /// Allowed roots.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Resolves `path` (relative to [`Workspace::base`]) and checks it stays inside a root, following
    /// symlinks of the existing part. The file itself may not exist yet (outputs).
    pub fn resolve(&self, path: &Path) -> Result<PathBuf, PathError> {
        let absolute = if path.is_absolute() { path.to_path_buf() } else { self.base().join(path) };
        let resolved = canonicalize_existing_prefix(&normalize_lexically(&absolute))?;
        if self.roots.iter().any(|root| resolved.starts_with(root)) {
            Ok(resolved)
        } else {
            Err(PathError::OutsideRoots { path: resolved.display().to_string(), roots: self.to_string() })
        }
    }

    /// `path` relative to the base when inside it (shorter for the summary), else absolute.
    pub fn display_path(&self, path: &Path) -> String {
        path.strip_prefix(self.base()).unwrap_or(path).display().to_string()
    }
}

impl fmt::Display for Workspace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<String> = self.roots.iter().map(|root| root.display().to_string()).collect();
        f.write_str(&names.join(", "))
    }
}

/// Removes `.` and resolves `..` without touching the disk (`..` above the root is dropped).
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    normalized
}

/// Canonicalizes the longest existing ancestor (resolving symlinks) and re-appends the missing tail.
fn canonicalize_existing_prefix(path: &Path) -> Result<PathBuf, PathError> {
    let mut existing = path;
    let mut missing: Vec<&std::ffi::OsStr> = Vec::new();
    loop {
        match existing.canonicalize() {
            Ok(canonical) => {
                return Ok(missing.iter().rev().fold(canonical, |accumulated, part| accumulated.join(part)));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let (Some(parent), Some(name)) = (existing.parent(), existing.file_name()) else {
                    return Err(PathError::Io { path: path.display().to_string(), source: error });
                };
                missing.push(name);
                existing = parent;
            }
            Err(error) => return Err(PathError::Io { path: path.display().to_string(), source: error }),
        }
    }
}

/// `file:///Users/me/my%20project` → `/Users/me/my project`. `None` for other schemes or hosts.
pub fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    if !path.starts_with('/') {
        return None;
    }
    let decoded = percent_decode(path)?;
    // Windows URIs look like `/C:/Users/...`.
    let windows_drive = decoded.len() > 2 && decoded.as_bytes()[2] == b':';
    Some(PathBuf::from(if windows_drive { &decoded[1..] } else { decoded.as_str() }))
}

fn percent_decode(text: &str) -> Option<String> {
    const HEX_RADIX: u32 = 16;
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = text.get(index + 1..index + 3)?;
            decoded.push(u8::from_str_radix(hex, HEX_RADIX).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_workspace() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::from_dirs([dir.path().to_path_buf()]).unwrap();
        (dir, workspace)
    }

    #[test]
    fn relative_paths_resolve_against_the_base() {
        let (_dir, workspace) = temp_workspace();
        let resolved = workspace.resolve(Path::new("diagrams/out.png")).unwrap();
        assert_eq!(resolved, workspace.base().join("diagrams/out.png"));
        assert_eq!(workspace.display_path(&resolved), "diagrams/out.png");
    }

    #[test]
    fn parent_traversal_out_of_the_root_is_refused() {
        let (_dir, workspace) = temp_workspace();
        let error = workspace.resolve(Path::new("../escape.png")).unwrap_err();
        assert!(matches!(error, PathError::OutsideRoots { .. }), "{error}");
    }

    #[test]
    fn absolute_paths_outside_are_refused_inside_are_kept() {
        let (_dir, workspace) = temp_workspace();
        assert!(workspace.resolve(Path::new("/etc/passwd")).is_err());
        let inside = workspace.base().join("a/../b.svg");
        assert_eq!(workspace.resolve(&inside).unwrap(), workspace.base().join("b.svg"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_pointing_outside_are_refused() {
        let (_dir, workspace) = temp_workspace();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), workspace.base().join("link")).unwrap();
        assert!(workspace.resolve(Path::new("link/out.png")).is_err());
    }

    #[test]
    fn file_uris_are_decoded() {
        assert_eq!(file_uri_to_path("file:///Users/me/my%20project"), Some(PathBuf::from("/Users/me/my project")));
        assert_eq!(file_uri_to_path("file://localhost/tmp"), Some(PathBuf::from("/tmp")));
        assert_eq!(file_uri_to_path("https://example.com"), None);
        assert_eq!(file_uri_to_path("file:///bad%zz"), None);
    }

    #[test]
    fn missing_roots_are_skipped_and_none_is_an_error() {
        assert!(Workspace::from_dirs([PathBuf::from("/definitely/not/here")]).is_err());
        let (dir, _) = temp_workspace();
        let uri = format!("file://{}", dir.path().display());
        let workspace = Workspace::from_root_uris(["file:///definitely/not/here", uri.as_str()]).unwrap();
        assert_eq!(workspace.roots().len(), 1);
    }
}
