//! MCP resources: `dioramic://schema/v1`, `dioramic://guide/{topic}`, `dioramic://examples/{name}`,
//! `dioramic://icons/catalog`. Everything is embedded in the binary; nothing is read from disk.

use dioramic_core::icons::{self, IconEntry};
use rmcp::model::{Resource, ResourceContents, ResourceTemplate};
use serde::Serialize;

use crate::guides::GuideTopic;

/// URI of the JSON Schema of `SpecV1`.
pub const SCHEMA_URI: &str = "dioramic://schema/v1";
/// URI of the icon catalog.
pub const ICON_CATALOG_URI: &str = "dioramic://icons/catalog";
const GUIDE_PREFIX: &str = "dioramic://guide/";
const EXAMPLE_PREFIX: &str = "dioramic://examples/";

const MIME_JSON: &str = "application/json";
const MIME_MARKDOWN: &str = "text/markdown";
const MIME_YAML: &str = "application/yaml";

/// Example specs (`examples/`), embedded at build time.
pub const EXAMPLES: [(&str, &str); 7] = [
    ("minimal", include_str!("../../../examples/minimal.diorama.yaml")),
    ("checkout", include_str!("../../../examples/checkout.diorama.yaml")),
    ("bank-branch", include_str!("../../../examples/bank-branch.diorama.yaml")),
    ("kubernetes-platform", include_str!("../../../examples/kubernetes-platform.diorama.yaml")),
    ("event-driven", include_str!("../../../examples/event-driven.diorama.yaml")),
    ("ml-pipeline", include_str!("../../../examples/ml-pipeline.diorama.yaml")),
    ("aws-serverless", include_str!("../../../examples/aws-serverless.diorama.yaml")),
];

/// An icon as exposed to the model (`search_icons`, catalog resource).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IconSummary {
    /// Canonical id.
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// Aliases accepted in `icon:`.
    pub aliases: &'static [&'static str],
    /// Category (`data`, `k8s`…).
    pub category: &'static str,
    /// `procedural` | `isometric` | `flat`.
    pub kind: &'static str,
    /// Pack of a flat icon (`aws`, `local`); absent for built-in icons.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack: Option<&'static str>,
}

impl From<&IconEntry> for IconSummary {
    fn from(entry: &IconEntry) -> Self {
        Self {
            id: entry.id,
            name: entry.name,
            aliases: entry.aliases,
            category: entry.category.as_str(),
            kind: entry.kind.as_str(),
            pack: icons::flat::entry_pack(entry),
        }
    }
}

/// Why a resource could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ResourceError {
    /// Unknown URI.
    #[error("resource '{0}' does not exist")]
    NotFound(String),
    /// Serialization failure (should not happen).
    #[error("cannot serialize resource '{uri}': {message}")]
    Serialize {
        /// URI.
        uri: String,
        /// Cause.
        message: String,
    },
}

/// Concrete resources (one per guide topic and example, plus schema and catalog).
pub fn list() -> Vec<Resource> {
    let fixed = [
        Resource::new(SCHEMA_URI, "spec-schema-v1").with_title("DioramaSpec v1 JSON Schema").with_mime_type(MIME_JSON),
        Resource::new(ICON_CATALOG_URI, "icon-catalog")
            .with_title("Icon catalog")
            .with_description("Every icon id with aliases and category")
            .with_mime_type(MIME_JSON),
    ];
    let guides = GuideTopic::ALL.into_iter().map(|topic| {
        Resource::new(guide_uri(topic), format!("guide-{}", topic.as_str()))
            .with_description(topic.summary())
            .with_mime_type(MIME_MARKDOWN)
    });
    let examples = EXAMPLES.iter().map(|(name, _)| {
        Resource::new(example_uri(name), format!("example-{name}"))
            .with_description(format!("Example spec '{name}'"))
            .with_mime_type(MIME_YAML)
    });
    fixed.into_iter().chain(guides).chain(examples).collect()
}

