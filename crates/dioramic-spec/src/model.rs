//! Normalized model: shorthand expanded, defaults applied, ids typed. Input of `dioramic-core`.
//!
//! Icons are kept as written (`Option<String>`): resolving ids/aliases and inferring from the node id
//! is done by `dioramic_core::icons`, because this crate does not know the catalog.

use std::fmt;

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::common::{
    Arrange, ArrowEnds, BorderStyle, ColorRef, Density, EdgeColors, EdgeKind, FlowAxis, GridStyle, LayoutAlgorithm,
    LegendMode, NodeSize, RotationSpec, Routing, ThemeName, ValueSource, Wrap,
};

macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            /// The id as a string slice.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(text: &str) -> Self {
                Self(text.to_owned())
            }
        }
    };
}

string_id!(
    /// Node id (`snake_case`).
    NodeId
);
string_id!(
    /// Zone id (`snake_case`).
    ZoneId
);
string_id!(
    /// Edge id (default `"from->to"`, with `#2`, `#3`… appended for parallel edges).
    EdgeId
);

/// A point on the floor plan in tiles, as written in `at: [x, y]` (rotation 0 world axes).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanCoord {
    /// Plan x (screen: down-right at rotation 0).
    pub x: f64,
    /// Plan y (screen: down-left at rotation 0).
    pub y: f64,
}

/// Fully normalized diagram.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Model {
    /// Title.
    pub title: Option<String>,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Theme (default `blueprint`).
    pub theme: ThemeName,
    /// Layout options with defaults applied.
    pub layout: LayoutOptions,
    /// View options with defaults applied.
    pub view: ViewOptions,
    /// User-defined edge styles by name (theme styles `primary`, `secondary`, `muted`, `danger`, `success` are implicit).
    pub styles: IndexMap<String, EdgeStyle>,
    /// Zones in spec order.
    pub zones: Vec<Zone>,
    /// Nodes in spec order.
    pub nodes: Vec<Node>,
    /// Edges in spec order.
    pub edges: Vec<Edge>,
    /// Animation, if requested in the spec.
    pub animation: Option<Animation>,
}

impl Model {
    /// Finds a node by id.
    pub fn node(&self, id: &NodeId) -> Option<&Node> {
        self.nodes.iter().find(|node| &node.id == id)
    }

    /// Finds a zone by id.
    pub fn zone(&self, id: &ZoneId) -> Option<&Zone> {
        self.zones.iter().find(|zone| &zone.id == id)
    }
}

/// `layout` with defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct LayoutOptions {
    /// Flow axis.
    pub flow: FlowAxis,
    /// Density.
    pub density: Density,
    /// Zones per row.
    pub wrap: Wrap,
    /// Default routing.
    pub routing: Routing,
    /// Whether `routing` was written (or overridden) or is the default; a theme may replace the default.
    #[serde(default)]
    pub routing_source: ValueSource,
    /// Edge coloring; `None` = the theme decides (`semantic` in `schematic`, `style` elsewhere).
    #[serde(default)]
    pub edge_colors: Option<EdgeColors>,
    /// Placement algorithm (T-411).
    #[serde(default, skip_serializing_if = "LayoutAlgorithm::is_default")]
    pub algorithm: LayoutAlgorithm,
    /// Whether `algorithm` was written or is the default; a theme may replace the default.
    #[serde(default, skip_serializing_if = "ValueSource::is_default")]
    pub algorithm_source: ValueSource,
}

/// `view` with defaults.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct ViewOptions {
    /// Camera rotation.
    pub rotation: RotationSpec,
    /// Background grid.
    #[serde(default)]
    pub grid: GridOptions,
    /// Edge legend.
    #[serde(default)]
    pub legend: LegendMode,
}

/// `view.grid` with defaults (the defaults draw the grid of v1: lines every tile, theme color).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GridOptions {
    /// Side of a cell in plan tiles.
    pub size: f64,
    /// Lines, dots or none.
    pub style: GridStyle,
    /// Opacity, 0 to 1.
    pub opacity: f64,
    /// Color; `None` = the theme's grid color.
    pub color: Option<ColorRef>,
}

impl GridOptions {
    /// Default cell side (tiles).
    pub const DEFAULT_SIZE: f64 = 1.0;
    /// Smallest accepted cell side (tiles): finer grids turn into noise and heavy SVG.
    pub const MIN_SIZE: f64 = 0.25;
    /// Largest accepted cell side (tiles).
    pub const MAX_SIZE: f64 = 4.0;
}

impl Default for GridOptions {
    fn default() -> Self {
        Self { size: Self::DEFAULT_SIZE, style: GridStyle::default(), opacity: 1.0, color: None }
    }
}

