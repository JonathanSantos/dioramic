//! `manifest.json` of a pack: what the pack is, where it came from and which icons it has.

use std::path::Path;

use dioramic_core::icons::{IconPackInfo, Mount};
use serde::{Deserialize, Serialize};

use crate::error::PackError;

/// Current manifest format.
pub const MANIFEST_FORMAT: u32 = 1;
/// File name of the manifest inside a pack directory.
pub const MANIFEST_FILE: &str = "manifest.json";

/// A pack manifest.
///
/// ```json
/// { "format": 1, "id": "aws", "name": "AWS Architecture Icons", "license": "…", "source": "https://…zip",
///   "version": "2026-07-31",
///   "icons": [{ "id": "lambda", "name": "AWS Lambda", "file": "svg/lambda.svg",
///               "aliases": ["aws-lambda", "function"], "category": "Compute", "mount": "top" }] }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Format version ([`MANIFEST_FORMAT`]).
    #[serde(default = "default_format")]
    pub format: u32,
    /// Pack id, also the icon prefix (`aws` → `aws/lambda`).
    pub id: String,
    /// Display name.
    pub name: String,
    /// License or terms of use.
    #[serde(default)]
    pub license: String,
    /// Origin (URL or path).
    #[serde(default)]
    pub source: String,
    /// Provider release.
    #[serde(default)]
    pub version: String,
    /// Icons, sorted by id.
    #[serde(default)]
    pub icons: Vec<ManifestIcon>,
}

/// One icon of a manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestIcon {
    /// Id inside the pack (`lambda`).
    pub id: String,
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// SVG path relative to the pack directory.
    pub file: String,
    /// Other ids inside the pack.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Provider category (`Compute`, `Databases`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// How the icon sits on its base.
    #[serde(default)]
    pub mount: Mount,
}

fn default_format() -> u32 {
    MANIFEST_FORMAT
}

impl Manifest {
    /// Reads and parses `path`.
    pub fn read(path: &Path) -> Result<Self, PackError> {
        let text = std::fs::read_to_string(path).map_err(|source| PackError::io(path, source))?;
        Self::parse(&text).map_err(|message| PackError::Manifest { path: path.to_path_buf(), message })
    }

    /// Parses manifest JSON; the message says what is wrong.
    pub fn parse(text: &str) -> Result<Self, String> {
        let manifest: Manifest = serde_json::from_str(text).map_err(|error| error.to_string())?;
        if manifest.format > MANIFEST_FORMAT {
            return Err(format!(
                "manifest format {} is newer than this dioramic ({MANIFEST_FORMAT}); update dioramic",
                manifest.format
            ));
        }
        Ok(manifest)
    }

    /// Pretty JSON with a trailing newline (stable: icons keep their order).
    pub fn to_json(&self) -> String {
        let mut json = serde_json::to_string_pretty(self).unwrap_or_default();
        json.push('\n');
        json
    }

    /// The pack description the core registry keeps.
    pub fn pack_info(&self) -> IconPackInfo {
        IconPackInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            license: self.license.clone(),
            source: self.source.clone(),
            version: self.version.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_defaults() {
        let manifest = Manifest::parse(
            r#"{"id":"local","name":"Project icons","icons":[{"id":"logo","file":"logo.svg","mount":"stand"}]}"#,
        )
        .unwrap();
        assert_eq!(manifest.format, MANIFEST_FORMAT);
        assert_eq!(manifest.icons[0].mount, Mount::Stand);
        assert!(manifest.icons[0].aliases.is_empty());
        let again = Manifest::parse(&manifest.to_json()).unwrap();
        assert_eq!(again, manifest);
        assert!(manifest.to_json().ends_with("}\n"));
    }

    #[test]
    fn rejects_newer_formats_and_bad_json() {
        assert!(Manifest::parse(r#"{"format":99,"id":"x","name":"x"}"#).unwrap_err().contains("newer"));
        assert!(Manifest::parse("{").is_err());
        assert!(
            Manifest::parse(r#"{"id":"x","name":"x","icons":[{"id":"a","file":"a.svg","mount":"side"}]}"#).is_err()
        );
    }
}
