//! Tool input schemas (`schemars`). Doc comments here are what the model reads: English, short, actionable.

use dioramic_render::{GridRequest, OutputFormat};
use dioramic_spec::{
    AnimationPreset, ColorRef, Density, GridStyle, LayoutAlgorithm, RotationSpec, Routing, SpecV1, ThemeName,
};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::Deserialize;
use serde_json::Value;

use crate::guides::GuideTopic;

/// Default `search_icons.limit`.
pub const DEFAULT_ICON_LIMIT: u32 = 12;

/// The inline `spec` is declared with the schema of [`SpecV1`] but received as raw JSON, so a malformed
/// spec reaches the engine and comes back as `E_*` diagnostics instead of a protocol error.
fn spec_schema(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<SpecV1>()
}

/// Where the spec comes from: exactly one of `spec` / `spec_path`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct SpecInputParams {
    /// Inline DioramaSpec object. Provide either `spec` or `spec_path`.
    #[serde(default)]
    #[schemars(schema_with = "spec_schema")]
    pub spec: Option<Value>,
    /// Path to a `.diorama.yaml` / `.diorama.json` file inside the project (relative to the project root).
    /// Prefer this when you can edit files.
    #[serde(default)]
    pub spec_path: Option<String>,
}

/// Output format of `render_diagram`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FormatParam {
    /// PNG (default).
    #[default]
    Png,
    /// Static SVG (text as outlines).
    Svg,
    /// Animated SVG (CSS/SMIL), a few KB.
    SvgAnimated,
    /// High-quality looping GIF (1600 px, 50 fps).
    Gif,
    /// Not supported yet.
    Apng,
    /// Not supported yet.
    Webp,
    /// Not supported yet (needs ffmpeg).
    Mp4,
    /// Not supported yet (needs ffmpeg).
    Webm,
}

impl FormatParam {
    /// Engine format, or the name of the unsupported format.
    pub fn output_format(self) -> Result<OutputFormat, &'static str> {
        match self {
            Self::Png => Ok(OutputFormat::Png),
            Self::Svg => Ok(OutputFormat::Svg),
            Self::SvgAnimated => Ok(OutputFormat::SvgAnimated),
            Self::Gif => Ok(OutputFormat::Gif),
            Self::Apng => Err("apng"),
            Self::Webp => Err("webp"),
            Self::Mp4 => Err("mp4"),
            Self::Webm => Err("webm"),
        }
    }
}

/// Whether to return the preview image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PreviewParam {
    /// PNG ≤ 1024 px (4-frame filmstrip for animated formats).
    #[default]
    Image,
    /// No image, only the JSON summary.
    None,
}

/// Animation overrides.
#[derive(Debug, Clone, Copy, PartialEq, Default, Deserialize, JsonSchema)]
pub struct AnimationParams {
    /// Preset: flow (default), build, story or pulse.
    #[serde(default)]
    pub preset: Option<AnimationPreset>,
    /// Loop duration in seconds (default 2).
    #[serde(default)]
    pub duration: Option<f64>,
    /// Frames per second for GIF (default 50).
    #[serde(default)]
    pub fps: Option<u32>,
}

/// Background grid overrides (`view.grid`); each omitted field keeps the spec's value.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, JsonSchema)]
pub struct GridParams {
    /// Cell side in plan tiles, 0.25 to 4 (default 1). 0.5 = finer, 2 = coarser.
    #[serde(default)]
    #[schemars(range(min = 0.25, max = 4.0))]
    pub size: Option<f64>,
    /// lines (default), dots or none.
    #[serde(default)]
    pub style: Option<GridStyle>,
    /// Opacity, 0 to 1 (default 1).
    #[serde(default)]
    #[schemars(range(min = 0.0, max = 1.0))]
    pub opacity: Option<f64>,
    /// Color token (blue, slate…) or #rrggbb. Default: the theme's grid color.
    #[serde(default)]
    pub color: Option<ColorRef>,
}

