//! Structured diagnostics: stable codes the AI (and humans) can act on.
//! Table of codes: `docs/04-spec.md#diagnósticos`.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Diagnostic severity. Errors stop the pipeline; warnings and hints never do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Blocks rendering.
    Error,
    /// Rendered, but something looks wrong.
    Warning,
    /// Suggestion.
    Hint,
}

/// Stable diagnostic code. Serialized as its `E_*` / `W_*` string.
///
/// Codes are shared by every crate (spec, core, render), so they live here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub enum DiagnosticCode {
    /// Syntax error in JSON/YAML, or a field with the wrong type.
    #[serde(rename = "E_PARSE")]
    Parse,
    /// `diorama:` missing or not `1`.
    #[serde(rename = "E_VERSION")]
    Version,
    /// `nodes` is empty.
    #[serde(rename = "E_NO_NODES")]
    NoNodes,
    /// Edge shorthand string could not be parsed.
    #[serde(rename = "E_EDGE_SYNTAX")]
    EdgeSyntax,
    /// Id is not `snake_case` (letters, digits, `_`, `-`, `/` allowed; must start with a letter).
    #[serde(rename = "E_BAD_ID")]
    BadId,
    /// Two nodes/zones/edges share an id.
    #[serde(rename = "E_DUPLICATE_ID")]
    DuplicateId,
    /// Reference to a node/zone/style that does not exist.
    #[serde(rename = "E_UNKNOWN_REF")]
    UnknownRef,
    /// Icon id/alias not in the catalog (emitted by `dioramic-core`).
    #[serde(rename = "E_UNKNOWN_ICON")]
    UnknownIcon,
    /// A zone is its own ancestor.
    #[serde(rename = "E_ZONE_CYCLE")]
    ZoneCycle,
    /// Malformed color (neither a token nor `#rgb`/`#rrggbb`).
    #[serde(rename = "E_BAD_COLOR")]
    BadColor,
    /// Value outside the accepted range (replicas, width, coordinates, durations…).
    #[serde(rename = "E_BAD_VALUE")]
    BadValue,
    /// Hard limits against abusive specs (500 nodes, 600 frames, 8192 px).
    #[serde(rename = "E_LIMIT")]
    Limit,
    /// Two labels/pills/zone labels overlap after placement (emitted by `dioramic-core`).
    #[serde(rename = "W_LABEL_OVERLAP")]
    LabelOverlap,
    /// An edge passes over another node (emitted by `dioramic-core`).
    #[serde(rename = "W_EDGE_THROUGH_NODE")]
    EdgeThroughNode,
    /// Label longer than ~28 characters; it will wrap.
    #[serde(rename = "W_LABEL_LONG")]
    LabelLong,
    /// Node without edges.
    #[serde(rename = "W_ISOLATED")]
    Isolated,
    /// More than ~40 nodes.
    #[serde(rename = "W_TOO_MANY_NODES")]
    TooManyNodes,
    /// Edge from a node to itself (drawn as nothing).
    #[serde(rename = "W_SELF_LOOP")]
    SelfLoop,
    /// Icon of a provider pack (`aws/…`, `gcp/…`, `azure/…`, `k8s/…`) that is not installed; a built-in
    /// stand-in is drawn (emitted by `dioramic-core`).
    #[serde(rename = "W_ICON_PACK_MISSING")]
    IconPackMissing,
    /// Two edges that share a node cross each other: their fan reads as an "X" (emitted by `dioramic-core`).
    #[serde(rename = "W_EDGE_CROSSING")]
    EdgeCrossing,
}

