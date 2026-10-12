//! `dioramic icons install <pack> [--from …]`: official download (with `curl`) or a local copy → pack store.

use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use dioramic_packs::normalize::SourceFile;
use dioramic_packs::{Download, Origin, PackStore, Provider, find_archive_link, sources};

use super::store;
use crate::ui::{self, BOLD, DIM, GREEN, UserError, paint};

/// Longest a single download may take (the Kubernetes repository archive is ~45 MB).
const DOWNLOAD_TIMEOUT_SECONDS: &str = "900";
/// Scratch directory for downloads, inside the store root (same filesystem, removed afterwards).
const DOWNLOAD_DIR: &str = ".downloads";

/// Installs `provider` from `from` (local archives, directories or URLs) or from the official download.
pub fn run(provider: &Provider, from: &[String]) -> Result<()> {
    let store = store()?;
    print_terms(*provider);
    let download_dir = store.root().join(DOWNLOAD_DIR);
    let result = collect_sources(*provider, from, &download_dir).and_then(|(files, origin)| {
        anstream::eprintln!("{}", paint(DIM, format!("normalizing {} SVG files…", files.len())));
        install(&store, *provider, &files, &origin)
    });
    // Downloads are scratch data; a failure to clean them is not worth failing the install.
    let _ = fs::remove_dir_all(&download_dir);
    result
}

fn print_terms(provider: Provider) {
    anstream::eprintln!("{} {}", paint(BOLD, provider.name()), paint(DIM, provider.homepage()));
    anstream::eprintln!("{}", paint(DIM, format!("terms: {}", provider.license())));
}

fn install(store: &PackStore, provider: Provider, files: &[SourceFile], origin: &Origin) -> Result<()> {
    let summary = store.install(provider, files, origin).map_err(|error| {
        UserError::new(error.to_string())
            .hint(format!("pass the official archive from {} with --from <file.zip>", provider.homepage()))
    })?;
    let manifest = &summary.manifest;
    anstream::println!(
        "{} {} {}  {}",
        paint(GREEN, "✓"),
        paint(BOLD, &manifest.id),
        ui::plural(manifest.icons.len(), "icon"),
        paint(DIM, format!("version {} → {}", manifest.version, summary.dir.display()))
    );
    if summary.skipped > 0 {
        ui::print_warning(&format!("{} files were not valid SVG icons and were skipped", summary.skipped));
    }
    let example = manifest
        .icons
        .iter()
        .find(|icon| icon.id == provider.example_icon())
        .or(manifest.icons.first())
        .map_or_else(String::new, |icon| format!("{}/{}", manifest.id, icon.id));
    ui::print_hint(&format!(
        "use them as `icon: {example}`; find ids with `dioramic icons search <name> --pack {}`",
        manifest.id
    ));
    Ok(())
}

/// Reads every `--from`, or downloads the official archives when there is none.
fn collect_sources(provider: Provider, from: &[String], download_dir: &Path) -> Result<(Vec<SourceFile>, Origin)> {
    let locations: Vec<String> = if from.is_empty() { official_archives(provider)? } else { from.to_vec() };
    let mut files = Vec::new();
    let mut names = Vec::new();
    for (index, location) in locations.iter().enumerate() {
        let path = if is_url(location) {
            download(location, &download_dir.join(format!("{index}.zip")), provider)?
        } else {
            PathBuf::from(location)
        };
        names.push(location.rsplit('/').next().unwrap_or(location).to_owned());
        files.extend(read_location(&path, provider)?);
    }
    // Extracted archives keep the release in their folder names (`Architecture-Service-Icons_07312026/`).
    names.extend(files.iter().take(1).map(|file| file.path.clone()));
    let origin = Origin {
        source: locations.join(", "),
        version: provider.version_from(&names).or_else(|| default_version(provider, from)),
    };
    Ok((files, origin))
}

/// The Kubernetes archive is the `main` branch of kubernetes/community.
fn default_version(provider: Provider, from: &[String]) -> Option<String> {
    (provider == Provider::Kubernetes && from.is_empty()).then(|| "main".to_owned())
}

fn read_location(path: &Path, provider: Provider) -> Result<Vec<SourceFile>> {
    let metadata = fs::metadata(path).map_err(|error| {
        UserError::new(format!("cannot read {}: {error}", path.display()))
            .hint(format!("download the icons from {} and pass the zip or folder", provider.homepage()))
    })?;
    let files = if metadata.is_dir() { sources::read_dir(path) } else { sources::read_zip(path) };
    files.map_err(|error| UserError::new(error.to_string()).into())
}

fn is_url(location: &str) -> bool {
    location.starts_with("https://") || location.starts_with("http://")
}

/// URLs of the provider's current archives (pages are read for the link; the last known link is the fallback).
fn official_archives(provider: Provider) -> Result<Vec<String>> {
    match provider.download() {
        Download::Archives(urls) => Ok(urls.iter().map(|url| (*url).to_owned()).collect()),
        Download::LinkedFrom { page, marker, fallback } => {
            anstream::eprintln!("{}", paint(DIM, format!("looking up the current archive on {page}")));
            let link = fetch_text(page).ok().and_then(|html| find_archive_link(&html, marker));
            if link.is_none() {
                ui::print_warning(&format!("could not find the archive link on {page}; using {fallback}"));
            }
            Ok(vec![link.unwrap_or_else(|| fallback.to_owned())])
        }
    }
}

fn curl() -> Command {
    let mut command = Command::new("curl");
    command.args([
        "--fail",
        "--location",
        "--silent",
        "--show-error",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--max-time",
        DOWNLOAD_TIMEOUT_SECONDS,
    ]);
    command
}

fn fetch_text(url: &str) -> Result<String> {
    let output = curl().arg(url).output().context("cannot run curl")?;
    if !output.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn download(url: &str, target: &Path, provider: Provider) -> Result<PathBuf> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).with_context(|| format!("cannot create {}", parent.display()))?;
    }
    anstream::eprintln!("{} {url}", paint(DIM, "downloading"));
    let mut command = curl();
    if std::io::stderr().is_terminal() {
        command.arg("--progress-bar");
    }
    let status = command.arg("--output").arg(target).arg(url).status();
    let manual_hint = format!(
        "download it yourself from {} and run `dioramic icons install {provider} --from <file.zip>`",
        provider.homepage()
    );
    match status {
        Ok(status) if status.success() => Ok(target.to_path_buf()),
        Ok(_) => Err(UserError::new(format!("download failed: {url}")).hint(manual_hint).into()),
        Err(error) => Err(UserError::new(format!("cannot run curl ({error})")).hint(manual_hint).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_recognized() {
        assert!(is_url("https://example.com/a.zip"));
        assert!(!is_url("/tmp/a.zip"));
        assert!(!is_url("icons"));
    }

    #[test]
    fn missing_local_sources_explain_where_to_download() {
        let error = read_location(Path::new("/definitely/missing.zip"), Provider::Azure).unwrap_err();
        let user_error = error.downcast_ref::<UserError>().map(|error| error.hint.clone().unwrap_or_default());
        assert!(user_error.is_some_and(|hint| hint.contains("learn.microsoft.com")));
    }
}