impl From<GridParams> for GridRequest {
    fn from(params: GridParams) -> Self {
        Self { size: params.size, style: params.style, opacity: params.opacity, color: params.color }
    }
}

/// `render_diagram` input.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct RenderDiagramParams {
    /// Spec source.
    #[serde(flatten)]
    pub input: SpecInputParams,
    /// Output format. Default png.
    #[serde(default)]
    pub format: FormatParam,
    /// Output file (inside the project). Default: next to `spec_path`, else `./diagrams/<slug>.<ext>`.
    #[serde(default)]
    pub output_path: Option<String>,
    /// With an inline `spec`, also save it as `<slug>.diorama.yaml` next to the output. Default true.
    #[serde(default)]
    pub save_spec: Option<bool>,
    /// PNG pixel density, 1–4. Default 2.
    #[serde(default)]
    #[schemars(range(min = 1.0, max = 4.0))]
    pub scale: Option<f64>,
    /// Theme override: blueprint, midnight, paper, mono or schematic.
    #[serde(default)]
    pub theme: Option<ThemeName>,
    /// Camera rotation override: 0, 90, 180, 270 or "auto" (fewest collisions; the summary says which).
    #[serde(default)]
    pub rotation: Option<RotationSpec>,
    /// Density override: spaced (default) or compact.
    #[serde(default)]
    pub density: Option<Density>,
    /// Placement override: flow (zones in flow order) or hub (busiest nodes in the middle, short aligned edges).
    /// Default: the spec's `layout.algorithm`, else hub for theme schematic and flow otherwise.
    #[serde(default)]
    pub layout_algorithm: Option<LayoutAlgorithm>,
    /// Edge routing override: curved (default), orthogonal, straight or rails (alias elbow: legs along the iso
    /// axes, rounded 90° bends, fewest bends; default of theme schematic, best with density spaced). Edges with
    /// their own `route` keep it.
    #[serde(default)]
    pub routing: Option<Routing>,
    /// Background grid override: { size, style, opacity, color }.
    #[serde(default)]
    pub grid: Option<GridParams>,
    /// Animation overrides for gif / svg-animated.
    #[serde(default)]
    pub animation: Option<AnimationParams>,
    /// gif / svg-animated: small looping motion inside the icons (LEDs, waves, rings). Default true; false keeps
    /// them still while packets flow.
    #[serde(default)]
    pub icon_animation: Option<bool>,
    /// Return the preview image (default) or not.
    #[serde(default)]
    pub preview: PreviewParam,
}

/// `validate_diagram` input.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ValidateDiagramParams {
    /// Spec source.
    #[serde(flatten)]
    pub input: SpecInputParams,
    /// Theme override.
    #[serde(default)]
    pub theme: Option<ThemeName>,
    /// Camera rotation override (0, 90, 180, 270, "auto").
    #[serde(default)]
    pub rotation: Option<RotationSpec>,
    /// Density override.
    #[serde(default)]
    pub density: Option<Density>,
    /// Placement override (flow, hub).
    #[serde(default)]
    pub layout_algorithm: Option<LayoutAlgorithm>,
    /// Edge routing override (curved, orthogonal, straight, rails; elbow = rails).
    #[serde(default)]
    pub routing: Option<Routing>,
}

/// `search_icons` input.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct SearchIconsParams {
    /// What to look for: "postgres", "message queue", "load balancer". Empty lists the catalog.
    #[serde(default)]
    pub query: String,
    /// Category filter: compute, network, security, data, messaging, cloud, clients, devices, k8s,
    /// observability, generic.
    #[serde(default)]
    pub category: Option<String>,
    /// Pack filter: builtin, aws, gcp, azure, k8s or local (project icons).
    #[serde(default)]
    pub pack: Option<String>,
    /// Maximum results. Default 12.
    #[serde(default)]
    #[schemars(range(min = 1, max = 100))]
    pub limit: Option<u32>,
    /// Also return a contact sheet image of the results. Default false.
    #[serde(default)]
    pub preview: bool,
}