impl DiagnosticCode {
    /// Stable string form (`"E_UNKNOWN_ICON"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Parse => "E_PARSE",
            Self::Version => "E_VERSION",
            Self::NoNodes => "E_NO_NODES",
            Self::EdgeSyntax => "E_EDGE_SYNTAX",
            Self::BadId => "E_BAD_ID",
            Self::DuplicateId => "E_DUPLICATE_ID",
            Self::UnknownRef => "E_UNKNOWN_REF",
            Self::UnknownIcon => "E_UNKNOWN_ICON",
            Self::ZoneCycle => "E_ZONE_CYCLE",
            Self::BadColor => "E_BAD_COLOR",
            Self::BadValue => "E_BAD_VALUE",
            Self::Limit => "E_LIMIT",
            Self::LabelOverlap => "W_LABEL_OVERLAP",
            Self::EdgeThroughNode => "W_EDGE_THROUGH_NODE",
            Self::LabelLong => "W_LABEL_LONG",
            Self::Isolated => "W_ISOLATED",
            Self::TooManyNodes => "W_TOO_MANY_NODES",
            Self::SelfLoop => "W_SELF_LOOP",
            Self::IconPackMissing => "W_ICON_PACK_MISSING",
            Self::EdgeCrossing => "W_EDGE_CROSSING",
        }
    }

    /// Default severity of the code (`E_*` → error, `W_ISOLATED` → hint, other `W_*` → warning).
    pub fn default_severity(self) -> Severity {
        match self {
            Self::Isolated => Severity::Hint,
            Self::LabelOverlap
            | Self::EdgeThroughNode
            | Self::LabelLong
            | Self::TooManyNodes
            | Self::SelfLoop
            | Self::IconPackMissing
            | Self::EdgeCrossing => Severity::Warning,
            _ => Severity::Error,
        }
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 1-based position in the source text (only for parse errors and editor integration).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct SourceLocation {
    /// 1-based line.
    pub line: u32,
    /// 1-based column.
    pub column: u32,
}

/// One finding about a spec, pointing at the spec path that caused it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Diagnostic {
    /// Stable code (`E_UNKNOWN_REF`).
    pub code: DiagnosticCode,
    /// Severity (normally `code.default_severity()`).
    pub severity: Severity,
    /// Path into the spec: `nodes[3].icon`, `edges[2]`, `zones[0].parent`; empty for the whole document.
    pub path: String,
    /// Short English sentence for the AI.
    pub message: String,
    /// Actionable suggestion ("did you mean 'postgres'?").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// Source position when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<SourceLocation>,
}

impl Diagnostic {
    /// New diagnostic with the code's default severity.
    pub fn new(code: DiagnosticCode, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: code.default_severity(),
            path: path.into(),
            message: message.into(),
            hint: None,
            location: None,
        }
    }

    /// Adds a hint.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Adds a source location.
    pub fn at(mut self, location: SourceLocation) -> Self {
        self.location = Some(location);
        self
    }

    /// True for `Severity::Error`.
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ", self.code)?;
        if !self.path.is_empty() {
            write!(f, "{}: ", self.path)?;
        }
        f.write_str(&self.message)?;
        if let Some(hint) = &self.hint {
            write!(f, " ({hint})")?;
        }
        Ok(())
    }
}

/// Ordered list of diagnostics. Also the error type of the pipeline (`Result<_, Diagnostics>`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct Diagnostics(pub Vec<Diagnostic>);

impl Diagnostics {
    /// Empty list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one diagnostic.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.0.push(diagnostic);
    }

    /// Appends all diagnostics of `other`.
    pub fn extend(&mut self, other: Diagnostics) {
        self.0.extend(other.0);
    }

    /// True when at least one diagnostic is an error.
    pub fn has_errors(&self) -> bool {
        self.0.iter().any(Diagnostic::is_error)
    }

    /// True when empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Number of diagnostics.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Iterates in insertion order.
    pub fn iter(&self) -> std::slice::Iter<'_, Diagnostic> {
        self.0.iter()
    }

    /// Only the errors.
    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.0.iter().filter(|diagnostic| diagnostic.is_error())
    }

    /// Stable ordering for output: severity, then path, then code. Keeps determinism across runs.
    pub fn sort(&mut self) {
        self.0.sort_by(|a, b| (a.severity, &a.path, a.code).cmp(&(b.severity, &b.path, b.code)));
    }
}

impl From<Diagnostic> for Diagnostics {
    fn from(diagnostic: Diagnostic) -> Self {
        Self(vec![diagnostic])
    }
}

impl From<Vec<Diagnostic>> for Diagnostics {
    fn from(list: Vec<Diagnostic>) -> Self {
        Self(list)
    }
}

impl IntoIterator for Diagnostics {
    type Item = Diagnostic;
    type IntoIter = std::vec::IntoIter<Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a Diagnostics {
    type Item = &'a Diagnostic;
    type IntoIter = std::slice::Iter<'a, Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl fmt::Display for Diagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, diagnostic) in self.0.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            diagnostic.fmt(f)?;
        }
        Ok(())
    }
}

impl std::error::Error for Diagnostics {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_serializes_as_stable_string() {
        let json = serde_json::to_string(&DiagnosticCode::UnknownIcon).unwrap_or_default();
        assert_eq!(json, "\"E_UNKNOWN_ICON\"");
        assert_eq!(DiagnosticCode::UnknownIcon.as_str(), "E_UNKNOWN_ICON");
    }

    #[test]
    fn errors_are_detected() {
        let mut list = Diagnostics::new();
        list.push(Diagnostic::new(DiagnosticCode::Isolated, "nodes[0]", "node 'x' has no edges"));
        assert!(!list.has_errors());
        list.push(Diagnostic::new(DiagnosticCode::UnknownRef, "edges[0]", "node 'y' does not exist"));
        assert!(list.has_errors());
    }
}
