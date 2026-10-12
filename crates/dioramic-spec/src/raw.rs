//! Raw spec as written by the AI or a human (`docs/04-spec.md`). Mirrors the file 1:1, all optional
//! fields stay `Option`; defaults are applied by [`crate::normalize`].
//!
//! The JSON Schema of [`SpecV1`] is what the MCP tool exposes, so doc comments here are user-facing (English).

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::common::{
    Arrange, ArrowEnds, BorderStyle, ColorRef, Density, EdgeColors, EdgeKind, FlowAxis, GridStyle, LayoutAlgorithm,
    LegendMode, NodeSize, RotationSpec, Routing, ThemeName, Wrap,
};

/// The only spec version this crate understands.
pub const SPEC_VERSION: u32 = 1;

/// A Diorama spec, version 1: an isometric architecture diagram.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpecV1 {
    /// Spec version. Must be `1`.
    pub diorama: u32,
    /// Title drawn in the top-left corner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Subtitle under the title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// Visual theme. Default `blueprint`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<ThemeName>,
    /// Global layout hints (density, flow axis, wrapping, routing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<LayoutSpec>,
    /// Camera options (rotation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<ViewSpec>,
    /// Named edge styles, referenced as `@name` in edge shorthand or `style:` in long form.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub styles: IndexMap<String, EdgeStyleSpec>,
    /// Areas on the floor that group nodes (network, room, VPC, cluster, tier).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zones: Vec<ZoneSpec>,
    /// Components. At least one.
    pub nodes: Vec<NodeSpec>,
    /// Connections. Shorthand string `"a -> b: label @style"`, single-key map, or long form object.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<EdgeEntry>,
    /// Animation (GIF / animated SVG).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<AnimationSpec>,
}

/// `layout:` block.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayoutSpec {
    /// Main flow axis. Default `x`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<FlowAxis>,
    /// `spaced` (default, zones in a line) or `compact` (zones folded in a block).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub density: Option<Density>,
    /// Zones per row before folding: `auto` or an integer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrap: Option<Wrap>,
    /// Default edge routing. Default `curved` (`rails` in the `schematic` theme).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing: Option<Routing>,
    /// Edge coloring: `semantic` (by connection class: data, cache, messaging…), `style` (by style name) or
    /// `mono`. Default `semantic` in the `schematic` theme, `style` in the others.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_colors: Option<EdgeColors>,
    /// Placement: `flow` (default; zones in flow order, in a row or folded) or `hub` (the most connected places
    /// in the middle, short aligned edges, zones packed tight).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub algorithm: Option<LayoutAlgorithm>,
}

/// `view:` block.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewSpec {
    /// Camera rotation: 0, 90, 180, 270 or `auto`. Default: `auto` when `layout.density: compact`, else 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<RotationSpec>,
    /// Background grid: cell size, style, opacity and color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<GridSpec>,
    /// Edge legend with semantic colors: `auto` (default, when two or more classes are in use), `on` or `off`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legend: Option<LegendMode>,
}

/// `view.grid` block.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GridSpec {
    /// Side of a grid cell in plan tiles (0.25 to 4). Default 1 (one cell per tile).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    /// `lines` (default), `dots` or `none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<GridStyle>,
    /// Opacity from 0 to 1. Default 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    /// Color token or hex. Default: the theme's grid color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorRef>,
}

/// A named edge style.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EdgeStyleSpec {
    /// Color token (`orange`) or hex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorRef>,
    /// Stroke width in px (default 2.6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    /// Dashed line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashed: Option<bool>,
}

/// An area on the floor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ZoneSpec {
    /// Unique snake_case id.
    pub id: String,
    /// Text of the label lying on the floor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Color token or hex. Default: palette rotation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorRef>,
    /// Parent zone id (nested zone).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// How nodes are distributed inside the zone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrange: Option<Arrange>,
    /// Pin the zone's top corner at `[x, y]` tiles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<[f64; 2]>,
    /// Force the size `[width, depth]` in tiles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<[f64; 2]>,
    /// `dashed` (default), `solid` or `none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<BorderStyle>,
}

/// A component.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NodeSpec {
    /// Unique snake_case id.
    pub id: String,
    /// Icon id from the catalog (`server`, `postgres`, `k8s/pod`…). Inferred from the id, else `block`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Label (≤ 28 chars; longer wraps to 2 lines).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Zone id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
    /// Tint for procedural icons.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorRef>,
    /// Draw N stacked copies and a `×N` badge. Default 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replicas: Option<u32>,
    /// Small tag in the corner (`v2`, `!`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    /// Pin the position at `[x, y]` tiles (half tiles allowed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<[f64; 2]>,
    /// Elevation in tiles. Default 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z: Option<f64>,
    /// Icon scale `s | m | l`. Default `m`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<NodeSize>,
    /// Tooltip (desktop app / HTML export only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// One entry of `edges:`. Three accepted shapes (see `docs/04-spec.md#edge`):
