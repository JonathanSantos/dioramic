//! Icon packs on disk for Dioramic (ADR-006 layer 3, ADR-007).
//!
//! - [`PackStore`]: `$XDG_DATA_HOME/dioramic/icons/<pack>/` (default `~/.local/share/dioramic/icons`, also on
//!   macOS) with a [`Manifest`] and the provider SVGs; install, list, remove, load into an
//!   [`IconRegistry`](dioramic_core::icons::IconRegistry).
//! - [`Provider`]: AWS, Google Cloud, Azure and Kubernetes — official archive locations and terms. Downloading
//!   is the caller's job (the CLI uses `curl`); this crate reads the archives ([`sources`]) and normalizes
//!   provider file names into icon ids ([`normalize`]).
//! - [`load_project_icons`]: the project's `icons/` directory (`local/<name>`).
//!
//! The core stays I/O-free: everything here ends as SVG text handed to the registry.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod error;
pub mod local;
pub mod manifest;
pub mod normalize;
pub mod provider;
pub mod sources;
pub mod store;

pub use error::PackError;
pub use local::{PROJECT_ICON_DIR, load_project_icons};
pub use manifest::{MANIFEST_FILE, Manifest, ManifestIcon};
pub use provider::{Download, Provider, find_archive_link};
pub use store::{InstallSummary, InstalledPack, LoadProblem, Origin, PackStore, RegistryCache};

use dioramic_core::icons::IconRegistry;

/// Pack ids referenced as `<pack>/` in a spec text (`icon: aws/lambda` → `aws`). Used to load only the packs
/// a render needs; a false positive only costs loading a pack.
pub fn referenced_packs<'a>(spec_text: &str, candidates: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    candidates.into_iter().filter(|pack| spec_text.contains(&format!("{pack}/"))).collect()
}

/// Registry for one spec: the installed packs it references plus the project icons next to it.
pub fn registry_for_spec(
    store: Option<&PackStore>,
    spec_text: &str,
    spec_dir: Option<&std::path::Path>,
) -> (IconRegistry, Vec<LoadProblem>) {
    let mut registry = IconRegistry::new();
    let mut problems = Vec::new();
    if let Some(store) = store {
        let installed = store.installed();
        let ids: Vec<&str> = installed.iter().map(|pack| pack.manifest.id.as_str()).collect();
        let wanted = referenced_packs(spec_text, ids);
        if !wanted.is_empty() {
            problems.extend(store.load_into(&mut registry, Some(&wanted)));
        }
    }
    if let Some(dir) = spec_dir {
        problems.extend(load_project_icons(&mut registry, dir));
    }
    (registry, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_referenced_packs() {
        let spec = "nodes:\n  - { id: fn, icon: aws/lambda }\n  - { id: db, icon: postgres }\n";
        assert_eq!(referenced_packs(spec, ["aws", "gcp", "k8s"]), ["aws"]);
    }
}
