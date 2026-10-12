//! "Pin": writes the current view (rotation, density, theme, layout algorithm, routing, grid) into the spec **text** with surgical
//! edits. Reserializing would drop comments and formatting (doc 08), so only the values that change
//! are touched: YAML specs are edited line by line ([`yaml`]), JSON specs through a concrete syntax
//! tree ([`json`]).
//!
//! Every edit is then checked semantically ([`verify`]): the pinned text must parse to the original
//! document with exactly the pinned keys changed. When a layout is too unusual to edit safely
//! (multi-line flow map with comments, anchors shared through aliases, …) pinning is refused instead
//! of risking a broken spec.

mod json;
mod verify;
mod yaml;

use dioramic_desktop_ipc::{ViewGrid, ViewRotation, ViewState};
use serde_json::{Map, Value};
use thiserror::Error;

/// Why the view could not be pinned into the text.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PinError {
    /// The current text does not parse, so the edit could not be checked.
    #[error("fix the spec errors before pinning the view ({0})")]
    Unparsable(String),
    /// The top level of the spec is not a mapping.
    #[error("the spec is not a mapping at the top level")]
    NotAMapping,
    /// The section is written in a way pinning cannot edit without breaking it.
    #[error("cannot pin into `{0}` as it is written; edit it by hand")]
    UnsupportedLayout(&'static str),
}

/// One value to write: `section.key` (nested) or a top-level `key`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PinnedValue {
    /// Top-level section (`view`, `layout`) or `None` for a top-level key.
    pub section: Option<&'static str>,
    /// Key that receives the value.
    pub key: &'static str,
    /// The value, as data.
    pub value: Value,
}

/// Keys of a pinned mapping are written in the order the docs list them (`view.grid`), the rest after.
const PREFERRED_KEY_ORDER: [&str; 4] = ["size", "style", "opacity", "color"];

impl PinnedValue {
    /// The value as a YAML plain scalar, or a one-line flow mapping for objects.
    pub fn yaml_scalar(&self) -> String {
        yaml_inline(&self.value)
    }
}

fn yaml_inline(value: &Value) -> String {
    match value {
        Value::String(text) if is_plain_yaml(text) => text.clone(),
        Value::Object(map) => {
            let entries: Vec<String> =
                ordered_keys(map).into_iter().map(|key| format!("{key}: {}", yaml_inline(&map[key]))).collect();
            format!("{{ {} }}", entries.join(", "))
        }
        // JSON strings, numbers, booleans and arrays are valid YAML flow scalars as they are.
        other => other.to_string(),
    }
}

/// Words that can stay unquoted in a flow mapping and still read back as the same string.
fn is_plain_yaml(text: &str) -> bool {
    const RESERVED: [&str; 8] = ["true", "false", "null", "yes", "no", "on", "off", "~"];
    let mut characters = text.chars();
    characters.next().is_some_and(|first| first.is_ascii_alphabetic())
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-')
        && !RESERVED.contains(&text.to_ascii_lowercase().as_str())
}

fn ordered_keys(map: &Map<String, Value>) -> Vec<&str> {
    let preferred = PREFERRED_KEY_ORDER.iter().copied().filter(|key| map.contains_key(*key));
    let others = map.keys().map(String::as_str).filter(|key| !PREFERRED_KEY_ORDER.contains(key));
    preferred.chain(others).collect()
}

/// Writes the `Some` fields of `view` into the spec text, refusing layouts it cannot edit safely.
pub fn try_pin_view(text: &str, view: &ViewState) -> Result<String, PinError> {
    if *view == ViewState::default() {
        return Ok(text.to_owned());
    }
    let original = verify::parse(text)?;
    let values = pinned_values(view, &original);
    if values.is_empty() {
        return Ok(text.to_owned());
    }
    let pinned = if is_json(text) { json::pin(text, &values)? } else { yaml::pin(text, &values) };
    verify::same_document_with(&original, &pinned, &values)?;
    Ok(pinned)
}

