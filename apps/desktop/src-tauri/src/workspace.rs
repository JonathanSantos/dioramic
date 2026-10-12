//! Folder scanning and spec file I/O. The engine never touches files; this module does.

use std::fs;
use std::path::{Path, PathBuf};

use dioramic_desktop_ipc::{FileKind, FileNode, Workspace};

use crate::error::CommandError;

/// File name suffixes recognised as Diorama specs.
pub const SPEC_SUFFIXES: [&str; 3] = [".diorama.yaml", ".diorama.yml", ".diorama.json"];

/// Folders never worth descending into (build output, dependencies, VCS).
const SKIPPED_FOLDERS: [&str; 4] = ["target", "node_modules", "dist", "build"];

/// Deep enough for any real project, shallow enough that opening `~` does not freeze the app.
const MAX_DEPTH: usize = 8;

/// True for `*.diorama.yaml` / `.yml` / `.json`.
pub fn is_spec_file(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()).is_some_and(|name| {
        let lower = name.to_ascii_lowercase();
        SPEC_SUFFIXES.iter().any(|suffix| lower.ends_with(suffix) && lower.len() > suffix.len())
    })
}

/// Scans `root` for specs. Folders without specs are left out so the tree stays focused.
pub fn scan(root: &Path) -> Result<Workspace, CommandError> {
    if !root.is_dir() {
        return Err(CommandError::NotAFolder(root.display().to_string()));
    }
    let name = root.file_name().map_or_else(|| root.display().to_string(), |name| name.to_string_lossy().into_owned());
    Ok(Workspace { root: root.display().to_string(), name, tree: scan_folder(root, 0) })
}

fn scan_folder(folder: &Path, depth: usize) -> Vec<FileNode> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(Result::ok).map(|entry| entry.path()).collect();
    paths.sort_by_key(|path| path.file_name().map(|name| name.to_string_lossy().to_lowercase()));

    let mut folders = Vec::new();
    let mut specs = Vec::new();
    for path in paths {
        if path.is_dir() {
            if depth < MAX_DEPTH && !is_skipped_folder(&path) {
                let children = scan_folder(&path, depth + 1);
                if !children.is_empty() {
                    folders.push(node(&path, FileKind::Folder, children));
                }
            }
        } else if is_spec_file(&path) {
            specs.push(node(&path, FileKind::Spec, Vec::new()));
        }
    }
    folders.extend(specs);
    folders
}

fn is_skipped_folder(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.') || SKIPPED_FOLDERS.contains(&name))
}

fn node(path: &Path, kind: FileKind, children: Vec<FileNode>) -> FileNode {
    FileNode {
        name: path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
        path: path.display().to_string(),
        kind,
        children,
    }
}

/// Reads a spec as text.
pub fn read_spec(path: &Path) -> Result<String, CommandError> {
    ensure_spec(path)?;
    fs::read_to_string(path).map_err(|error| CommandError::io(path, &error))
}

/// Writes the editor text verbatim (comments and formatting survive because nothing is reserialized).
pub fn save_spec(path: &Path, text: &str) -> Result<(), CommandError> {
    ensure_spec(path)?;
    fs::write(path, text).map_err(|error| CommandError::io(path, &error))
}

fn ensure_spec(path: &Path) -> Result<(), CommandError> {
    if is_spec_file(path) { Ok(()) } else { Err(CommandError::NotASpec(path.display().to_string())) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, "diorama: 1\n")
    }

    #[test]
    fn recognises_spec_suffixes() {
        assert!(is_spec_file(Path::new("/a/checkout.diorama.yaml")));
        assert!(is_spec_file(Path::new("/a/X.DIORAMA.YML")));
        assert!(is_spec_file(Path::new("a.diorama.json")));
        assert!(!is_spec_file(Path::new(".diorama.yaml")));
        assert!(!is_spec_file(Path::new("checkout.yaml")));
    }

    #[test]
    fn scan_keeps_only_folders_with_specs() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        touch(&dir.path().join("b.diorama.yaml"))?;
        touch(&dir.path().join("a.diorama.yaml"))?;
        touch(&dir.path().join("notes.md"))?;
        touch(&dir.path().join("nested/deep/c.diorama.yml"))?;
        touch(&dir.path().join("empty/readme.txt"))?;
        touch(&dir.path().join(".git/x.diorama.yaml"))?;
        touch(&dir.path().join("target/y.diorama.yaml"))?;

        let workspace = scan(dir.path())?;
        let names: Vec<&str> = workspace.tree.iter().map(|node| node.name.as_str()).collect();
        assert_eq!(names, ["nested", "a.diorama.yaml", "b.diorama.yaml"]);
        assert_eq!(workspace.spec_count(), 3);
        assert_eq!(workspace.tree[0].children[0].children[0].kind, FileKind::Spec);
        Ok(())
    }

    #[test]
    fn scan_rejects_files() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("a.diorama.yaml");
        touch(&file)?;
        assert!(matches!(scan(&file), Err(CommandError::NotAFolder(_))));
        Ok(())
    }

    #[test]
    fn save_then_read_round_trips_comments() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("a.diorama.yaml");
        let text = "# keep me\ndiorama: 1 # inline\n";
        save_spec(&file, text)?;
        assert_eq!(read_spec(&file)?, text);
        Ok(())
    }

    #[test]
    fn refuses_to_touch_non_spec_files() {
        assert!(matches!(save_spec(Path::new("/tmp/passwd"), "x"), Err(CommandError::NotASpec(_))));
        assert!(matches!(read_spec(Path::new("/etc/hosts")), Err(CommandError::NotASpec(_))));
    }
}