///
/// 1. shorthand string: `"fw -> core: 1Gbps trunk @trunk"`;
/// 2. single-key map (what YAML produces from an unquoted shorthand): `{ "fw -> core": "1Gbps trunk @trunk" }`
///    or `{ "fw -> core": { label: ..., style: ..., flow: true } }`;
/// 3. long form object with `from` / `to`.
///
/// Variant order matters for `untagged`: `Long` is tried before `Keyed` (a long form always has `from`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum EdgeEntry {
    /// `"a -> b: label @style"`.
    Shorthand(String),
    /// Long form with `from` / `to`.
    Long(EdgeSpec),
    /// `{ "a -> b": <label or details> }`. Must have exactly one key (checked by normalize).
    Keyed(IndexMap<String, Option<EdgeKeyedValue>>),
}

/// Value of a single-key edge map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum EdgeKeyedValue {
    /// Label text, optionally followed by `@style`.
    Text(String),
    /// Unquoted numeric label in YAML (`a -> b: 443`).
    Number(f64),
    /// Extra fields.
    Details(EdgeDetails),
}

/// Fields allowed in the value of a single-key edge map.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EdgeDetails {
    /// Edge id (default `"from->to"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Pill label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Style name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    /// Connection class (data, cache, messaging, external, client, service, observability). Default: inferred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<EdgeKind>,
    /// Color override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorRef>,
    /// Width override (px).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    /// Dashed override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashed: Option<bool>,
    /// Arrow ends override (the operator already sets one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrow: Option<ArrowEnds>,
    /// Routing override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<Routing>,
    /// Traffic animation on this edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<FlowSpec>,
}

/// Long form edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EdgeSpec {
    /// Edge id (default `"from->to"`), used by `animation.steps`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Source node id.
    pub from: String,
    /// Target node id.
    pub to: String,
    /// Pill label in the middle of the edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Style name (from `styles` or theme: primary, secondary, muted, danger, success). Default `primary`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    /// Connection class (data, cache, messaging, external, client, service, observability). Default: inferred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<EdgeKind>,
    /// Color override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorRef>,
    /// Width override (px).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    /// Dashed override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashed: Option<bool>,
    /// Arrow ends. Default `end`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrow: Option<ArrowEnds>,
    /// Routing. Default `layout.routing`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<Routing>,
    /// Traffic animation: `true`, `false` or `reverse`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<FlowSpec>,
}

/// `flow:` value: boolean or `"reverse"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum FlowSpec {
    /// `true` / `false`.
    Enabled(bool),
    /// `forward` / `reverse`.
    Direction(FlowDirection),
}

/// Named flow direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FlowDirection {
    /// Packets travel from → to.
    Forward,
    /// Packets travel to → from.
    Reverse,
}

/// `animation:` block (`docs/07-animacao.md`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnimationSpec {
    /// `flow` (default), `build`, `story` or `pulse`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<crate::model::AnimationPreset>,
    /// Frames per second for raster output (GIF default 50).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fps: Option<u32>,
    /// Loop duration: seconds (`2`) or text (`"2.5s"`, `"800ms"`). Default 2 s.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<DurationSpec>,
    /// Loop forever. Default true for `flow`/`pulse`.
    #[serde(default, rename = "loop", skip_serializing_if = "Option::is_none")]
    pub looped: Option<bool>,
    /// Small looping motion inside the icons (LEDs, waves, rings). Default true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icons: Option<bool>,
    /// Steps of the `story` preset.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<AnimationStepSpec>,
}

/// One step of a `story` animation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnimationStepSpec {
    /// Sequence of node ids; becomes the consecutive edges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,
    /// Edge ids (`"api1->cache"`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<String>,
    /// Node ids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<String>,
    /// Effect on the nodes (`pulse`, `highlight`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<crate::model::StepEffect>,
    /// Caption shown under the diagram.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    /// Step duration (default 2 s).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<DurationSpec>,
}

/// A duration: number of seconds, or text with unit (`"2.5s"`, `"800ms"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum DurationSpec {
    /// Seconds.
    Seconds(f64),
    /// `"2.5s"` / `"800ms"`; parsed by normalize.
    Text(String),
}