/// `get_guide` input.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
pub struct GetGuideParams {
    /// quickstart, spec, styles, animation, examples or icons.
    pub topic: GuideTopic,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_params_accept_the_documented_shape() {
        let params: RenderDiagramParams = serde_json::from_value(serde_json::json!({
            "spec_path": "diagrams/a.diorama.yaml",
            "format": "svg-animated",
            "rotation": "auto",
            "density": "compact",
            "animation": { "preset": "story", "duration": 3.0 },
            "preview": "none"
        }))
        .unwrap();
        assert_eq!(params.input.spec_path.as_deref(), Some("diagrams/a.diorama.yaml"));
        assert_eq!(params.format, FormatParam::SvgAnimated);
        assert_eq!(params.rotation, Some(RotationSpec::Auto));
        assert_eq!(params.preview, PreviewParam::None);
        assert_eq!(params.animation.and_then(|animation| animation.preset), Some(AnimationPreset::Story));
    }

    #[test]
    fn routing_grid_and_icon_motion_are_accepted() {
        let params: RenderDiagramParams = serde_json::from_value(serde_json::json!({
            "spec_path": "a.diorama.yaml",
            "format": "gif",
            "routing": "rails",
            "grid": { "size": 0.5, "style": "dots", "opacity": 0.6, "color": "slate" },
            "icon_animation": false
        }))
        .unwrap();
        assert_eq!(params.routing, Some(Routing::Rails));
        assert_eq!(params.icon_animation, Some(false));
        let grid = GridRequest::from(params.grid.unwrap());
        assert_eq!((grid.size, grid.style, grid.opacity), (Some(0.5), Some(GridStyle::Dots), Some(0.6)));
        assert!(grid.color.is_some());
        let schema = serde_json::to_value(schemars::schema_for!(RenderDiagramParams)).unwrap();
        for property in ["routing", "grid", "icon_animation"] {
            assert!(schema["properties"][property].is_object(), "{property} missing from the schema");
        }
    }

    #[test]
    fn layout_algorithm_and_the_elbow_alias_are_accepted() {
        let params: RenderDiagramParams = serde_json::from_value(serde_json::json!({
            "spec_path": "a.diorama.yaml",
            "layout_algorithm": "hub",
            "routing": "elbow"
        }))
        .unwrap();
        assert_eq!((params.layout_algorithm, params.routing), (Some(LayoutAlgorithm::Hub), Some(Routing::Rails)));
        let validate: ValidateDiagramParams =
            serde_json::from_value(serde_json::json!({ "spec_path": "a.diorama.yaml", "layout_algorithm": "flow" }))
                .unwrap();
        assert_eq!(validate.layout_algorithm, Some(LayoutAlgorithm::Flow));
        let schema = serde_json::to_value(schemars::schema_for!(RenderDiagramParams)).unwrap();
        assert!(schema["properties"]["layout_algorithm"].is_object(), "layout_algorithm missing from the schema");
    }

    #[test]
    fn inline_spec_is_kept_raw() {
        let params: RenderDiagramParams =
            serde_json::from_value(serde_json::json!({ "spec": { "diorama": 1, "nodes": [], "bogus": true } }))
                .unwrap();
        assert_eq!(params.input.spec.as_ref().and_then(|spec| spec.get("bogus")), Some(&Value::Bool(true)));
    }

    #[test]
    fn unsupported_formats_are_named() {
        assert_eq!(FormatParam::Mp4.output_format(), Err("mp4"));
        assert_eq!(FormatParam::Gif.output_format(), Ok(OutputFormat::Gif));
        assert_eq!(FormatParam::default().output_format(), Ok(OutputFormat::Png));
    }

    #[test]
    fn spec_property_uses_the_spec_v1_schema() {
        let schema = serde_json::to_value(schemars::schema_for!(RenderDiagramParams)).unwrap();
        assert!(schema["properties"]["spec"]["$ref"].as_str().is_some_and(|reference| reference.ends_with("SpecV1")));
        let required = schema["required"].as_array().cloned().unwrap_or_default();
        assert!(required.is_empty(), "no parameter is required: {required:?}");
    }
}