/// URI templates for guides and examples.
pub fn templates() -> Vec<ResourceTemplate> {
    vec![
        ResourceTemplate::new(format!("{GUIDE_PREFIX}{{topic}}"), "guide")
            .with_description("Guide by topic: quickstart, spec, styles, animation, examples, icons")
            .with_mime_type(MIME_MARKDOWN),
        ResourceTemplate::new(format!("{EXAMPLE_PREFIX}{{name}}"), "example")
            .with_description(format!("Example spec: {}", example_names().join(", ")))
            .with_mime_type(MIME_YAML),
    ]
}

/// Reads one resource.
pub fn read(uri: &str) -> Result<ResourceContents, ResourceError> {
    let text_with = |text: String, mime: &str| Ok(ResourceContents::text(text, uri).with_mime_type(mime));
    if uri == SCHEMA_URI {
        return text_with(to_json(uri, &dioramic_spec::json_schema())?, MIME_JSON);
    }
    if uri == ICON_CATALOG_URI {
        return text_with(to_json(uri, &catalog_summaries())?, MIME_JSON);
    }
    if let Some(topic) = uri.strip_prefix(GUIDE_PREFIX).and_then(GuideTopic::parse) {
        return text_with(topic.markdown().to_owned(), MIME_MARKDOWN);
    }
    if let Some(text) = uri.strip_prefix(EXAMPLE_PREFIX).and_then(example) {
        return text_with(text.to_owned(), MIME_YAML);
    }
    Err(ResourceError::NotFound(uri.to_owned()))
}

/// The whole catalog in summary form.
pub fn catalog_summaries() -> Vec<IconSummary> {
    icons::catalog().iter().map(IconSummary::from).collect()
}

/// An embedded example by name.
pub fn example(name: &str) -> Option<&'static str> {
    EXAMPLES.iter().find(|(candidate, _)| *candidate == name).map(|(_, text)| *text)
}

fn example_names() -> Vec<&'static str> {
    EXAMPLES.iter().map(|(name, _)| *name).collect()
}

fn guide_uri(topic: GuideTopic) -> String {
    format!("{GUIDE_PREFIX}{}", topic.as_str())
}

fn example_uri(name: &str) -> String {
    format!("{EXAMPLE_PREFIX}{name}")
}

fn to_json(uri: &str, value: &impl Serialize) -> Result<String, ResourceError> {
    serde_json::to_string_pretty(value)
        .map_err(|error| ResourceError::Serialize { uri: uri.to_owned(), message: error.to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(contents: &ResourceContents) -> &str {
        match contents {
            ResourceContents::TextResourceContents { text, .. } => text,
            _ => "",
        }
    }

    #[test]
    fn every_listed_resource_can_be_read() {
        for resource in list() {
            let contents = read(&resource.uri).unwrap();
            assert!(!text_of(&contents).is_empty(), "{} is empty", resource.uri);
        }
    }

    #[test]
    fn schema_is_the_spec_v1_schema() {
        let contents = read(SCHEMA_URI).unwrap();
        let schema: serde_json::Value = serde_json::from_str(text_of(&contents)).unwrap();
        assert_eq!(schema["title"], "SpecV1");
    }

    #[test]
    fn examples_are_valid_specs() {
        for (name, text) in EXAMPLES {
            assert!(dioramic_spec::parse_auto(text).is_ok(), "example {name} does not parse");
        }
    }

    #[test]
    fn catalog_lists_aliases() {
        let catalog = catalog_summaries();
        assert!(catalog.iter().any(|icon| icon.id == "database" && icon.aliases.contains(&"postgres")));
    }

    #[test]
    fn unknown_uris_are_not_found() {
        assert!(matches!(read("dioramic://guide/nope"), Err(ResourceError::NotFound(_))));
        assert!(matches!(read("dioramic://examples/nope"), Err(ResourceError::NotFound(_))));
        assert!(matches!(read("file:///etc/passwd"), Err(ResourceError::NotFound(_))));
    }
}