/// Like [`try_pin_view`], but never fails: when the view cannot be pinned the text is returned
/// unchanged, so a spec is never corrupted.
pub fn pin_view(text: &str, view: &ViewState) -> String {
    try_pin_view(text, view).unwrap_or_else(|_| text.to_owned())
}

/// Same criterion as the preview (`view::sniff_format`): JSON specs start with `{`.
fn is_json(text: &str) -> bool {
    text.trim_start().starts_with('{')
}

fn pinned_values(view: &ViewState, original: &Value) -> Vec<PinnedValue> {
    let mut values = Vec::new();
    if let Some(rotation) = view.rotation {
        values.push(PinnedValue { section: Some("view"), key: "rotation", value: rotation_value(rotation) });
    }
    if let Some(density) = view.density {
        values.push(PinnedValue { section: Some("layout"), key: "density", value: density.spec_value().into() });
    }
    if let Some(theme) = view.theme {
        values.push(PinnedValue { section: None, key: "theme", value: theme.spec_value().into() });
    }
    if let Some(layout) = view.layout {
        values.push(PinnedValue { section: Some("layout"), key: "algorithm", value: layout.spec_value().into() });
    }
    if let Some(routing) = view.routing {
        values.push(PinnedValue { section: Some("layout"), key: "routing", value: routing.spec_value().into() });
    }
    if let Some(value) = view.grid.and_then(|grid| grid_value(grid, original)) {
        values.push(PinnedValue { section: Some("view"), key: "grid", value });
    }
    values
}

/// `view.grid` with the view's size, style and opacity merged over what the spec already has (its
/// `color` stays). A field still at its default is written only when the spec names it; `None` when
/// there is nothing to write at all.
fn grid_value(grid: ViewGrid, original: &Value) -> Option<Value> {
    let existing = original.get("view").and_then(|view| view.get("grid"));
    let mut map = existing.and_then(Value::as_object).cloned().unwrap_or_default();
    let fields = [
        ("size", Value::from(grid.size), grid.size == ViewGrid::DEFAULT_SIZE),
        ("style", Value::from(grid.style.spec_value()), grid.style == Default::default()),
        ("opacity", Value::from(grid.opacity), grid.opacity == ViewGrid::DEFAULT_OPACITY),
    ];
    for (key, value, is_default) in fields {
        let unchanged = map.get(key).is_some_and(|current| same_number_or_value(current, &value));
        if unchanged || (is_default && !map.contains_key(key)) {
            continue;
        }
        map.insert(key.to_owned(), value);
    }
    (existing.is_some() || !map.is_empty()).then_some(Value::Object(map))
}

/// `size: 1` in the spec equals the view's `1.0`: keep the spec's spelling.
fn same_number_or_value(current: &Value, wanted: &Value) -> bool {
    match (current.as_f64(), wanted.as_f64()) {
        (Some(current), Some(wanted)) => current == wanted,
        _ => current == wanted,
    }
}

