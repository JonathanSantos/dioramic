//! Text → [`SpecV1`] for JSON and YAML (`serde-saphyr`; never `serde_yml`/`serde_yaml`, see ADR-003).

use std::path::Path;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::diagnostic::{Diagnostic, DiagnosticCode, Diagnostics, SourceLocation};
use crate::raw::{SPEC_VERSION, SpecV1};

/// Source text format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceFormat {
    /// JSON (what the AI sends through MCP).
    Json,
    /// YAML (`*.diorama.yaml`).
    Yaml,
}

impl SourceFormat {
    /// Guesses from the file name: `.json` → JSON, `.yaml`/`.yml` → YAML.
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "json" => Some(Self::Json),
            "yaml" | "yml" => Some(Self::Yaml),
            _ => None,
        }
    }

    /// Guesses from content: first non-blank char `{` → JSON, else YAML.
    pub fn sniff(source: &str) -> Self {
        if source.trim_start().starts_with('{') { Self::Json } else { Self::Yaml }
    }
}

/// Parses a spec. Errors are `E_PARSE` diagnostics with line/column when the parser reports them,
/// and an `E_VERSION` when `diorama` is not 1.
pub fn parse(source: &str, format: SourceFormat) -> Result<SpecV1, Diagnostics> {
    match deserialize::<SpecV1>(source, format) {
        Ok(spec) if spec.diorama == SPEC_VERSION => Ok(spec),
        Ok(spec) => Err(version_diagnostic(Some(&serde_json::Value::from(spec.diorama))).into()),
        Err(parse_error) => match probe_version(source, format) {
            Some(VersionProbe { diorama }) if !is_supported_version(diorama.as_ref()) => {
                Err(version_diagnostic(diorama.as_ref()).into())
            }
            _ => Err(parse_error.into_diagnostic().into()),
        },
    }
}

/// [`parse`] with [`SourceFormat::sniff`].
pub fn parse_auto(source: &str) -> Result<SpecV1, Diagnostics> {
    parse(source, SourceFormat::sniff(source))
}

/// Only the version field; everything else is ignored, so it parses even when the rest of the spec is wrong.
#[derive(Deserialize)]
struct VersionProbe {
    #[serde(default)]
    diorama: Option<serde_json::Value>,
}

fn probe_version(source: &str, format: SourceFormat) -> Option<VersionProbe> {
    deserialize::<VersionProbe>(source, format).ok()
}

fn is_supported_version(version: Option<&serde_json::Value>) -> bool {
    version.and_then(serde_json::Value::as_u64) == Some(u64::from(SPEC_VERSION))
}

fn version_diagnostic(found: Option<&serde_json::Value>) -> Diagnostic {
    let message = match found {
        None => "missing 'diorama' version field".to_owned(),
        Some(value) => format!("unsupported spec version {value}; this build understands version {SPEC_VERSION}"),
    };
    Diagnostic::new(DiagnosticCode::Version, "diorama", message)
        .with_hint(format!("start the spec with `diorama: {SPEC_VERSION}`"))
}

/// A parser error, reduced to what a diagnostic needs.
struct ParseFailure {
    message: String,
    location: Option<SourceLocation>,
}

impl ParseFailure {
    fn into_diagnostic(self) -> Diagnostic {
        let mut diagnostic = Diagnostic::new(DiagnosticCode::Parse, "", readable_message(&self.message));
        if let Some(hint) = hint_for(&self.message) {
            diagnostic = diagnostic.with_hint(hint);
        }
        match self.location {
            Some(location) => diagnostic.at(location),
            None => diagnostic,
        }
    }
}

/// Replaces serde's "data did not match any variant of untagged enum X" with the spec term for X.
fn readable_message(message: &str) -> String {
    const UNTAGGED_PREFIX: &str = "data did not match any variant of untagged enum ";
    let Some(type_name) = message.strip_prefix(UNTAGGED_PREFIX) else {
        return message.to_owned();
    };
    let term = match type_name.trim() {
        "EdgeEntry" => "edge",
        "EdgeKeyedValue" => "edge value",
        "DurationSpec" => "duration",
        "ColorRef" => "color",
        "FlowSpec" => "flow value",
        other => other,
    };
    format!("invalid {term}")
}

/// serde's message for an `untagged` enum names the Rust type, which means nothing to the author;
/// the hint says what shapes are accepted.
fn hint_for(message: &str) -> Option<&'static str> {
    if message.contains("EdgeEntry") || message.contains("EdgeKeyedValue") {
        Some(
            "an edge is a string \"a -> b: label @style\", a single-key map {\"a -> b\": label}, \
             or an object with from/to and known fields",
        )
    } else if message.contains("DurationSpec") {
        Some("write a duration as seconds (2), \"2.5s\" or \"800ms\"")
    } else if message.contains("ColorRef") {
        Some("write a color as a token (blue, orange, teal, green, violet, pink, red, gray) or \"#rrggbb\"")
    } else if message.contains("FlowSpec") {
        Some("flow is true, false or \"reverse\"")
    } else {
        None
    }
}

fn deserialize<T: DeserializeOwned>(source: &str, format: SourceFormat) -> Result<T, ParseFailure> {
    match format {
        SourceFormat::Json => serde_json::from_str(source).map_err(json_failure),
        SourceFormat::Yaml => serde_saphyr::from_str(source).map_err(|error| yaml_failure(&error)),
    }
}

fn json_failure(error: serde_json::Error) -> ParseFailure {
    let location = (error.line() > 0).then(|| SourceLocation {
        line: saturating_u32(error.line() as u64),
        column: saturating_u32(error.column() as u64),
    });
    ParseFailure { message: strip_position(&error.to_string()), location }
}

