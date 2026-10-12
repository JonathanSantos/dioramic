//! The pack store on disk: `$XDG_DATA_HOME/dioramic/icons/<pack>/{manifest.json, svg/*.svg}`
//! (`~/.local/share/dioramic/icons` when `XDG_DATA_HOME` is not set, on Linux **and** macOS;
//! `%LOCALAPPDATA%\dioramic\icons` on Windows).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

use dioramic_core::icons::IconRegistry;
use dioramic_core::icons::flat::{FlatIconSource, sanitize_svg};

use crate::error::PackError;
use crate::manifest::{MANIFEST_FILE, MANIFEST_FORMAT, Manifest, ManifestIcon};
use crate::normalize::{SourceFile, normalize};
use crate::provider::Provider;

/// Directory of the SVGs inside a pack.
const SVG_DIR: &str = "svg";

/// The store root and its operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackStore {
    root: PathBuf,
}

/// A pack found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPack {
    /// Pack directory.
    pub dir: PathBuf,
    /// Its manifest.
    pub manifest: Manifest,
}

/// Something that could not be loaded; the rest of the registry is still usable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadProblem {
    /// File involved.
    pub path: PathBuf,
    /// What happened.
    pub message: String,
}

/// Where the files of an install came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    /// URL(s) or path(s), shown in `icons list --packs`.
    pub source: String,
    /// Provider release, when known.
    pub version: Option<String>,
}

/// Result of an install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallSummary {
    /// Pack directory.
    pub dir: PathBuf,
    /// Manifest written.
    pub manifest: Manifest,
    /// Matching files that were not valid SVGs.
    pub skipped: usize,
}

impl PackStore {
    /// Store at an explicit root (tests, `--store`).
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Store at the default root for this environment (see the module docs).
    pub fn from_env() -> Result<Self, PackError> {
        default_root(&|name| std::env::var_os(name).map(PathBuf::from)).map(Self::at).ok_or(PackError::NoDataDir)
    }

    /// Root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Directory of a pack.
    pub fn pack_dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    /// Installed packs, sorted by id. A missing root means no pack; unreadable manifests are skipped.
    pub fn installed(&self) -> Vec<InstalledPack> {
        let Ok(entries) = fs::read_dir(&self.root) else { return Vec::new() };
        let mut packs: Vec<InstalledPack> = entries
            .filter_map(Result::ok)
            .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
            .filter_map(|entry| {
                let dir = entry.path();
                Manifest::read(&dir.join(MANIFEST_FILE)).ok().map(|manifest| InstalledPack { dir, manifest })
            })
            .collect();
        packs.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
        packs
    }

    /// Loads the installed packs (only `only` when given) into `registry`.
    pub fn load_into(&self, registry: &mut IconRegistry, only: Option<&[&str]>) -> Vec<LoadProblem> {
        let mut problems = Vec::new();
        for pack in self.installed() {
            if only.is_some_and(|wanted| !wanted.iter().any(|id| id.eq_ignore_ascii_case(&pack.manifest.id))) {
                continue;
            }
            problems.extend(load_pack(registry, &pack.dir, &pack.manifest, None));
        }
        problems
    }

    /// Fingerprint of the store (pack ids and manifest times): changes when a pack is installed or removed.
    pub fn fingerprint(&self) -> Fingerprint {
        self.installed()
            .into_iter()
            .map(|pack| {
                let modified = fs::metadata(pack.dir.join(MANIFEST_FILE)).and_then(|meta| meta.modified()).ok();
                (pack.manifest.id, modified)
            })
            .collect()
    }

    /// Normalizes `files` with the provider's rules and writes the pack, replacing an installed one.
    pub fn install(
        &self,
        provider: Provider,
        files: &[SourceFile],
        origin: &Origin,
    ) -> Result<InstallSummary, PackError> {
        let icons = normalize(provider, files);
        let mut valid = Vec::with_capacity(icons.len());
        let mut skipped = 0;
        for icon in icons {
            if sanitize_svg(&files[icon.file].svg).is_ok() {
                valid.push(icon);
            } else {
                skipped += 1;
            }
        }
        if valid.is_empty() {
            return Err(PackError::NoIcons { provider: provider.id().to_owned(), origin: origin.source.clone() });
        }
        let manifest = Manifest {
            format: MANIFEST_FORMAT,
            id: provider.id().to_owned(),
            name: provider.name().to_owned(),
            license: provider.license().to_owned(),
            source: origin.source.clone(),
            version: origin.version.clone().unwrap_or_else(|| "unknown".to_owned()),
            icons: valid
                .iter()
                .map(|icon| ManifestIcon {
                    id: icon.id.clone(),
                    name: icon.name.clone(),
                    file: format!("{SVG_DIR}/{}.svg", icon.id),
                    aliases: icon.aliases.clone(),
                    category: icon.group.clone(),
                    mount: icon.mount,
                })
                .collect(),
        };
        let staging = self.root.join(format!(".{}.partial", provider.id()));
        remove_dir_if_present(&staging)?;
        let svg_dir = staging.join(SVG_DIR);
        fs::create_dir_all(&svg_dir).map_err(|source| PackError::io(&svg_dir, source))?;
        for icon in &valid {
            let path = svg_dir.join(format!("{}.svg", icon.id));
            fs::write(&path, &files[icon.file].svg).map_err(|source| PackError::io(&path, source))?;
        }
        let manifest_path = staging.join(MANIFEST_FILE);
        fs::write(&manifest_path, manifest.to_json()).map_err(|source| PackError::io(&manifest_path, source))?;
        let dir = self.pack_dir(provider.id());
        self.swap_in(&staging, &dir)?;
        Ok(InstallSummary { dir, manifest, skipped })
    }

