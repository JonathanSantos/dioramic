//! Flat icons for the renders (T-391): the installed icon packs (`dioramic_packs::PackStore`) plus the
//! `icons/` folder next to the open spec, kept in memory and reloaded only when the files change.
//!
//! The change checks only list folders and read file metadata, so they run before every render: a pack
//! installed from a terminal or a logo dropped in `icons/` shows up at the next preview without a restart.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, SystemTime};

use dioramic_core::icons::IconRegistry;
use dioramic_desktop_ipc::{DiagnosticView, Severity};
use dioramic_packs::{LoadProblem, MANIFEST_FILE, PROJECT_ICON_DIR, PackStore, load_project_icons};

/// Code of the warning for an icon file that could not be loaded (desktop only, like `E_RENDER`).
pub const ICON_LOAD_CODE: &str = "W_ICON_LOAD";

/// How often the pack store is checked for installs and removals. Listing one folder is cheap, and a
/// poll (unlike a file system watch) also notices the store folder being created by the first install.
pub const STORE_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Names, sizes and modification times of the files that define a folder of icons: it changes when a
/// file is added, removed or rewritten.
pub type Fingerprint = Vec<(OsString, u64, Option<SystemTime>)>;

/// Icons for one render and what could not be loaded.
#[derive(Debug, Clone, Default)]
pub struct LoadedIcons {
    /// Installed packs plus the project icons.
    pub registry: IconRegistry,
    /// Files that were skipped (the rest of the registry is usable).
    pub problems: Vec<LoadProblem>,
}

/// A loaded registry and the fingerprint(s) it was loaded from.
#[derive(Debug)]
struct Snapshot<Key> {
    key: Key,
    icons: LoadedIcons,
}

/// Key of the registry for a spec folder: the folder and the fingerprints of the store and of its `icons/`.
type ProjectKey = (PathBuf, Fingerprint, Fingerprint);

/// The icon packs on disk, cached between renders.
#[derive(Debug, Default)]
pub struct IconLibrary {
    store: Option<PackStore>,
    installed: Mutex<Option<Snapshot<Fingerprint>>>,
    /// Last spec folder rendered (switching between specs of one folder reuses it).
    project: Mutex<Option<Snapshot<ProjectKey>>>,
}

impl IconLibrary {
    /// Library over the default store (`$XDG_DATA_HOME/dioramic/icons`, else `~/.local/share/dioramic/icons`).
    pub fn from_env() -> Self {
        Self::with_store(PackStore::from_env().ok())
    }

    /// Library over an explicit store (`None`: built-in and project icons only).
    pub fn with_store(store: Option<PackStore>) -> Self {
        Self { store, ..Self::default() }
    }

    /// Root of the pack store, if there is one.
    pub fn store_root(&self) -> Option<&Path> {
        self.store.as_ref().map(PackStore::root)
    }

    /// Fingerprint of the installed packs (their manifests).
    pub fn store_fingerprint(&self) -> Fingerprint {
        self.store_root().map(|root| fingerprint(root, |entry| entry.join(MANIFEST_FILE))).unwrap_or_default()
    }

    /// Icons for a render of `spec_path` (`None`: an unsaved spec, installed packs only).
    pub fn icons_for(&self, spec_path: Option<&Path>) -> LoadedIcons {
        let installed_key = self.store_fingerprint();
        let installed = self.installed(&installed_key);
        let Some(spec_dir) = spec_path.and_then(Path::parent).filter(|dir| !dir.as_os_str().is_empty()) else {
            return installed;
        };
        let project_key =
            (spec_dir.to_path_buf(), installed_key, fingerprint(&spec_dir.join(PROJECT_ICON_DIR), Path::to_path_buf));
        let mut project = self.project.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(snapshot) = project.as_ref().filter(|snapshot| snapshot.key == project_key) {
            return snapshot.icons.clone();
        }
        let mut icons = installed;
        icons.problems.extend(load_project_icons(&mut icons.registry, spec_dir));
        *project = Some(Snapshot { key: project_key, icons: icons.clone() });
        icons
    }

