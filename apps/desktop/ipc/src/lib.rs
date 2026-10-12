//! Data crossing the Tauri IPC boundary between the desktop backend and the Leptos frontend.
//!
//! The frontend never links the engine crates (the v1 renders everything in the backend), so these
//! DTOs mirror the few engine concepts the UI needs with plain serde types. Field names are the JSON
//! wire format: change them together on both sides.

use serde::{Deserialize, Serialize};

/// Names of the events the backend emits.
pub mod events {
    /// The file tree of the open workspace changed (payload: [`super::Workspace`]).
    pub const WORKSPACE_CHANGED: &str = "workspace-changed";
    /// A spec file was written on disk by someone (payload: [`super::SpecChanged`]).
    pub const SPEC_CHANGED: &str = "spec-changed";
    /// Installed icon packs or a project `icons/` folder changed: render again (payload: [`super::IconsChanged`]).
    pub const ICONS_CHANGED: &str = "icons-changed";
}

/// Kind of an entry in the file tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    /// Folder that (transitively) contains specs.
    Folder,
    /// `*.diorama.yaml` / `*.diorama.yml` / `*.diorama.json` file.
    Spec,
}

/// Entry of the workspace file tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileNode {
    /// File or folder name.
    pub name: String,
    /// Absolute path.
    pub path: String,
    /// Folder or spec.
    pub kind: FileKind,
    /// Children (folders first, then specs, each alphabetical). Empty for specs.
    #[serde(default)]
    pub children: Vec<FileNode>,
}

impl FileNode {
    /// Depth-first iterator over this node and its descendants.
    pub fn walk(&self) -> Box<dyn Iterator<Item = &FileNode> + '_> {
        Box::new(std::iter::once(self).chain(self.children.iter().flat_map(FileNode::walk)))
    }
}

/// An open folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    /// Absolute path of the folder.
    pub root: String,
    /// Display name (last path component).
    pub name: String,
    /// Top-level entries.
    pub tree: Vec<FileNode>,
}

impl Workspace {
    /// Number of spec files in the tree.
    pub fn spec_count(&self) -> usize {
        self.tree.iter().flat_map(FileNode::walk).filter(|node| node.kind == FileKind::Spec).count()
    }

    /// First spec in display order, if any.
    pub fn first_spec(&self) -> Option<&FileNode> {
        self.tree.iter().flat_map(FileNode::walk).find(|node| node.kind == FileKind::Spec)
    }
}

/// Payload of [`events::SPEC_CHANGED`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecChanged {
    /// Absolute path of the file that changed.
    pub path: String,
}

/// Payload of [`events::ICONS_CHANGED`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IconsChanged {
    /// Folder whose icons changed: the pack store or a project `icons/` folder.
    pub path: String,
}

/// Camera angle requested by the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewRotation {
    /// 0°.
    Deg0,
    /// 90° clockwise.
    Deg90,
    /// 180°.
    Deg180,
    /// 270° clockwise.
    Deg270,
    /// Let the engine pick the angle with the fewest collisions.
    Auto,
}

impl ViewRotation {
    /// The four fixed angles, clockwise.
    pub const FIXED: [ViewRotation; 4] = [Self::Deg0, Self::Deg90, Self::Deg180, Self::Deg270];

    /// Fixed angle for a number of degrees (`None` unless 0/90/180/270).
    pub fn from_degrees(degrees: u16) -> Option<Self> {
        Self::FIXED.into_iter().find(|rotation| rotation.degrees() == Some(degrees))
    }

    /// Degrees of a fixed angle; `None` for `Auto`.
    pub fn degrees(self) -> Option<u16> {
        match self {
            Self::Deg0 => Some(0),
            Self::Deg90 => Some(90),
            Self::Deg180 => Some(180),
            Self::Deg270 => Some(270),
            Self::Auto => None,
        }
    }