/// A named edge style (all fields optional: unset ones fall back to the theme's `primary`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EdgeStyle {
    /// Color.
    pub color: Option<ColorRef>,
    /// Stroke width (px).
    pub width: Option<f64>,
    /// Dashed.
    pub dashed: Option<bool>,
}

/// A zone with defaults applied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Zone {
    /// Id.
    pub id: ZoneId,
    /// Label (None = no pill).
    pub label: Option<String>,
    /// Color (None = palette rotation decided by the theme, by zone index).
    pub color: Option<ColorRef>,
    /// Parent zone.
    pub parent: Option<ZoneId>,
    /// Arrangement.
    pub arrange: Arrange,
    /// Pinned top corner.
    pub at: Option<PlanCoord>,
    /// Forced size `[width, depth]` in tiles.
    pub size: Option<[f64; 2]>,
    /// Border.
    pub border: BorderStyle,
    /// Spec path (`zones[2]`) for diagnostics.
    pub path: String,
}

/// A node with defaults applied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Node {
    /// Id.
    pub id: NodeId,
    /// Icon id or alias as written (None → inferred from id by the core, else `block`).
    pub icon: Option<String>,
    /// Label.
    pub label: Option<String>,
    /// Zone.
    pub zone: Option<ZoneId>,
    /// Tint.
    pub color: Option<ColorRef>,
    /// Replicas (≥ 1).
    pub replicas: u32,
    /// Badge text.
    pub badge: Option<String>,
    /// Pinned position.
    pub at: Option<PlanCoord>,
    /// Elevation in tiles.
    pub z: f64,
    /// Icon scale.
    pub size: NodeSize,
    /// Tooltip.
    pub note: Option<String>,
    /// Spec path (`nodes[3]`) for diagnostics.
    pub path: String,
}

/// Traffic animation mode of an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FlowMode {
    /// Not animated by the `flow` preset unless no edge opts in (then all are).
    #[default]
    Off,
    /// Packets from → to.
    Forward,
    /// Packets to → from.
    Reverse,
}

/// An edge with shorthand expanded and defaults applied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Edge {
    /// Id (`"from->to"` by default).
    pub id: EdgeId,
    /// Source.
    pub from: NodeId,
    /// Target.
    pub to: NodeId,
    /// Pill label.
    pub label: Option<String>,
    /// Style name (default `primary`).
    pub style: String,
    /// Whether `style` was written or is the default (`primary`): only unstyled edges take semantic colors.
    #[serde(default)]
    pub style_source: ValueSource,
    /// Connection class written in the spec; `None` = inferred by the renderer.
    #[serde(default)]
    pub kind: Option<EdgeKind>,
    /// Color override.
    pub color: Option<ColorRef>,
    /// Width override.
    pub width: Option<f64>,
    /// Dashed (from operator `..>`/`..`, style or override). `None` = style decides.
    pub dashed: Option<bool>,
    /// Arrow ends.
    pub arrow: ArrowEnds,
    /// Routing (edge override or `layout.routing`).
    pub route: Routing,
    /// Whether `route` was written on the edge or inherited from `layout.routing`.
    #[serde(default)]
    pub route_source: ValueSource,
    /// Traffic animation.
    pub flow: FlowMode,
    /// Spec path (`edges[4]`) for diagnostics.
    pub path: String,
}

/// Animation preset (`docs/07-animacao.md#presets`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AnimationPreset {
    /// Dashes and packets moving along edges (loop).
    #[default]
    Flow,
    /// Zones appear, nodes drop in, edges draw, labels fade in.
    Build,
    /// Step by step highlighted path with captions.
    Story,
    /// Pulsing ring on marked nodes.
    Pulse,
}

/// Effect applied to the nodes of a story step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum StepEffect {
    /// Pulsing ring.
    Pulse,
    /// Highlight (rest dimmed).
    Highlight,
}

/// Normalized animation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Animation {
    /// Preset.
    pub preset: AnimationPreset,
    /// Raster frames per second (default 50).
    pub fps: u32,
    /// Loop duration in seconds (default 2.0).
    pub duration_s: f64,
    /// Loops.
    pub looped: bool,
    /// Icons move a little (LEDs blinking, waves, rings) while the diagram animates (default true).
    #[serde(default = "icons_move_by_default")]
    pub icons: bool,
    /// Story steps (`path` already expanded to edge ids).
    pub steps: Vec<AnimationStep>,
}

fn icons_move_by_default() -> bool {
    true
}

/// Normalized story step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AnimationStep {
    /// Edges highlighted in this step (from `path` and `edges`).
    pub edges: Vec<EdgeId>,
    /// Nodes highlighted in this step (from `path` and `nodes`).
    pub nodes: Vec<NodeId>,
    /// Effect.
    pub effect: Option<StepEffect>,
    /// Caption.
    pub caption: Option<String>,
    /// Duration in seconds (default 2.0).
    pub duration_s: f64,
}