    fn installed(&self, key: &Fingerprint) -> LoadedIcons {
        let mut installed = self.installed.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(snapshot) = installed.as_ref().filter(|snapshot| snapshot.key == *key) {
            return snapshot.icons.clone();
        }
        let mut icons = LoadedIcons::default();
        if let Some(store) = &self.store {
            icons.problems = store.load_into(&mut icons.registry, None);
        }
        *installed = Some(Snapshot { key: key.clone(), icons: icons.clone() });
        icons
    }
}

/// Fingerprint of the entries of `dir` (hidden ones skipped), each measured through `file_of(entry)`.
/// A missing folder has an empty fingerprint.
fn fingerprint(dir: &Path, file_of: impl Fn(&Path) -> PathBuf) -> Fingerprint {
    let Ok(entries) = fs::read_dir(dir) else { return Fingerprint::new() };
    let mut parts: Fingerprint = entries
        .filter_map(Result::ok)
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .map(|entry| {
            let metadata = fs::metadata(file_of(&entry.path())).ok();
            let size = metadata.as_ref().map_or(0, fs::Metadata::len);
            (entry.file_name(), size, metadata.and_then(|metadata| metadata.modified().ok()))
        })
        .collect();
    parts.sort();
    parts
}

/// Remembers the last store fingerprint seen and tells when it changed.
#[derive(Debug, Default)]
pub struct StoreMonitor {
    last: Fingerprint,
}

impl StoreMonitor {
    /// Starts from the current state (no change reported for it).
    pub fn starting_at(current: Fingerprint) -> Self {
        Self { last: current }
    }

    /// True when `current` differs from the previous check.
    pub fn changed(&mut self, current: Fingerprint) -> bool {
        if current == self.last {
            return false;
        }
        self.last = current;
        true
    }
}

/// Polls the pack store in the background and calls `on_change` after each install or removal, so the
/// open diagram picks up a pack installed from a terminal. Runs for the life of the app.
pub fn monitor_store(
    library: std::sync::Arc<IconLibrary>,
    on_change: impl Fn() + Send + 'static,
) -> std::io::Result<thread::JoinHandle<()>> {
    thread::Builder::new().name("dioramic-icon-packs".into()).spawn(move || {
        let mut monitor = StoreMonitor::starting_at(library.store_fingerprint());
        loop {
            thread::sleep(STORE_POLL_INTERVAL);
            if monitor.changed(library.store_fingerprint()) {
                on_change();
            }
        }
    })
}

/// Load problems as warnings for the problems panel.
pub fn problem_views(problems: &[LoadProblem]) -> Vec<DiagnosticView> {
    problems
        .iter()
        .map(|problem| DiagnosticView {
            code: ICON_LOAD_CODE.to_owned(),
            severity: Severity::Warning,
            path: String::new(),
            message: format!("icon file '{}' was not loaded: {}", problem.path.display(), problem.message),
            hint: Some("Fix or remove the file; the preview reloads icons by itself.".to_owned()),
            line: None,
            column: None,
            target_id: None,
        })
        .collect()
}

/// Synthetic pack stores for the tests (never the real data folder).
#[cfg(test)]
pub(crate) mod test_packs {
    use std::fs;
    use std::path::Path;

    use dioramic_packs::MANIFEST_FILE;

    /// An AWS-like icon: opaque square background and a white glyph.
    pub(crate) const ORANGE_SQUARE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 80 80"><rect width="80" height="80" fill="#ED7100"/><path d="M10 10h20v20z" fill="#fff"/></svg>"##;

