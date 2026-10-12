//! Semantic check of a pin: the edited text must be the original document with exactly the pinned
//! values changed. Text edits are surgical and therefore heuristic; this is what makes them safe.

use serde_json::{Map, Value};

use super::{PinError, PinnedValue, is_json};

/// Parses a spec text (JSON or YAML) into a generic document. An empty YAML text is an empty mapping.
pub fn parse(text: &str) -> Result<Value, PinError> {
    let value = if is_json(text) {
        serde_json::from_str(text).map_err(|error| PinError::Unparsable(error.to_string()))?
    } else {
        serde_saphyr::from_str::<Value>(text).map_err(|error| PinError::Unparsable(error.to_string()))?
    };
    match value {
        Value::Object(_) => Ok(value),
        Value::Null => Ok(Value::Object(Map::new())),
        _ => Err(PinError::NotAMapping),
    }
}

/// Checks that `pinned` parses to `original` with `values` applied.
pub fn same_document_with(original: &Value, pinned: &str, values: &[PinnedValue]) -> Result<(), PinError> {
    let first_label = values.first().map_or("view", label);
    let actual = parse(pinned).map_err(|_| PinError::UnsupportedLayout(first_label))?;
    let expected = apply(original.clone(), values);
    if actual == expected {
        return Ok(());
    }
    let wrong = values.iter().find(|value| lookup(&actual, value) != Some(&value.value));
    Err(PinError::UnsupportedLayout(wrong.map_or(first_label, label)))
}

/// Name shown to the user for the part of the spec a value lives in.
fn label(value: &PinnedValue) -> &'static str {
    value.section.unwrap_or(value.key)
}

fn apply(mut document: Value, values: &[PinnedValue]) -> Value {
    for pinned in values {
        let Value::Object(root) = &mut document else { break };
        let target = match pinned.section {
            Some(section) => {
                let entry = root.entry(section).or_insert_with(|| Value::Object(Map::new()));
                if !entry.is_object() {
                    *entry = Value::Object(Map::new());
                }
                match entry {
                    Value::Object(map) => map,
                    _ => continue,
                }
            }
            None => root,
        };
        target.insert(pinned.key.to_owned(), pinned.value.clone());
    }
    document
}

fn lookup<'a>(document: &'a Value, pinned: &PinnedValue) -> Option<&'a Value> {
    let container = match pinned.section {
        Some(section) => document.get(section)?,
        None => document,
    };
    container.get(pinned.key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rotation(degrees: u16) -> PinnedValue {
        PinnedValue { section: Some("view"), key: "rotation", value: Value::from(degrees) }
    }

    #[test]
    fn accepts_exactly_the_pinned_change() -> Result<(), PinError> {
        let original = parse("diorama: 1\nview: { padding: 2 }\n")?;
        same_document_with(&original, "diorama: 1\nview: { padding: 2, rotation: 90 }\n", &[rotation(90)])
    }

    #[test]
    fn rejects_collateral_changes() -> Result<(), PinError> {
        let original = parse("diorama: 1\nview: { padding: 2 }\n")?;
        let outcome = same_document_with(&original, "diorama: 1\nview: { rotation: 90 }\n", &[rotation(90)]);
        assert_eq!(outcome, Err(PinError::UnsupportedLayout("view")));
        Ok(())
    }

    #[test]
    fn a_scalar_section_becomes_a_mapping() -> Result<(), PinError> {
        let original = parse("diorama: 1\nview: null\n")?;
        same_document_with(&original, "diorama: 1\nview: { rotation: 0 }\n", &[rotation(0)])
    }

    #[test]
    fn empty_and_non_mapping_documents() {
        assert_eq!(parse(""), Ok(Value::Object(Map::new())));
        assert_eq!(parse("- a\n"), Err(PinError::NotAMapping));
        assert!(matches!(parse("{ \"a\": "), Err(PinError::Unparsable(_))));
    }
}