/// Both parsers append " at line L[,] column C"; the location is kept separately.
fn strip_position(message: &str) -> String {
    match message.rfind(" at line ") {
        Some(index) => message[..index].to_owned(),
        None => message.to_owned(),
    }
}

fn yaml_failure(error: &serde_saphyr::Error) -> ParseFailure {
    let plain = error.without_snippet();
    let location = plain.location().filter(|location| location.line() > 0).map(|location| SourceLocation {
        line: saturating_u32(location.line()),
        column: saturating_u32(location.column()),
    });
    ParseFailure { message: strip_position(&first_line(&plain.to_string())), location }
}

fn first_line(message: &str) -> String {
    message.lines().next().unwrap_or_default().trim().to_owned()
}

fn saturating_u32(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_error(source: &str, format: SourceFormat) -> Diagnostic {
        match parse(source, format) {
            Ok(spec) => panic!("expected an error, got {spec:?}"),
            Err(diagnostics) => diagnostics.0.into_iter().next().unwrap_or_else(|| panic!("empty diagnostics")),
        }
    }

    #[test]
    fn json_and_yaml_parse() {
        let json = r#"{"diorama": 1, "nodes": [{"id": "a"}]}"#;
        let yaml = "diorama: 1\nnodes:\n  - id: a\n";
        assert!(parse(json, SourceFormat::Json).is_ok());
        assert!(parse(yaml, SourceFormat::Yaml).is_ok());
        assert!(parse_auto(json).is_ok());
        assert!(parse_auto(yaml).is_ok());
    }

    #[test]
    fn json_syntax_error_has_location() {
        let diagnostic = parse_error("{\n  \"diorama\": 1,\n  \"nodes\": [\n}", SourceFormat::Json);
        assert_eq!(diagnostic.code, DiagnosticCode::Parse);
        assert_eq!(diagnostic.location.map(|location| location.line), Some(4));
        assert!(!diagnostic.message.contains("at line"), "{}", diagnostic.message);
    }

    #[test]
    fn yaml_syntax_error_has_location() {
        let diagnostic = parse_error("diorama: 1\nnodes:\n  - { id: a\n", SourceFormat::Yaml);
        assert_eq!(diagnostic.code, DiagnosticCode::Parse);
        assert!(diagnostic.location.is_some(), "{diagnostic:?}");
        assert!(!diagnostic.message.contains('\n'));
    }

    #[test]
    fn unknown_field_is_a_parse_error_in_both_formats() {
        let json = parse_error(r#"{"diorama": 1, "nodes": [{"id": "a", "colour": "red"}]}"#, SourceFormat::Json);
        assert_eq!(json.code, DiagnosticCode::Parse);
        assert!(json.message.contains("colour"), "{}", json.message);
        assert!(json.location.is_some());
        let yaml = parse_error("diorama: 1\nnodes:\n  - { id: a, colour: red }\n", SourceFormat::Yaml);
        assert_eq!(yaml.code, DiagnosticCode::Parse);
        assert!(yaml.message.contains("colour"), "{}", yaml.message);
        assert!(!yaml.message.contains("at line"), "{}", yaml.message);
        assert_eq!(yaml.location.map(|location| location.line), Some(3));
    }

    #[test]
    fn wrong_type_is_a_parse_error_in_both_formats() {
        let json = parse_error(r#"{"diorama": 1, "nodes": [{"id": "a", "replicas": "two"}]}"#, SourceFormat::Json);
        assert_eq!(json.code, DiagnosticCode::Parse);
        assert!(json.location.is_some());
        let yaml = parse_error("diorama: 1\nnodes:\n  - { id: a, replicas: two }\n", SourceFormat::Yaml);
        assert_eq!(yaml.code, DiagnosticCode::Parse);
        assert!(yaml.location.is_some());
    }

    #[test]
    fn bad_edge_shape_gets_a_hint() {
        let diagnostic = parse_error("diorama: 1\nnodes: [{ id: a }]\nedges:\n  - [a, b]\n", SourceFormat::Yaml);
        assert_eq!(diagnostic.code, DiagnosticCode::Parse);
        assert!(diagnostic.hint.is_some(), "{diagnostic:?}");
        assert_eq!(diagnostic.message, "invalid edge");
    }

    #[test]
    fn missing_or_wrong_version_is_e_version() {
        for (source, format) in [
            ("nodes:\n  - id: a\n", SourceFormat::Yaml),
            ("diorama: 2\nnodes:\n  - id: a\n", SourceFormat::Yaml),
            ("diorama: one\nnodes:\n  - id: a\n", SourceFormat::Yaml),
            (r#"{"nodes": [{"id": "a"}]}"#, SourceFormat::Json),
            (r#"{"diorama": 3, "nodes": [{"id": "a"}]}"#, SourceFormat::Json),
            (r#"{"diorama": 2, "bogus": true}"#, SourceFormat::Json),
        ] {
            let diagnostic = parse_error(source, format);
            assert_eq!(diagnostic.code, DiagnosticCode::Version, "{source}");
            assert!(diagnostic.hint.is_some());
        }
    }

    #[test]
    fn source_format_guesses() {
        assert_eq!(SourceFormat::from_path(Path::new("a.diorama.YAML")), Some(SourceFormat::Yaml));
        assert_eq!(SourceFormat::from_path(Path::new("a.json")), Some(SourceFormat::Json));
        assert_eq!(SourceFormat::from_path(Path::new("a.txt")), None);
        assert_eq!(SourceFormat::sniff("  {\"diorama\": 1}"), SourceFormat::Json);
        assert_eq!(SourceFormat::sniff("diorama: 1"), SourceFormat::Yaml);
    }
}
