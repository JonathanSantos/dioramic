//! Loading the spec of a tool call (`spec` inline or `spec_path`) and naming its outputs.

use std::path::{Path, PathBuf};

use dioramic_render::{SourceText, TextFormat};
use dioramic_spec::{SourceFormat, SpecV1};
use serde_json::Value;

use crate::outcome::ToolError;
use crate::params::SpecInputParams;
use crate::workspace::Workspace;

/// Largest spec file read from disk (a 500-node spec is well under 100 KB).
pub const MAX_SPEC_BYTES: u64 = 1024 * 1024;
/// Longest slug derived from a title.
const MAX_SLUG_CHARS: usize = 48;
/// Slug when the spec has no usable title.
const DEFAULT_SLUG: &str = "diagram";
/// Extension of saved specs.
pub const SPEC_EXTENSION: &str = "diorama.yaml";
/// Suffixes stripped from a spec file name to get its stem (longest first).
const SPEC_SUFFIXES: [&str; 6] = [".diorama.yaml", ".diorama.yml", ".diorama.json", ".yaml", ".yml", ".json"];

/// A spec ready for the engine.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedSpec {
    /// Text and format.
    pub text: SourceText,
    /// File it came from (`spec_path`), resolved inside the workspace.
    pub path: Option<PathBuf>,
}

impl LoadedSpec {
    /// True for an inline `spec`.
    pub fn is_inline(&self) -> bool {
        self.path.is_none()
    }

    /// File-name friendly name: the title slug, else `diagram`.
    pub fn slug(&self) -> String {
        let spec = match self.text.format {
            Some(format) => dioramic_spec::parse(&self.text.text, SourceFormat::from(format)),
            None => dioramic_spec::parse_auto(&self.text.text),
        };
        spec.ok().and_then(|spec| spec.title).map_or_else(|| DEFAULT_SLUG.to_owned(), |title| slugify(&title))
    }

    /// The spec as YAML for `save_spec`. JSON input is converted; YAML text is kept as written.
    pub fn to_yaml(&self) -> Result<String, ToolError> {
        if self.text.format != Some(TextFormat::Json) {
            return Ok(self.text.text.clone());
        }
        let spec: SpecV1 = dioramic_spec::parse(&self.text.text, SourceFormat::Json)?;
        serde_saphyr::to_string(&spec)
            .map_err(|error| ToolError::Internal(format!("cannot write spec as YAML: {error}")))
    }
}

/// Loads `spec` or `spec_path` (exactly one), reading files only inside the workspace.
pub fn load(input: &SpecInputParams, workspace: &Workspace) -> Result<LoadedSpec, ToolError> {
    match (&input.spec, &input.spec_path) {
        (Some(_), Some(_)) => Err(ToolError::BadRequest("provide either `spec` or `spec_path`, not both".to_owned())),
        (None, None) => {
            Err(ToolError::BadRequest("provide `spec` (inline DioramaSpec object) or `spec_path`".to_owned()))
        }
        (Some(spec), None) => Ok(LoadedSpec { text: inline_text(spec)?, path: None }),
        (None, Some(path)) => load_file(&workspace.resolve(Path::new(path))?),
    }
}

/// Inline JSON object → JSON text; a string is accepted too (some models send YAML text).
fn inline_text(spec: &Value) -> Result<SourceText, ToolError> {
    match spec {
        Value::String(text) => Ok(SourceText { text: text.clone(), format: None }),
        Value::Object(_) => Ok(SourceText { text: spec.to_string(), format: Some(TextFormat::Json) }),
        _ => Err(ToolError::BadRequest("`spec` must be a DioramaSpec object".to_owned())),
    }
}

fn load_file(path: &Path) -> Result<LoadedSpec, ToolError> {
    let metadata = std::fs::metadata(path).map_err(|error| ToolError::io("read", path, error))?;
    if !metadata.is_file() {
        return Err(ToolError::BadRequest(format!("spec_path '{}' is not a file", path.display())));
    }
    if metadata.len() > MAX_SPEC_BYTES {
        return Err(ToolError::BadRequest(format!(
            "spec_path '{}' has {} bytes; the limit is {MAX_SPEC_BYTES}",
            path.display(),
            metadata.len()
        )));
    }
    let text = std::fs::read_to_string(path).map_err(|error| ToolError::io("read", path, error))?;
    Ok(LoadedSpec { text: SourceText { text, format: format_of(path) }, path: Some(path.to_path_buf()) })
}