    /// Turns a fixed angle by `quarter_turns` (negative = counter-clockwise).
    pub fn turned(degrees: u16, quarter_turns: i32) -> Self {
        let current = i32::from(degrees / 90);
        let index = (current + quarter_turns).rem_euclid(4);
        Self::FIXED[usize::try_from(index).unwrap_or_default()]
    }

    /// Value as written in the spec (`view.rotation`).
    pub fn spec_value(self) -> String {
        self.degrees().map_or_else(|| "auto".to_owned(), |degrees| degrees.to_string())
    }
}

/// Layout density.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewDensity {
    /// Zones in a line, generous gaps.
    Spaced,
    /// Zones folded in a block.
    Compact,
}

impl ViewDensity {
    /// The other density (toolbar toggle).
    pub fn toggled(self) -> Self {
        match self {
            Self::Spaced => Self::Compact,
            Self::Compact => Self::Spaced,
        }
    }

    /// Value as written in the spec (`layout.density`).
    pub fn spec_value(self) -> &'static str {
        match self {
            Self::Spaced => "spaced",
            Self::Compact => "compact",
        }
    }
}

/// Built-in diagram theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagramTheme {
    /// Light (default).
    Blueprint,
    /// Dark.
    Midnight,
    /// Warm off-white.
    Paper,
    /// Greyscale.
    Mono,
    /// Technical drawing: silver icons, blue rails, semantic edge colors.
    Schematic,
}

impl DiagramTheme {
    /// All themes in menu order.
    pub const ALL: [DiagramTheme; 5] = [Self::Blueprint, Self::Midnight, Self::Paper, Self::Mono, Self::Schematic];

    /// Value as written in the spec.
    pub fn spec_value(self) -> &'static str {
        match self {
            Self::Blueprint => "blueprint",
            Self::Midnight => "midnight",
            Self::Paper => "paper",
            Self::Mono => "mono",
            Self::Schematic => "schematic",
        }
    }

    /// Human label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Blueprint => "Blueprint",
            Self::Midnight => "Midnight",
            Self::Paper => "Paper",
            Self::Mono => "Mono",
            Self::Schematic => "Schematic",
        }
    }
}

/// How edges are drawn (`layout.routing`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewRouting {
    /// Smooth curves (the engine default).
    #[default]
    Curved,
    /// Grid-following paths with rounded corners.
    Orthogonal,
    /// Straight segments.
    Straight,
    /// Isometric rails: straight legs along the plan axes, rounded 90° bends, fewest bends.
    Rails,
}

impl ViewRouting {
    /// Value as written in the spec (`layout.routing`).
    pub fn spec_value(self) -> &'static str {
        match self {
            Self::Curved => "curved",
            Self::Orthogonal => "orthogonal",
            Self::Straight => "straight",
            Self::Rails => "rails",
        }
    }

    /// The toolbar switch: rails ⟷ curves (orthogonal and straight specs switch to rails).
    pub fn toggled(self) -> Self {
        if self == Self::Rails { Self::Curved } else { Self::Rails }
    }
}

/// How nodes and zones are placed (`layout.algorithm`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewLayout {
    /// Zones in flow order, in a row or folded (the engine default of the classic themes).
    #[default]
    Flow,
    /// The busiest nodes in the middle, short aligned edges, packed zones (the `schematic` default).
    Hub,
}

impl ViewLayout {
    /// Value as written in the spec (`layout.algorithm`).
    pub fn spec_value(self) -> &'static str {
        match self {
            Self::Flow => "flow",
            Self::Hub => "hub",
        }
    }

    /// Human label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Flow => "Flow",
            Self::Hub => "Hub",
        }
    }

    /// The toolbar switch: flow ⟷ hub.
    pub fn toggled(self) -> Self {
        match self {
            Self::Flow => Self::Hub,
            Self::Hub => Self::Flow,
        }
    }
}

