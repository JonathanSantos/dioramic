//! Diorama spec format (`*.diorama.yaml` / JSON), version 1.
//!
//! Pipeline owned by this crate (see `docs/03-arquitetura.md`):
//! `parse` (text → [`SpecV1`]) → `normalize` ([`SpecV1`] → [`Model`]) → `validate` ([`Model`] → [`Diagnostics`]).
//!
//! This crate knows nothing about icons, layout or rendering. Icon validation lives in `dioramic-core`.
//! Contract: `docs/work/contracts/C-001-spec-model-diagnostics.md`.

mod common;
mod diagnostic;
mod model;
mod normalize;
mod parse;
mod raw;
mod shorthand;
mod validate;

pub use common::{
    Arrange, ArrowEnds, BorderStyle, ColorRef, ColorToken, Density, EdgeColors, EdgeKind, FlowAxis, GridStyle,
    HexColor, LayoutAlgorithm, LegendMode, NodeSize, Rotation, RotationSpec, Routing, ThemeName, ValueSource, Wrap,
};
pub use diagnostic::{Diagnostic, DiagnosticCode, Diagnostics, Severity, SourceLocation};
pub use model::{
    Animation, AnimationPreset, AnimationStep, Edge, EdgeId, EdgeStyle, FlowMode, GridOptions, LayoutOptions, Model,
    Node, NodeId, PlanCoord, StepEffect, ViewOptions, Zone, ZoneId,
};
pub use normalize::normalize;
pub use parse::{SourceFormat, parse, parse_auto};
pub use raw::{
    AnimationSpec, AnimationStepSpec, DurationSpec, EdgeDetails, EdgeEntry, EdgeKeyedValue, EdgeSpec, EdgeStyleSpec,
    FlowDirection, FlowSpec, GridSpec, LayoutSpec, NodeSpec, SPEC_VERSION, SpecV1, ViewSpec, ZoneSpec,
};
pub use shorthand::{EdgeOp, EdgeShorthand, ShorthandError, parse_edge_shorthand};
pub use validate::{MAX_NODES, RECOMMENDED_MAX_LABEL_CHARS, RECOMMENDED_MAX_NODES, Validated, load, validate};

/// JSON Schema of [`SpecV1`], used by the MCP tool schema and `dioramic://schema/v1`.
pub fn json_schema() -> schemars::Schema {
    schemars::schema_for!(SpecV1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_generated() {
        let schema = serde_json::to_value(json_schema()).unwrap_or_default();
        assert_eq!(schema["title"], "SpecV1");
        assert!(schema["properties"]["edges"].is_object());
    }

    #[test]
    fn raw_types_accept_the_three_edge_shapes() {
        let json = r#"{
            "diorama": 1,
            "view": { "rotation": "auto" },
            "layout": { "density": "compact", "wrap": 2 },
            "nodes": [{ "id": "a" }, { "id": "b", "at": [1, 2.5] }],
            "edges": [
                "a -> b: HTTPS @primary",
                { "from": "a", "to": "b", "flow": "reverse" },
                { "a -> b": "1Gbps trunk @trunk" },
                { "a -> b": { "label": "x", "flow": true } },
                { "a -> b": 443 },
                { "a -> b": null }
            ]
        }"#;
        let spec: SpecV1 = match serde_json::from_str(json) {
            Ok(spec) => spec,
            Err(error) => panic!("{error}"),
        };
        assert_eq!(spec.view.and_then(|view| view.rotation), Some(RotationSpec::Auto));
        assert!(matches!(spec.edges[0], EdgeEntry::Shorthand(_)));
        assert!(matches!(spec.edges[1], EdgeEntry::Long(_)));
        assert!(spec.edges[2..].iter().all(|edge| matches!(edge, EdgeEntry::Keyed(_))));
    }

    #[test]
    fn yaml_unquoted_shorthand_is_a_single_key_map() {
        let yaml = "diorama: 1\nview: { rotation: 90 }\nnodes:\n  - { id: fw }\n  - { id: core }\nedges:\n  - fw -> core: 1Gbps trunk @trunk\n  - \"core .. fw\"\n  - core -> fw\n";
        let spec: SpecV1 = match serde_saphyr::from_str(yaml) {
            Ok(spec) => spec,
            Err(error) => panic!("{error}"),
        };
        assert_eq!(spec.view.and_then(|view| view.rotation), Some(RotationSpec::Fixed(Rotation::R90)));
        assert!(matches!(spec.edges[0], EdgeEntry::Keyed(_)));
        assert!(matches!(spec.edges[1], EdgeEntry::Shorthand(_)));
        assert!(matches!(spec.edges[2], EdgeEntry::Shorthand(_)));
    }
}