fn format_of(path: &Path) -> Option<TextFormat> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("json") => Some(TextFormat::Json),
        Some("yaml" | "yml") => Some(TextFormat::Yaml),
        _ => None,
    }
}

/// `Checkout Service!` → `checkout-service`.
pub fn slugify(title: &str) -> String {
    let mut slug = String::new();
    for character in title.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let trimmed: String = slug.trim_end_matches('-').chars().take(MAX_SLUG_CHARS).collect();
    let trimmed = trimmed.trim_end_matches('-');
    if trimmed.is_empty() { DEFAULT_SLUG.to_owned() } else { trimmed.to_owned() }
}

/// `checkout.diorama.yaml` → `checkout`; other names lose their last extension.
pub fn spec_stem(path: &Path) -> String {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    SPEC_SUFFIXES
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .map(str::to_owned)
        .or_else(|| path.file_stem().map(|stem| stem.to_string_lossy().into_owned()))
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| DEFAULT_SLUG.to_owned())
}

/// Directory whose `icons/` holds the project icons of a spec: the spec file's directory, else the
/// workspace base (inline specs).
pub fn spec_directory(loaded: &LoadedSpec, workspace: &Workspace) -> PathBuf {
    loaded
        .path
        .as_ref()
        .and_then(|path| path.parent())
        .map_or_else(|| workspace.base().to_path_buf(), Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::from_dirs([dir.path().to_path_buf()]).unwrap();
        (dir, workspace)
    }

    fn inline(spec: Value) -> SpecInputParams {
        SpecInputParams { spec: Some(spec), spec_path: None }
    }

    #[test]
    fn slugs_are_lowercase_ascii_with_dashes() {
        assert_eq!(slugify("Checkout Service"), "checkout-service");
        assert_eq!(slugify("  API #1 / v2!! "), "api-1-v2");
        assert_eq!(slugify("¿¿??"), "diagram");
        assert!(slugify(&"x".repeat(100)).len() <= MAX_SLUG_CHARS);
    }

    #[test]
    fn spec_stems_drop_the_diorama_suffix() {
        assert_eq!(spec_stem(Path::new("a/checkout.diorama.yaml")), "checkout");
        assert_eq!(spec_stem(Path::new("net.diorama.json")), "net");
        assert_eq!(spec_stem(Path::new("plain.png")), "plain");
    }

    #[test]
    fn exactly_one_source_is_required() {
        let (_dir, workspace) = workspace();
        assert!(matches!(load(&SpecInputParams::default(), &workspace), Err(ToolError::BadRequest(_))));
        let both = SpecInputParams { spec: Some(serde_json::json!({})), spec_path: Some("a.yaml".to_owned()) };
        assert!(matches!(load(&both, &workspace), Err(ToolError::BadRequest(_))));
        assert!(matches!(load(&inline(Value::Bool(true)), &workspace), Err(ToolError::BadRequest(_))));
    }

    #[test]
    fn inline_objects_become_json_text_with_a_title_slug() {
        let (_dir, workspace) = workspace();
        let loaded =
            load(&inline(serde_json::json!({ "diorama": 1, "title": "My Net", "nodes": [{ "id": "a" }] })), &workspace)
                .unwrap();
        assert!(loaded.is_inline());
        assert_eq!(loaded.text.format, Some(TextFormat::Json));
        assert_eq!(loaded.slug(), "my-net");
        let yaml = loaded.to_yaml().unwrap();
        assert!(yaml.contains("title: My Net"), "{yaml}");
        assert!(dioramic_spec::parse(&yaml, SourceFormat::Yaml).is_ok(), "{yaml}");
    }

    #[test]
    fn files_are_read_inside_the_workspace_only() {
        let (dir, workspace) = workspace();
        std::fs::write(dir.path().join("a.diorama.yaml"), "diorama: 1\nnodes:\n  - { id: a }\n").unwrap();
        let loaded =
            load(&SpecInputParams { spec: None, spec_path: Some("a.diorama.yaml".to_owned()) }, &workspace).unwrap();
        assert_eq!(loaded.text.format, Some(TextFormat::Yaml));
        assert_eq!(loaded.slug(), "diagram");
        let outside = SpecInputParams { spec: None, spec_path: Some("/etc/hosts".to_owned()) };
        assert!(matches!(load(&outside, &workspace), Err(ToolError::Path(_))));
        let missing = SpecInputParams { spec: None, spec_path: Some("missing.yaml".to_owned()) };
        assert!(matches!(load(&missing, &workspace), Err(ToolError::Io { .. })));
    }
}