    /// Writes a one-icon `aws` pack (`aws/lambda`) into the store at `store_root`.
    pub(crate) fn install_aws(store_root: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let dir = store_root.join("aws");
        fs::create_dir_all(dir.join("svg"))?;
        fs::write(dir.join("svg/lambda.svg"), ORANGE_SQUARE)?;
        let manifest = serde_json::json!({
            "format": 1, "id": "aws", "name": "AWS", "license": "test", "source": "fixture", "version": "1",
            "icons": [{ "id": "lambda", "name": "AWS Lambda", "file": "svg/lambda.svg" }]
        });
        fs::write(dir.join(MANIFEST_FILE), serde_json::to_string(&manifest)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::test_packs::{ORANGE_SQUARE, install_aws};
    use super::*;

    #[test]
    fn installed_packs_are_loaded_and_cached() -> Result<(), Box<dyn std::error::Error>> {
        let store = tempfile::tempdir()?;
        install_aws(store.path())?;
        let library = IconLibrary::with_store(Some(PackStore::at(store.path())));
        let icons = library.icons_for(None);
        assert!(icons.problems.is_empty(), "{:?}", icons.problems);
        assert!(icons.registry.lookup("aws/lambda").is_some());
        assert_eq!(library.installed.lock().map(|cached| cached.is_some()).ok(), Some(true));
        assert!(library.icons_for(None).registry.lookup("aws/lambda").is_some());
        Ok(())
    }

    #[test]
    fn installing_or_removing_a_pack_reloads_without_restart() -> Result<(), Box<dyn std::error::Error>> {
        let store = tempfile::tempdir()?;
        let library = IconLibrary::with_store(Some(PackStore::at(store.path().join("not-yet"))));
        assert!(library.icons_for(None).registry.lookup("aws/lambda").is_none());
        install_aws(&store.path().join("not-yet"))?;
        assert!(library.icons_for(None).registry.lookup("aws/lambda").is_some());
        fs::remove_dir_all(store.path().join("not-yet/aws"))?;
        assert!(library.icons_for(None).registry.lookup("aws/lambda").is_none());
        Ok(())
    }

    #[test]
    fn project_icons_join_the_installed_packs() -> Result<(), Box<dyn std::error::Error>> {
        let store = tempfile::tempdir()?;
        install_aws(store.path())?;
        let project = tempfile::tempdir()?;
        let spec = project.path().join("app.diorama.yaml");
        let library = IconLibrary::with_store(Some(PackStore::at(store.path())));
        assert!(library.icons_for(Some(&spec)).registry.lookup("local/payments").is_none());

        fs::create_dir_all(project.path().join(PROJECT_ICON_DIR))?;
        fs::write(project.path().join("icons/Payments.svg"), ORANGE_SQUARE)?;
        let icons = library.icons_for(Some(&spec));
        assert!(icons.registry.lookup("local/payments").is_some());
        assert!(icons.registry.lookup("aws/lambda").is_some());

        fs::remove_file(project.path().join("icons/Payments.svg"))?;
        assert!(library.icons_for(Some(&spec)).registry.lookup("local/payments").is_none());
        Ok(())
    }

    #[test]
    fn broken_icons_become_problems_not_failures() -> Result<(), Box<dyn std::error::Error>> {
        let project = tempfile::tempdir()?;
        fs::create_dir_all(project.path().join(PROJECT_ICON_DIR))?;
        fs::write(project.path().join("icons/broken.svg"), "<not svg")?;
        fs::write(project.path().join("icons/ok.svg"), ORANGE_SQUARE)?;
        let library = IconLibrary::with_store(None);
        let icons = library.icons_for(Some(&project.path().join("app.diorama.yaml")));
        assert!(icons.registry.lookup("local/ok").is_some());
        assert_eq!(icons.problems.len(), 1);
        let views = problem_views(&icons.problems);
        assert_eq!((views[0].code.as_str(), views[0].severity), (ICON_LOAD_CODE, Severity::Warning));
        assert!(views[0].message.contains("broken.svg"));
        Ok(())
    }

    #[test]
    fn no_store_means_built_in_icons_only() {
        let library = IconLibrary::with_store(None);
        assert!(library.store_root().is_none());
        assert!(library.store_fingerprint().is_empty());
        assert!(library.icons_for(Some(Path::new("relative.diorama.yaml"))).registry.is_empty());
    }

    #[test]
    fn monitor_reports_each_change_once() -> Result<(), Box<dyn std::error::Error>> {
        let store = tempfile::tempdir()?;
        let library = IconLibrary::with_store(Some(PackStore::at(store.path())));
        let mut monitor = StoreMonitor::starting_at(library.store_fingerprint());
        assert!(!monitor.changed(library.store_fingerprint()));
        install_aws(store.path())?;
        assert!(monitor.changed(library.store_fingerprint()));
        assert!(!monitor.changed(library.store_fingerprint()));
        Ok(())
    }
}