/// How the background grid is drawn (`view.grid.style`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GridStyle {
    /// Lines along the plan axes (the engine default).
    #[default]
    Lines,
    /// A dot at every intersection.
    Dots,
    /// No grid.
    None,
}

impl GridStyle {
    /// All styles in menu order.
    pub const ALL: [GridStyle; 3] = [Self::Lines, Self::Dots, Self::None];

    /// Value as written in the spec (`view.grid.style`).
    pub fn spec_value(self) -> &'static str {
        match self {
            Self::Lines => "lines",
            Self::Dots => "dots",
            Self::None => "none",
        }
    }

    /// Human label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Lines => "Lines",
            Self::Dots => "Dots",
            Self::None => "None",
        }
    }
}

/// The background grid of the view (`view.grid` without its color, which only the spec sets).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ViewGrid {
    /// Cell side in plan tiles ([`ViewGrid::MIN_SIZE`]..=[`ViewGrid::MAX_SIZE`]).
    pub size: f64,
    /// Lines, dots or none.
    pub style: GridStyle,
    /// Opacity, 0 to 1.
    pub opacity: f64,
}

impl ViewGrid {
    /// Smallest cell side the engine accepts (tiles).
    pub const MIN_SIZE: f64 = 0.25;
    /// Largest cell side the engine accepts (tiles).
    pub const MAX_SIZE: f64 = 4.0;
    /// Default cell side (tiles).
    pub const DEFAULT_SIZE: f64 = 1.0;
    /// Default opacity.
    pub const DEFAULT_OPACITY: f64 = 1.0;

    /// The same grid with the size clamped to the accepted range and rounded to hundredths of a tile.
    pub fn with_size(self, size: f64) -> Self {
        Self { size: round_hundredths(size.clamp(Self::MIN_SIZE, Self::MAX_SIZE)), ..self }
    }

    /// The same grid with the opacity clamped to 0–1 and rounded to hundredths.
    pub fn with_opacity(self, opacity: f64) -> Self {
        Self { opacity: round_hundredths(opacity.clamp(0.0, 1.0)), ..self }
    }
}

impl Default for ViewGrid {
    fn default() -> Self {
        Self { size: Self::DEFAULT_SIZE, style: GridStyle::default(), opacity: Self::DEFAULT_OPACITY }
    }
}

/// Sliders produce long binary fractions; the spec and the pin keep two decimals.
fn round_hundredths(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// View overrides. `None` means "whatever the spec says" so the file is not touched until the user pins.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct ViewState {
    /// Camera override.
    #[serde(default)]
    pub rotation: Option<ViewRotation>,
    /// Density override.
    #[serde(default)]
    pub density: Option<ViewDensity>,
    /// Theme override.
    #[serde(default)]
    pub theme: Option<DiagramTheme>,
    /// Placement override (`layout.algorithm`).
    #[serde(default)]
    pub layout: Option<ViewLayout>,
    /// Edge routing override.
    #[serde(default)]
    pub routing: Option<ViewRouting>,
    /// Background grid override (size, style and opacity; the spec's color is kept).
    #[serde(default)]
    pub grid: Option<ViewGrid>,
}

/// View the engine actually used (with `auto` resolved).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ResolvedView {
    /// Rotation in degrees.
    pub rotation_degrees: u16,
    /// Density.
    pub density: ViewDensity,
    /// Theme.
    pub theme: DiagramTheme,
    /// Placement algorithm.
    #[serde(default)]
    pub layout: ViewLayout,
    /// Edge routing.
    #[serde(default)]
    pub routing: ViewRouting,
    /// Background grid.
    #[serde(default)]
    pub grid: ViewGrid,
}

/// Diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Blocks rendering.
    Error,
    /// Rendered, but suspicious.
    Warning,
    /// Suggestion.
    Hint,
}

