//! Pin for JSON specs: edits a concrete syntax tree (`jsonc-parser`, as `dioramic init` does) so
//! the indentation, key order and line layout of everything else stay as written.

use jsonc_parser::ParseOptions;
use jsonc_parser::cst::{CstInputValue, CstObject, CstObjectProp, CstRootNode};
use serde_json::Value;

use super::{PinError, PinnedValue};

/// Top-level keys that make up the header of a spec, after which a missing key is inserted
/// (same order as the YAML pin).
const HEADER_KEYS: [&str; 6] = ["diorama", "title", "subtitle", "theme", "layout", "view"];

/// Writes `values` into the JSON text.
pub fn pin(text: &str, values: &[PinnedValue]) -> Result<String, PinError> {
    let root =
        CstRootNode::parse(text, &ParseOptions::default()).map_err(|error| PinError::Unparsable(error.to_string()))?;
    let document = root.object_value().ok_or(PinError::NotAMapping)?;
    for pinned in values {
        match pinned.section {
            Some(section) => set_nested(&document, section, pinned.key, &pinned.value),
            None => set_top_level(&document, pinned.key, &pinned.value),
        }
    }
    Ok(root.to_string())
}

fn set_nested(document: &CstObject, section: &str, key: &str, value: &Value) {
    let Some(property) = document.get(section) else {
        insert_after_header(document, section, object_with(key, value));
        return;
    };
    let Some(object) = property.object_value() else {
        // `"view": null` or similar: the section becomes an object.
        property.set_value(object_with(key, value));
        return;
    };
    match object.get(key) {
        Some(existing) => replace_value(&existing, value),
        None => {
            object.append(key, input(value));
        }
    }
}

fn set_top_level(document: &CstObject, key: &str, value: &Value) {
    match document.get(key) {
        Some(existing) => replace_value(&existing, value),
        None => insert_after_header(document, key, input(value)),
    }
}

/// Leaves an equal value alone so pinning twice never touches the text. An object pinned over an
/// object is edited key by key, so the keys it already has keep their place and formatting.
fn replace_value(property: &CstObjectProp, value: &Value) {
    if property.value().and_then(|node| node.to_serde_value()).as_ref() == Some(value) {
        return;
    }
    match (value, property.object_value()) {
        (Value::Object(entries), Some(object)) => {
            for (key, entry) in entries {
                match object.get(key) {
                    Some(existing) => replace_value(&existing, entry),
                    None => {
                        object.append(key, input(entry));
                    }
                }
            }
        }
        _ => property.set_value(input(value)),
    }
}

fn insert_after_header(document: &CstObject, key: &str, value: CstInputValue) {
    let position = document
        .properties()
        .iter()
        .rposition(|property| property.decoded_name().is_some_and(|name| HEADER_KEYS.contains(&name.as_str())))
        .map_or(0, |index| index + 1);
    document.insert(position, key, value);
}

fn object_with(key: &str, value: &Value) -> CstInputValue {
    CstInputValue::Object(vec![(key.to_owned(), input(value))])
}

fn input(value: &Value) -> CstInputValue {
    match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(flag) => CstInputValue::Bool(*flag),
        Value::Number(number) => CstInputValue::Number(number.to_string()),
        Value::String(text) => CstInputValue::String(text.clone()),
        Value::Array(items) => CstInputValue::Array(items.iter().map(input).collect()),
        Value::Object(map) => CstInputValue::Object(map.iter().map(|(key, item)| (key.clone(), input(item))).collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(section: Option<&'static str>, key: &'static str, value: Value) -> PinnedValue {
        PinnedValue { section, key, value }
    }

    #[test]
    fn inserts_missing_sections_after_the_header() -> Result<(), PinError> {
        let text = "{\n  \"diorama\": 1,\n  \"title\": \"T\",\n  \"nodes\": []\n}\n";
        let pinned = pin(text, &[value(Some("view"), "rotation", Value::from(90))])?;
        let at = |needle: &str| pinned.find(needle).unwrap_or(usize::MAX);
        assert!(at("\"title\"") < at("\"view\"") && at("\"view\"") < at("\"nodes\""), "{pinned}");
        assert!(pinned.ends_with("}\n"), "{pinned}");
        Ok(())
    }

    #[test]
    fn updates_values_in_place() -> Result<(), PinError> {
        let text = "{ \"diorama\": 1, \"theme\": \"blueprint\", \"view\": { \"rotation\": 0, \"padding\": 2 } }";
        let pinned = pin(
            text,
            &[value(None, "theme", Value::from("mono")), value(Some("view"), "rotation", Value::from("auto"))],
        )?;
        assert_eq!(
            pinned,
            "{ \"diorama\": 1, \"theme\": \"mono\", \"view\": { \"rotation\": \"auto\", \"padding\": 2 } }"
        );
        Ok(())
    }

    #[test]
    fn a_null_section_becomes_an_object() -> Result<(), PinError> {
        let pinned =
            pin("{\"diorama\": 1, \"layout\": null}", &[value(Some("layout"), "density", Value::from("compact"))])?;
        let parsed: Value = serde_json::from_str(&pinned).map_err(|error| PinError::Unparsable(error.to_string()))?;
        assert_eq!(parsed["layout"]["density"], "compact");
        Ok(())
    }

    #[test]
    fn a_non_object_root_is_refused() {
        assert_eq!(pin("[1]", &[value(None, "theme", Value::from("mono"))]), Err(PinError::NotAMapping));
    }
}