/// `view.rotation` is a number of degrees or the string `auto`.
fn rotation_value(rotation: ViewRotation) -> Value {
    rotation.degrees().map_or_else(|| Value::from(rotation.spec_value()), Value::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dioramic_desktop_ipc::{DiagramTheme, GridStyle, ViewDensity, ViewLayout, ViewRouting};

    fn rotation(rotation: ViewRotation) -> ViewState {
        ViewState { rotation: Some(rotation), ..ViewState::default() }
    }

    #[test]
    fn empty_view_changes_nothing_even_in_broken_text() {
        let text = "diorama: 1\nnodes: [\n";
        assert_eq!(try_pin_view(text, &ViewState::default()), Ok(text.to_owned()));
    }

    #[test]
    fn broken_text_is_refused_and_left_untouched() {
        let text = "diorama: 1\nnodes: [\n";
        assert!(matches!(try_pin_view(text, &rotation(ViewRotation::Deg90)), Err(PinError::Unparsable(_))));
        assert_eq!(pin_view(text, &rotation(ViewRotation::Deg90)), text);
    }

    #[test]
    fn json_specs_stay_json_and_keep_their_formatting() {
        let text = "{\n  \"diorama\": 1,\n  \"title\": \"Shop\",\n  \"nodes\": [{ \"id\": \"a\" }]\n}\n";
        let view = ViewState {
            rotation: Some(ViewRotation::Deg90),
            density: Some(ViewDensity::Compact),
            theme: Some(DiagramTheme::Paper),
            ..ViewState::default()
        };
        let pinned = try_pin_view(text, &view).unwrap_or_default();
        let value: Value = serde_json::from_str(&pinned).unwrap_or_default();
        assert_eq!(value["view"]["rotation"], 90);
        assert_eq!(value["layout"]["density"], "compact");
        assert_eq!(value["theme"], "paper");
        assert!(pinned.contains("  \"nodes\": [{ \"id\": \"a\" }]\n"), "{pinned}");
        assert_eq!(pin_view(&pinned, &view), pinned);
    }

    fn rails_on_a_dotted_grid() -> ViewState {
        ViewState {
            routing: Some(ViewRouting::Rails),
            grid: Some(ViewGrid { size: 0.5, style: GridStyle::Dots, opacity: 0.6 }),
            ..ViewState::default()
        }
    }

    #[test]
    fn routing_and_grid_are_pinned_into_yaml() {
        let text = "diorama: 1\nlayout: { density: compact }\nnodes:\n  - { id: a }\n";
        assert_eq!(
            try_pin_view(text, &rails_on_a_dotted_grid()),
            Ok("diorama: 1\nlayout: { density: compact, routing: rails }\nview: { grid: { size: 0.5, style: dots, opacity: 0.6 } }\nnodes:\n  - { id: a }\n".to_owned())
        );
    }

    #[test]
    fn layout_algorithm_is_pinned_into_yaml_and_json() {
        let view = ViewState { layout: Some(ViewLayout::Hub), ..ViewState::default() };
        let text = "diorama: 1\nlayout: { density: compact } # keep\nnodes:\n  - { id: a }\n";
        assert_eq!(
            try_pin_view(text, &view),
            Ok("diorama: 1\nlayout: { density: compact, algorithm: hub } # keep\nnodes:\n  - { id: a }\n".to_owned())
        );
        let flow = ViewState { layout: Some(ViewLayout::Flow), ..ViewState::default() };
        let json = "{\n  \"diorama\": 1,\n  \"theme\": \"schematic\",\n  \"nodes\": [{ \"id\": \"a\" }]\n}\n";
        let pinned = try_pin_view(json, &flow).unwrap_or_default();
        let value: Value = serde_json::from_str(&pinned).unwrap_or_default();
        assert_eq!(value["layout"]["algorithm"], "flow");
        assert_eq!(pin_view(&pinned, &flow), pinned);
    }

    #[test]
    fn grid_keeps_the_spec_color_and_the_spelling_of_unchanged_values() {
        let text = "diorama: 1\nview:\n  rotation: 90 # keep\n  grid: { color: \"#3e50c8\", size: 1 }\nnodes:\n  - { id: a }\n";
        let view = ViewState {
            grid: Some(ViewGrid { style: GridStyle::Dots, ..ViewGrid::default() }),
            ..ViewState::default()
        };
        assert_eq!(
            try_pin_view(text, &view),
            Ok("diorama: 1\nview:\n  rotation: 90 # keep\n  grid: { size: 1, style: dots, color: \"#3e50c8\" }\nnodes:\n  - { id: a }\n".to_owned())
        );
    }

    #[test]
    fn a_nested_block_grid_is_replaced_whole() {
        let text = "diorama: 1\nview:\n  grid:\n    size: 2\n    style: lines\n  rotation: 0\nnodes:\n  - { id: a }\n";
        let pinned = try_pin_view(text, &rails_on_a_dotted_grid());
        assert_eq!(
            pinned,
            Ok("diorama: 1\nview:\n  grid: { size: 0.5, style: dots, opacity: 0.6 }\n  rotation: 0\nlayout: { routing: rails }\nnodes:\n  - { id: a }\n".to_owned())
        );
    }

    #[test]
    fn a_default_grid_over_a_spec_without_one_writes_nothing() {
        let text = "diorama: 1\nnodes:\n  - { id: a }\n";
        let view = ViewState { grid: Some(ViewGrid::default()), ..ViewState::default() };
        assert_eq!(try_pin_view(text, &view), Ok(text.to_owned()));
    }

    #[test]
    fn routing_and_grid_are_pinned_into_json_in_place() {
        let text = "{\n  \"diorama\": 1,\n  \"view\": { \"grid\": { \"color\": \"slate\" } },\n  \"nodes\": [{ \"id\": \"a\" }]\n}\n";
        let pinned = try_pin_view(text, &rails_on_a_dotted_grid()).unwrap_or_default();
        let value: Value = serde_json::from_str(&pinned).unwrap_or_default();
        assert_eq!(value["layout"]["routing"], "rails");
        assert_eq!(value["view"]["grid"]["style"], "dots");
        assert_eq!(value["view"]["grid"]["size"], 0.5);
        assert_eq!(value["view"]["grid"]["color"], "slate");
        let at = |needle: &str| pinned.find(needle).unwrap_or(usize::MAX);
        assert!(at("\"color\"") < at("\"size\""), "the spec's keys keep their place: {pinned}");
        assert_eq!(pin_view(&pinned, &rails_on_a_dotted_grid()), pinned);
    }

    #[test]
    fn yaml_flow_values_quote_what_would_not_read_back() {
        assert_eq!(yaml_inline(&Value::from("dots")), "dots");
        assert_eq!(yaml_inline(&Value::from("#3e50c8")), "\"#3e50c8\"");
        assert_eq!(yaml_inline(&Value::from("no")), "\"no\"");
        assert_eq!(yaml_inline(&serde_json::json!({ "style": "none", "size": 2 })), "{ size: 2, style: none }");
    }

    #[test]
    fn auto_rotation_is_a_string() {
        assert_eq!(rotation_value(ViewRotation::Auto), Value::from("auto"));
        assert_eq!(rotation_value(ViewRotation::Deg270), Value::from(270));
    }

    #[test]
    fn a_shared_anchor_is_refused() {
        let text = "diorama: 1\nview: &camera\n  rotation: 0\nextra: *camera\nnodes:\n  - { id: a }\n";
        assert_eq!(try_pin_view(text, &rotation(ViewRotation::Deg90)), Err(PinError::UnsupportedLayout("view")));
    }

    #[test]
    fn an_anchored_block_section_is_edited_in_place() {
        let text = "diorama: 1\nview: &camera # keep\n  rotation: 0\nnodes:\n  - { id: a }\n";
        assert_eq!(
            try_pin_view(text, &rotation(ViewRotation::Deg90)),
            Ok("diorama: 1\nview: &camera # keep\n  rotation: 90\nnodes:\n  - { id: a }\n".to_owned())
        );
    }

    #[test]
    fn a_multiline_flow_map_is_folded_into_one_line() {
        let text = "diorama: 1\nview: {\n  rotation: 0,\n  padding: 2\n} # camera\nnodes: []\n";
        assert_eq!(
            try_pin_view(text, &rotation(ViewRotation::Deg180)),
            Ok("diorama: 1\nview: { rotation: 180, padding: 2 } # camera\nnodes: []\n".to_owned())
        );
    }

    #[test]
    fn a_multiline_flow_map_with_comments_is_refused() {
        let text = "diorama: 1\nview: {\n  rotation: 0, # chosen\n}\nnodes: []\n";
        assert_eq!(try_pin_view(text, &rotation(ViewRotation::Deg90)), Err(PinError::UnsupportedLayout("view")));
        assert_eq!(pin_view(text, &rotation(ViewRotation::Deg90)), text);
    }
}