/// A spec diagnostic, flattened for the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticView {
    /// Stable code (`E_UNKNOWN_REF`).
    pub code: String,
    /// Severity.
    pub severity: Severity,
    /// Spec path (`nodes[3].icon`); empty for the whole document.
    pub path: String,
    /// Message.
    pub message: String,
    /// Suggestion.
    #[serde(default)]
    pub hint: Option<String>,
    /// 1-based line when the parser knows it.
    #[serde(default)]
    pub line: Option<u32>,
    /// 1-based column when the parser knows it.
    #[serde(default)]
    pub column: Option<u32>,
    /// `data-id` of the SVG element to highlight (node/zone the path points at).
    #[serde(default)]
    pub target_id: Option<String>,
}

/// Hover text of a node (`note` in the spec).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeNote {
    /// Node id (`data-id`).
    pub id: String,
    /// Node label (or id).
    pub title: String,
    /// Note.
    pub note: String,
}

/// Request of the live preview.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewRequest {
    /// Editor text (never read from disk: the editor buffer is the truth).
    pub text: String,
    /// View overrides.
    pub view: ViewState,
    /// Spec being edited; the `icons/` folder next to it provides `local/<name>` icons.
    #[serde(default)]
    pub spec_path: Option<String>,
    /// Preview in motion: an animated SVG (packets and icons move) instead of the static one.
    #[serde(default)]
    pub motion: bool,
}

/// Outcome of a preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PreviewStatus {
    /// `svg` is set.
    Ready,
    /// The spec has errors; see the diagnostics.
    Invalid,
    /// The engine cannot render yet (a stage still being built) or crashed; the UI shows a calm empty state.
    EngineUnavailable {
        /// Why (English, shown to the user).
        reason: String,
    },
}

/// Response of the live preview.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewResponse {
    /// Status.
    pub status: PreviewStatus,
    /// SVG document (text as `<text>` with Inter, so the webview zooms it crisply).
    #[serde(default)]
    pub svg: Option<String>,
    /// Errors, warnings and hints.
    #[serde(default)]
    pub diagnostics: Vec<DiagnosticView>,
    /// View actually rendered.
    #[serde(default)]
    pub view: Option<ResolvedView>,
    /// Node notes for hover tooltips.
    #[serde(default)]
    pub notes: Vec<NodeNote>,
    /// Backend time spent (ms).
    pub elapsed_ms: u32,
}

/// Export format offered by the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    /// PNG raster.
    Png,
    /// Static SVG (text as outlines, portable).
    Svg,
    /// Animated GIF (1600 px, 50 fps).
    Gif,
}

impl ExportFormat {
    /// All formats in menu order.
    pub const ALL: [ExportFormat; 3] = [Self::Png, Self::Svg, Self::Gif];

    /// File extension.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Svg => "svg",
            Self::Gif => "gif",
        }
    }

    /// Human label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Svg => "SVG",
            Self::Gif => "GIF",
        }
    }
}

/// Request of an export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportRequest {
    /// Job id chosen by the UI (used to cancel).
    pub job_id: u32,
    /// Editor text.
    pub text: String,
    /// View overrides.
    pub view: ViewState,
    /// Format.
    pub format: ExportFormat,
    /// PNG pixel density (1–4).
    pub scale: f64,
    /// Spec being exported; its folder and stem suggest the output path.
    #[serde(default)]
    pub spec_path: Option<String>,
}

/// Result of an export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExportOutcome {
    /// File written.
    Saved {
        /// Output path.
        path: String,
        /// Size in bytes.
        bytes: u64,
    },
    /// The user closed the save dialog or pressed cancel.
    Cancelled,
}

/// Request to save the editor text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveRequest {
    /// Spec path.
    pub path: String,
    /// Full editor text (written verbatim: comments and formatting survive).
    pub text: String,
}

