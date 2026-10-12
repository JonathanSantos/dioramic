//! Project icons: an `icons/` directory next to the spec. Every `icons/*.svg` becomes `local/<name>`
//! (`icons/Payments Logo.svg` → `local/payments-logo`); an optional `icons/manifest.json` (same format as a
//! pack manifest) sets names, aliases and `mount: stand`. Project icons override installed packs with the
//! same id, and need no install step, so a repository can carry its own logos.

use std::fs;
use std::path::Path;

use dioramic_core::icons::IconRegistry;
use dioramic_core::icons::flat::LOCAL_PACK;

use crate::manifest::{MANIFEST_FILE, MANIFEST_FORMAT, Manifest, ManifestIcon};
use crate::normalize::slug;
use crate::store::{LoadProblem, load_pack};

/// Name of the project icon directory, next to the spec.
pub const PROJECT_ICON_DIR: &str = "icons";

/// Adds the icons of `<spec_dir>/icons/` to `registry` (nothing when the directory does not exist).
pub fn load_project_icons(registry: &mut IconRegistry, spec_dir: &Path) -> Vec<LoadProblem> {
    let dir = spec_dir.join(PROJECT_ICON_DIR);
    if !dir.is_dir() {
        return Vec::new();
    }
    let manifest_path = dir.join(MANIFEST_FILE);
    let mut problems = Vec::new();
    let mut manifest = if manifest_path.is_file() {
        match Manifest::read(&manifest_path) {
            Ok(manifest) => manifest,
            Err(error) => {
                problems.push(LoadProblem { path: manifest_path, message: error.to_string() });
                empty_manifest(&dir)
            }
        }
    } else {
        empty_manifest(&dir)
    };
    add_unlisted_svgs(&mut manifest, &dir);
    problems.extend(load_pack(registry, &dir, &manifest, Some(LOCAL_PACK)));
    problems
}

fn empty_manifest(dir: &Path) -> Manifest {
    Manifest {
        format: MANIFEST_FORMAT,
        id: LOCAL_PACK.to_owned(),
        name: "Project icons".to_owned(),
        license: "project".to_owned(),
        source: dir.display().to_string(),
        version: String::new(),
        icons: Vec::new(),
    }
}

/// SVGs of the directory that the manifest does not list, with ids from their file names.
fn add_unlisted_svgs(manifest: &mut Manifest, dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.to_ascii_lowercase().ends_with(".svg"))
        .collect();
    names.sort();
    for name in names {
        let listed = manifest.icons.iter().any(|icon| icon.file == name);
        let id = slug(&name[..name.len() - ".svg".len()]);
        if listed || id.is_empty() || manifest.icons.iter().any(|icon| icon.id == id) {
            continue;
        }
        manifest.icons.push(ManifestIcon {
            id: id.clone(),
            name: id.replace('-', " "),
            file: name,
            aliases: Vec::new(),
            category: None,
            mount: Default::default(),
        });
    }
}

#[cfg(test)]
mod tests {
    use dioramic_core::icons::{IconKind, Mount};

    use super::*;

    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5" cy="5" r="4" fill="#635bff"/></svg>"##;

    #[test]
    fn loads_svgs_and_the_optional_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let icons = dir.path().join(PROJECT_ICON_DIR);
        fs::create_dir_all(&icons).unwrap();
        fs::write(icons.join("Payments Logo.svg"), SVG).unwrap();
        fs::write(icons.join("badge.svg"), SVG).unwrap();
        fs::write(icons.join("notes.txt"), "x").unwrap();
        fs::write(
            icons.join(MANIFEST_FILE),
            r#"{"id":"anything","name":"Mine","icons":[{"id":"stripe","file":"badge.svg","mount":"stand","aliases":["pay"]}]}"#,
        )
        .unwrap();
        let mut registry = IconRegistry::new();
        let problems = load_project_icons(&mut registry, dir.path());
        assert!(problems.is_empty(), "{problems:?}");
        assert!(registry.has_pack(LOCAL_PACK));
        assert!(registry.lookup("local/payments-logo").is_some());
        let stripe = registry.lookup("local/pay").unwrap();
        assert_eq!(stripe.id, "local/stripe");
        assert!(matches!(stripe.kind, IconKind::Flat(flat) if flat.mount == Mount::Stand));
        assert!(registry.lookup("local/badge").is_none(), "listed files keep their manifest id");
    }

    #[test]
    fn missing_directory_adds_nothing_and_bad_files_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = IconRegistry::new();
        assert!(load_project_icons(&mut registry, dir.path()).is_empty());
        assert!(!registry.has_pack(LOCAL_PACK));
        let icons = dir.path().join(PROJECT_ICON_DIR);
        fs::create_dir_all(&icons).unwrap();
        fs::write(icons.join("broken.svg"), "<svg").unwrap();
        fs::write(icons.join(MANIFEST_FILE), "{").unwrap();
        let problems = load_project_icons(&mut registry, dir.path());
        assert_eq!(problems.len(), 2);
    }
}
