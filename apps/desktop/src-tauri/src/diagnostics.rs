//! Engine diagnostics → UI diagnostics, plus what the UI needs from the raw spec (ids, notes).

use dioramic_desktop_ipc::{DiagnosticView, NodeNote, Severity};
use dioramic_spec::{Diagnostic, DiagnosticCode, Diagnostics, SpecV1};

/// Raw spec facts the UI needs even when the engine cannot compile: element ids by index and node notes.
#[derive(Debug, Default)]
pub struct SpecOutline {
    node_ids: Vec<String>,
    zone_ids: Vec<String>,
    notes: Vec<NodeNote>,
}

impl SpecOutline {
    /// Outline of a spec text; empty when the text does not deserialize.
    pub fn of(text: &str) -> Self {
        parse_raw(text).map(|spec| Self::from_spec(&spec)).unwrap_or_default()
    }

    fn from_spec(spec: &SpecV1) -> Self {
        Self {
            node_ids: spec.nodes.iter().map(|node| node.id.clone()).collect(),
            zone_ids: spec.zones.iter().map(|zone| zone.id.clone()).collect(),
            notes: spec
                .nodes
                .iter()
                .filter_map(|node| {
                    let note = node.note.as_deref()?.trim();
                    (!note.is_empty()).then(|| NodeNote {
                        id: node.id.clone(),
                        title: node.label.clone().unwrap_or_else(|| node.id.clone()),
                        note: note.to_owned(),
                    })
                })
                .collect(),
        }
    }

    /// Node notes for hover tooltips.
    pub fn into_notes(self) -> Vec<NodeNote> {
        self.notes
    }

    /// `data-id` of the node or zone a spec path points at (`nodes[2].icon` → id of the third node).
    pub fn target_id(&self, path: &str) -> Option<String> {
        let (collection, index) = leading_index(path)?;
        let ids = match collection {
            "nodes" => &self.node_ids,
            "zones" => &self.zone_ids,
            _ => return None,
        };
        ids.get(index).cloned()
    }

    /// Converts engine diagnostics, resolving their targets.
    pub fn views(&self, diagnostics: &Diagnostics) -> Vec<DiagnosticView> {
        diagnostics.iter().map(|diagnostic| self.view(diagnostic)).collect()
    }

    fn view(&self, diagnostic: &Diagnostic) -> DiagnosticView {
        DiagnosticView {
            code: diagnostic.code.as_str().to_owned(),
            severity: severity(diagnostic.severity),
            path: diagnostic.path.clone(),
            message: diagnostic.message.clone(),
            hint: desktop_hint(diagnostic),
            line: diagnostic.location.map(|location| location.line),
            column: diagnostic.location.map(|location| location.column),
            target_id: self.target_id(&diagnostic.path),
        }
    }
}

/// The engine's hint, reworded where the CLI advice does not fit the app: a missing icon pack is installed
/// from a terminal, and the preview picks it up by itself (no "render again").
fn desktop_hint(diagnostic: &Diagnostic) -> Option<String> {
    let hint = diagnostic.hint.as_deref()?;
    if diagnostic.code == DiagnosticCode::IconPackMissing
        && let Some(command) = backticked(hint)
    {
        return Some(format!("Run `{command}` in a terminal; the preview updates once the pack is installed."));
    }
    Some(hint.to_owned())
}

/// The first `` `quoted` `` text of a hint.
fn backticked(text: &str) -> Option<&str> {
    let (_, rest) = text.split_once('`')?;
    let (quoted, _) = rest.split_once('`')?;
    (!quoted.is_empty()).then_some(quoted)
}

fn parse_raw(text: &str) -> Result<SpecV1, String> {
    if text.trim_start().starts_with('{') {
        serde_json::from_str(text).map_err(|error| error.to_string())
    } else {
        serde_saphyr::from_str(text).map_err(|error| error.to_string())
    }
}

/// `"nodes[12].icon"` → `("nodes", 12)`.
fn leading_index(path: &str) -> Option<(&str, usize)> {
    let (collection, rest) = path.split_once('[')?;
    let (index, _) = rest.split_once(']')?;
    Some((collection, index.parse().ok()?))
}

fn severity(severity: dioramic_spec::Severity) -> Severity {
    match severity {
        dioramic_spec::Severity::Error => Severity::Error,
        dioramic_spec::Severity::Warning => Severity::Warning,
        dioramic_spec::Severity::Hint => Severity::Hint,
    }
}

