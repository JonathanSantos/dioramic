//! Reading SVGs out of a provider archive (zip) or an extracted directory, without ever writing the
//! archive's paths to disk (no zip-slip: installed files are named by their normalized id).

use std::fs;
use std::io::Read;
use std::path::Path;

use dioramic_core::icons::flat::MAX_SVG_BYTES;

use crate::error::PackError;
use crate::normalize::SourceFile;

/// Most entries read from one archive or directory tree (the AWS package has ~8 000).
pub const MAX_FILES: usize = 50_000;

/// Reads every SVG of a zip archive; nested `.zip` files are not opened.
pub fn read_zip(path: &Path) -> Result<Vec<SourceFile>, PackError> {
    let file = fs::File::open(path).map_err(|source| PackError::io(path, source))?;
    let zip_error =
        |error: zip::result::ZipError| PackError::Zip { path: path.to_path_buf(), message: error.to_string() };
    let mut archive = zip::ZipArchive::new(file).map_err(zip_error)?;
    if archive.len() > MAX_FILES {
        return Err(PackError::TooManyFiles { origin: path.display().to_string(), limit: MAX_FILES });
    }
    let mut files = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(zip_error)?;
        let name = entry.name().map_err(zip_error)?.replace('\\', "/");
        if entry.is_dir() || !is_svg_name(&name) || entry.size() > MAX_SVG_BYTES as u64 {
            continue;
        }
        let mut text = String::new();
        // Read through `take` so a lying size header cannot make us buffer more than the limit.
        let read = (&mut entry).take(MAX_SVG_BYTES as u64 + 1).read_to_string(&mut text);
        if read.is_ok() && text.len() <= MAX_SVG_BYTES {
            files.push(SourceFile { path: name, svg: text });
        }
    }
    Ok(files)
}

/// Reads every SVG under `root` (recursively, symlinks not followed) and every SVG inside the `.zip`
/// archives found there (a folder with the three Google Cloud downloads works).
pub fn read_dir(root: &Path) -> Result<Vec<SourceFile>, PackError> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    let mut seen = 0usize;
    while let Some(directory) = pending.pop() {
        let mut entries: Vec<_> = fs::read_dir(&directory)
            .map_err(|source| PackError::io(&directory, source))?
            .filter_map(Result::ok)
            .collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            seen += 1;
            if seen > MAX_FILES {
                return Err(PackError::TooManyFiles { origin: root.display().to_string(), limit: MAX_FILES });
            }
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() {
                let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                if name.ends_with(".zip") {
                    files.extend(read_zip(&path)?);
                } else if is_svg_name(&name) {
                    files.extend(read_svg_file(root, &path));
                }
            }
        }
    }
    Ok(files)
}

fn read_svg_file(root: &Path, path: &Path) -> Option<SourceFile> {
    let metadata = fs::metadata(path).ok()?;
    if metadata.len() > MAX_SVG_BYTES as u64 {
        return None;
    }
    let svg = fs::read_to_string(path).ok()?;
    let relative = path.strip_prefix(root).unwrap_or(path);
    let path = relative.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
    Some(SourceFile { path, svg })
}

fn is_svg_name(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".svg")
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;

    use super::*;

    pub(crate) fn write_zip(path: &Path, entries: &[(&str, &str)]) {
        let file = fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, content) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(content.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn reads_svgs_from_zips_and_directories() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("icons.zip");
        write_zip(&archive, &[("a/b.svg", "<svg/>"), ("a/c.png", "png"), ("../../evil.svg", "<svg/>")]);
        let from_zip = read_zip(&archive).unwrap();
        let names: Vec<&str> = from_zip.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(names, ["a/b.svg", "../../evil.svg"], "names are data, never written as paths");

        fs::create_dir_all(dir.path().join("x/y")).unwrap();
        fs::write(dir.path().join("x/y/z.SVG"), "<svg/>").unwrap();
        fs::write(dir.path().join("x/big.svg"), " ".repeat(MAX_SVG_BYTES + 1)).unwrap();
        let from_dir = read_dir(dir.path()).unwrap();
        let mut names: Vec<&str> = from_dir.iter().map(|file| file.path.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["../../evil.svg", "a/b.svg", "x/y/z.SVG"]);
    }

    #[test]
    fn broken_archives_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("broken.zip");
        fs::write(&archive, "not a zip").unwrap();
        assert!(matches!(read_zip(&archive), Err(PackError::Zip { .. })));
        assert!(matches!(read_zip(&dir.path().join("missing.zip")), Err(PackError::Io { .. })));
    }
}