/// Request to write the current view into the spec text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PinRequest {
    /// Editor text.
    pub text: String,
    /// View to pin (only the `Some` fields are written).
    pub view: ViewState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_turns_wrap_both_ways() {
        assert_eq!(ViewRotation::turned(0, 1), ViewRotation::Deg90);
        assert_eq!(ViewRotation::turned(0, -1), ViewRotation::Deg270);
        assert_eq!(ViewRotation::turned(270, 1), ViewRotation::Deg0);
        assert_eq!(ViewRotation::from_degrees(180), Some(ViewRotation::Deg180));
        assert_eq!(ViewRotation::from_degrees(45), None);
        assert_eq!(ViewRotation::Auto.spec_value(), "auto");
    }

    #[test]
    fn layout_toggles_between_flow_and_hub() -> Result<(), serde_json::Error> {
        assert_eq!(ViewLayout::Flow.toggled(), ViewLayout::Hub);
        assert_eq!(ViewLayout::Hub.toggled(), ViewLayout::Flow);
        assert_eq!((ViewLayout::Hub.spec_value(), ViewLayout::Hub.label()), ("hub", "Hub"));
        assert_eq!(serde_json::to_value(ViewLayout::Hub)?, "hub");
        Ok(())
    }

    #[test]
    fn routing_toggles_between_rails_and_curves() {
        assert_eq!(ViewRouting::Curved.toggled(), ViewRouting::Rails);
        assert_eq!(ViewRouting::Rails.toggled(), ViewRouting::Curved);
        assert_eq!(ViewRouting::Orthogonal.toggled(), ViewRouting::Rails);
        assert_eq!(ViewRouting::Rails.spec_value(), "rails");
    }

    #[test]
    fn grid_values_are_clamped_and_rounded() {
        let grid = ViewGrid::default().with_size(0.333_333).with_opacity(1.7);
        assert_eq!((grid.size, grid.opacity), (0.33, 1.0));
        assert_eq!(ViewGrid::default().with_size(0.01).size, ViewGrid::MIN_SIZE);
        assert_eq!(ViewGrid::default().with_size(9.0).size, ViewGrid::MAX_SIZE);
        assert_eq!(GridStyle::Dots.spec_value(), "dots");
    }

    #[test]
    fn old_payloads_still_deserialize() -> Result<(), serde_json::Error> {
        let view: ViewState = serde_json::from_str(r#"{"rotation":"deg90"}"#)?;
        assert_eq!((view.layout, view.routing, view.grid), (None, None, None));
        let resolved: ResolvedView =
            serde_json::from_str(r#"{"rotation_degrees":90,"density":"spaced","theme":"paper"}"#)?;
        assert_eq!(
            (resolved.layout, resolved.routing, resolved.grid),
            (ViewLayout::Flow, ViewRouting::Curved, ViewGrid::default())
        );
        let request: PreviewRequest = serde_json::from_str(r#"{"text":"","view":{}}"#)?;
        assert!(!request.motion);
        Ok(())
    }

    #[test]
    fn preview_status_is_tagged_for_the_frontend() -> Result<(), serde_json::Error> {
        let status = PreviewStatus::EngineUnavailable { reason: "soon".into() };
        let json = serde_json::to_string(&status)?;
        assert_eq!(json, r#"{"kind":"engine_unavailable","reason":"soon"}"#);
        Ok(())
    }

    #[test]
    fn workspace_counts_nested_specs() {
        let spec = |name: &str| FileNode {
            name: name.into(),
            path: format!("/w/{name}"),
            kind: FileKind::Spec,
            children: vec![],
        };
        let workspace = Workspace {
            root: "/w".into(),
            name: "w".into(),
            tree: vec![
                FileNode {
                    name: "sub".into(),
                    path: "/w/sub".into(),
                    kind: FileKind::Folder,
                    children: vec![spec("b")],
                },
                spec("a"),
            ],
        };
        assert_eq!(workspace.spec_count(), 2);
        assert_eq!(workspace.first_spec().map(|node| node.name.as_str()), Some("b"));
    }
}