    /// Replaces `dir` with `staging` (the old pack is removed only after the new one is in place).
    fn swap_in(&self, staging: &Path, dir: &Path) -> Result<(), PackError> {
        let retired =
            self.root.join(format!(".{}.old", dir.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()));
        remove_dir_if_present(&retired)?;
        if dir.exists() {
            fs::rename(dir, &retired).map_err(|source| PackError::io(dir, source))?;
        }
        fs::rename(staging, dir).map_err(|source| PackError::io(dir, source))?;
        remove_dir_if_present(&retired)
    }

    /// Deletes an installed pack.
    pub fn remove(&self, id: &str) -> Result<InstalledPack, PackError> {
        let pack = self
            .installed()
            .into_iter()
            .find(|pack| pack.manifest.id.eq_ignore_ascii_case(id))
            .ok_or_else(|| PackError::NotInstalled(id.to_owned()))?;
        fs::remove_dir_all(&pack.dir).map_err(|source| PackError::io(&pack.dir, source))?;
        Ok(pack)
    }
}

fn remove_dir_if_present(dir: &Path) -> Result<(), PackError> {
    match fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(PackError::io(dir, source)),
    }
}

/// `$XDG_DATA_HOME/dioramic/icons`, else `$HOME/.local/share/dioramic/icons`, else
/// `%LOCALAPPDATA%\dioramic\icons`. `XDG_DATA_HOME` must be absolute (XDG spec).
pub fn default_root(env: &dyn Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    let base = env("XDG_DATA_HOME")
        .filter(|path| path.is_absolute())
        .or_else(|| env("HOME").filter(|path| !path.as_os_str().is_empty()).map(|home| home.join(".local/share")))
        .or_else(|| env("LOCALAPPDATA"))?;
    Some(base.join("dioramic").join("icons"))
}

/// Registers every icon of a manifest found in `dir`. `pack_id` overrides the manifest id (project icons
/// are always `local`).
pub(crate) fn load_pack(
    registry: &mut IconRegistry,
    dir: &Path,
    manifest: &Manifest,
    pack_id: Option<&str>,
) -> Vec<LoadProblem> {
    let mut info = manifest.pack_info();
    if let Some(id) = pack_id {
        info.id = id.to_owned();
    }
    let mut problems = Vec::new();
    for icon in &manifest.icons {
        let path = dir.join(&icon.file);
        let inside = path.components().all(|part| !matches!(part, std::path::Component::ParentDir));
        let svg = match fs::read_to_string(&path) {
            Ok(svg) if inside => svg,
            Ok(_) => {
                problems.push(LoadProblem { path, message: "icon file outside the pack".to_owned() });
                continue;
            }
            Err(error) => {
                problems.push(LoadProblem { path, message: error.to_string() });
                continue;
            }
        };
        let source = FlatIconSource {
            pack: info.id.clone(),
            id: icon.id.clone(),
            name: if icon.name.is_empty() { icon.id.clone() } else { icon.name.clone() },
            aliases: icon.aliases.clone(),
            group: icon.category.clone(),
            mount: icon.mount,
            svg,
        };
        if let Err(error) = registry.add_icon(source) {
            problems.push(LoadProblem { path, message: error.to_string() });
        }
    }
    registry.add_pack(info);
    problems
}

/// Pack ids and manifest modification times of a store (see [`PackStore::fingerprint`]).
pub type Fingerprint = Vec<(String, Option<SystemTime>)>;

/// Installed packs, reloaded only when the store changes (MCP server, desktop app).
#[derive(Debug, Default)]
pub struct RegistryCache {
    store: Option<PackStore>,
    cached: Mutex<Option<(Fingerprint, IconRegistry)>>,
}

impl RegistryCache {
    /// Cache over a store (`None`: no data directory, built-in icons only).
    pub fn new(store: Option<PackStore>) -> Self {
        Self { store, cached: Mutex::new(None) }
    }

    /// The registry of the installed packs (problems are logged by the caller via [`RegistryCache::load`]).
    pub fn registry(&self) -> IconRegistry {
        self.load().0
    }