/// Syntax check used while the engine cannot run: deserializing the raw spec still catches YAML
/// syntax errors and wrong field types, with their position, so the editor stays useful.
pub fn syntax_diagnostics(text: &str) -> Vec<DiagnosticView> {
    let error = if text.trim_start().starts_with('{') {
        match serde_json::from_str::<SpecV1>(text) {
            Ok(_) => return Vec::new(),
            Err(error) => SyntaxError {
                message: error.to_string(),
                line: u32::try_from(error.line()).ok(),
                column: u32::try_from(error.column()).ok(),
            },
        }
    } else {
        match serde_saphyr::from_str::<SpecV1>(text) {
            Ok(_) => return Vec::new(),
            Err(error) => {
                let location = error.location();
                SyntaxError {
                    message: error.to_string(),
                    line: location.and_then(|location| u32::try_from(location.line()).ok()),
                    column: location.and_then(|location| u32::try_from(location.column()).ok()),
                }
            }
        }
    };
    vec![error.into_view()]
}

struct SyntaxError {
    message: String,
    line: Option<u32>,
    column: Option<u32>,
}

impl SyntaxError {
    fn into_view(self) -> DiagnosticView {
        // Parser messages may carry a rendered source snippet on later lines; the editor shows its own.
        let first_line = self.message.lines().next().unwrap_or_default().trim().to_owned();
        DiagnosticView {
            code: DiagnosticCode::Parse.as_str().to_owned(),
            severity: Severity::Error,
            path: String::new(),
            message: first_line,
            hint: None,
            line: self.line.filter(|line| *line > 0),
            column: self.column.filter(|column| *column > 0),
            target_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dioramic_spec::SourceLocation;

    const SPEC: &str = "diorama: 1\nzones:\n  - { id: data }\nnodes:\n  - { id: api, label: API, note: \"Stateless.\" }\n  - { id: db }\n";

    #[test]
    fn outline_resolves_paths_to_ids() {
        let outline = SpecOutline::of(SPEC);
        assert_eq!(outline.target_id("nodes[1].icon").as_deref(), Some("db"));
        assert_eq!(outline.target_id("zones[0]").as_deref(), Some("data"));
        assert_eq!(outline.target_id("nodes[9]"), None);
        assert_eq!(outline.target_id("edges[0]"), None);
        assert_eq!(outline.target_id(""), None);
    }

    #[test]
    fn outline_collects_notes_with_titles() {
        let notes = SpecOutline::of(SPEC).into_notes();
        assert_eq!(notes, vec![NodeNote { id: "api".into(), title: "API".into(), note: "Stateless.".into() }]);
    }

    #[test]
    fn diagnostics_keep_code_location_and_target() {
        let outline = SpecOutline::of(SPEC);
        let mut diagnostics = Diagnostics::new();
        diagnostics.push(
            Diagnostic::new(DiagnosticCode::Parse, "nodes[0].icon", "unknown icon")
                .with_hint("did you mean 'server'?")
                .at(SourceLocation { line: 5, column: 7 }),
        );
        let views = outline.views(&diagnostics);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].code, "E_PARSE");
        assert_eq!(views[0].severity, Severity::Error);
        assert_eq!(views[0].line, Some(5));
        assert_eq!(views[0].target_id.as_deref(), Some("api"));
        assert_eq!(views[0].hint.as_deref(), Some("did you mean 'server'?"));
    }

    #[test]
    fn missing_pack_hint_names_the_install_command() {
        let outline = SpecOutline::of(SPEC);
        let mut diagnostics = Diagnostics::new();
        diagnostics.push(
            Diagnostic::new(
                DiagnosticCode::IconPackMissing,
                "nodes[0].icon",
                "icon 'aws/lambda' needs the 'aws' icon pack",
            )
            .with_hint("run `dioramic icons install aws`, then render again"),
        );
        diagnostics.push(
            Diagnostic::new(DiagnosticCode::IconPackMissing, "nodes[1].icon", "no command").with_hint("install it"),
        );
        let views = outline.views(&diagnostics);
        assert_eq!(views[0].severity, Severity::Warning);
        assert_eq!(views[0].target_id.as_deref(), Some("api"));
        assert_eq!(
            views[0].hint.as_deref(),
            Some("Run `dioramic icons install aws` in a terminal; the preview updates once the pack is installed.")
        );
        assert_eq!(views[1].hint.as_deref(), Some("install it"));
    }

    #[test]
    fn syntax_check_accepts_a_valid_spec() {
        assert!(syntax_diagnostics(SPEC).is_empty());
    }

    #[test]
    fn syntax_check_reports_broken_yaml_with_a_line() {
        let diagnostics = syntax_diagnostics("diorama: 1\nnodes:\n  - { id: api\n");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "E_PARSE");
        assert!(diagnostics[0].line.is_some());
        assert!(!diagnostics[0].message.contains('\n'));
    }

    #[test]
    fn syntax_check_reports_broken_json() {
        let diagnostics = syntax_diagnostics("{\"diorama\": 1, ");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].line, Some(1));
    }
}
