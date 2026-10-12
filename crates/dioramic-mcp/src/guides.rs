//! Short markdown guides embedded in the binary (`get_guide`, `dioramic://guide/{topic}`).
//! Derived from `docs/04-spec.md`, `docs/06-renderizacao.md` and `docs/07-animacao.md`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Topics of `get_guide`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GuideTopic {
    /// Workflow and a minimal spec.
    Quickstart,
    /// Full spec reference.
    Spec,
    /// Themes, color tokens and edge styles.
    Styles,
    /// Animation presets and formats.
    Animation,
    /// Complete example specs.
    Examples,
    /// Icon catalog.
    Icons,
}

impl GuideTopic {
    /// Every topic, in documentation order.
    pub const ALL: [GuideTopic; 6] =
        [Self::Quickstart, Self::Spec, Self::Styles, Self::Animation, Self::Examples, Self::Icons];

    /// Stable lowercase name (`spec`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Quickstart => "quickstart",
            Self::Spec => "spec",
            Self::Styles => "styles",
            Self::Animation => "animation",
            Self::Examples => "examples",
            Self::Icons => "icons",
        }
    }

    /// Parses [`GuideTopic::as_str`] (case-insensitive).
    pub fn parse(text: &str) -> Option<Self> {
        let wanted = text.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|topic| topic.as_str() == wanted)
    }

    /// The guide in markdown.
    pub fn markdown(self) -> &'static str {
        match self {
            Self::Quickstart => include_str!("../guides/quickstart.md"),
            Self::Spec => include_str!("../guides/spec.md"),
            Self::Styles => include_str!("../guides/styles.md"),
            Self::Animation => include_str!("../guides/animation.md"),
            Self::Examples => include_str!("../guides/examples.md"),
            Self::Icons => include_str!("../guides/icons.md"),
        }
    }

    /// One-line description (resource listing).
    pub fn summary(self) -> &'static str {
        match self {
            Self::Quickstart => "Workflow (write, render, look, fix) and a minimal spec",
            Self::Spec => "DioramaSpec v1 reference: nodes, zones, edge shorthand, layout, view, diagnostics",
            Self::Styles => "Themes, color tokens and named edge styles",
            Self::Animation => "Animation presets (flow, build, story, pulse) and animated formats",
            Self::Examples => "Complete example specs",
            Self::Icons => "Icon catalog by category, aliases and fallbacks",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_topic_round_trips_and_has_content() {
        for topic in GuideTopic::ALL {
            assert_eq!(GuideTopic::parse(topic.as_str()), Some(topic));
            assert!(topic.markdown().starts_with("# "), "{topic:?} guide must start with a heading");
        }
    }

    #[test]
    fn parse_is_case_insensitive_and_rejects_unknown() {
        assert_eq!(GuideTopic::parse(" Spec "), Some(GuideTopic::Spec));
        assert_eq!(GuideTopic::parse("layout"), None);
    }

    #[test]
    fn guides_stay_short() {
        // "Short and direct" (docs/05-mcp.md): each guide costs context every time the model reads it.
        const MAX_GUIDE_CHARS: usize = 6_000;
        for topic in GuideTopic::ALL {
            assert!(topic.markdown().len() <= MAX_GUIDE_CHARS, "{topic:?} guide is too long");
        }
    }

    #[test]
    fn serde_uses_lowercase_names() {
        let topic: GuideTopic = serde_json::from_str("\"animation\"").unwrap_or(GuideTopic::Quickstart);
        assert_eq!(topic, GuideTopic::Animation);
    }
}