    /// The registry and the problems of the last (re)load.
    pub fn load(&self) -> (IconRegistry, Vec<LoadProblem>) {
        let Some(store) = &self.store else { return (IconRegistry::new(), Vec::new()) };
        let fingerprint = store.fingerprint();
        let mut cached = self.cached.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((known, registry)) = cached.as_ref()
            && *known == fingerprint
        {
            return (registry.clone(), Vec::new());
        }
        let mut registry = IconRegistry::new();
        let problems = store.load_into(&mut registry, None);
        *cached = Some((fingerprint, registry.clone()));
        (registry, problems)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize::SourceFile;

    const SQUARE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 80 80"><rect width="80" height="80" fill="#ED7100"/><path d="M10 10h20v20z" fill="#fff"/></svg>"##;

    fn aws_files() -> Vec<SourceFile> {
        vec![
            SourceFile { path: "Arch_Compute/64/Arch_AWS-Lambda_64.svg".to_owned(), svg: SQUARE.to_owned() },
            SourceFile {
                path: "Arch_Storage/64/Arch_Amazon-Simple-Storage-Service_64.svg".to_owned(),
                svg: SQUARE.replace("ED7100", "7AA116"),
            },
            SourceFile { path: "Arch_Storage/64/Arch_Broken_64.svg".to_owned(), svg: "<nope".to_owned() },
        ]
    }

    fn origin() -> Origin {
        Origin { source: "fixture.zip".to_owned(), version: Some("2026-07-31".to_owned()) }
    }

    #[test]
    fn default_root_follows_xdg_then_home() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| pairs.iter().find(|(key, _)| *key == name).map(|(_, value)| PathBuf::from(value))
        };
        assert_eq!(
            default_root(&env(&[("XDG_DATA_HOME", "/data"), ("HOME", "/home/me")])),
            Some(PathBuf::from("/data/dioramic/icons"))
        );
        assert_eq!(
            default_root(&env(&[("XDG_DATA_HOME", "relative"), ("HOME", "/home/me")])),
            Some(PathBuf::from("/home/me/.local/share/dioramic/icons"))
        );
        assert_eq!(default_root(&env(&[])), None);
    }

    #[test]
    fn installs_lists_loads_and_removes() {
        let dir = tempfile::tempdir().unwrap();
        let store = PackStore::at(dir.path().join("icons"));
        assert!(store.installed().is_empty());
        let summary = store.install(Provider::Aws, &aws_files(), &origin()).unwrap();
        assert_eq!(summary.skipped, 1);
        assert_eq!(summary.manifest.icons.len(), 2);
        assert!(summary.dir.join("svg/lambda.svg").is_file());
        assert_eq!(summary.manifest.version, "2026-07-31");

        // Reinstall replaces atomically, leaving no staging directories behind.
        store.install(Provider::Aws, &aws_files()[..1], &origin()).unwrap();
        let names: Vec<String> = fs::read_dir(store.root())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["aws"]);
        assert_eq!(store.installed()[0].manifest.icons.len(), 1);

        let mut registry = IconRegistry::new();
        assert!(store.load_into(&mut registry, None).is_empty());
        assert!(registry.has_pack("aws"));
        assert_eq!(registry.lookup("aws/function").map(|entry| entry.id), Some("aws/lambda"));
        let mut filtered = IconRegistry::new();
        store.load_into(&mut filtered, Some(&["gcp"]));
        assert!(filtered.is_empty());

        assert_eq!(store.remove("AWS").unwrap().manifest.id, "aws");
        assert!(matches!(store.remove("aws"), Err(PackError::NotInstalled(_))));
    }

    #[test]
    fn empty_sources_are_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = PackStore::at(dir.path());
        let error = store.install(Provider::Gcp, &aws_files()[2..], &origin()).unwrap_err();
        assert!(matches!(error, PackError::NoIcons { .. }), "{error}");
    }

    #[test]
    fn cache_reloads_when_the_store_changes() {
        let dir = tempfile::tempdir().unwrap();
        let store = PackStore::at(dir.path());
        let cache = RegistryCache::new(Some(store.clone()));
        assert!(cache.registry().is_empty());
        store.install(Provider::Aws, &aws_files(), &origin()).unwrap();
        assert!(cache.registry().has_pack("aws"));
        store.remove("aws").unwrap();
        assert!(!cache.registry().has_pack("aws"));
        assert!(RegistryCache::new(None).registry().is_empty());
    }

    #[test]
    fn broken_icon_files_are_reported_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let store = PackStore::at(dir.path());
        let summary = store.install(Provider::Aws, &aws_files(), &origin()).unwrap();
        fs::write(summary.dir.join("svg/lambda.svg"), "garbage").unwrap();
        let mut registry = IconRegistry::new();
        let problems = store.load_into(&mut registry, None);
        assert_eq!(problems.len(), 1);
        assert!(registry.lookup("aws/simple-storage-service").is_some());
    }
}
