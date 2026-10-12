//! Scene: the projected, collision-resolved diagram, ready to be emitted (`docs/06-renderizacao.md`).
//! Contract: `docs/work/contracts/C-005-scene-svg.md`. Implementation: T-104.
//!
//! Computation order ≠ paint order: nodes → node labels → edges and pills → zone labels, each step
//! avoiding what was already placed (greedy). The residual overlap area is the scene score, used by
//! `rotation: auto` (4 angles, geometry only, lowest score wins; ties → fewer misread edges, 0°, smallest
//! viewBox area).
//!
//! Arrows (`docs/06-renderizacao.md#setas-e-portas`, ADR-012): the arrival port is a visible point of the
//! target footprint (slides to the visible side corner when the natural edge is behind a tall node), the
//! head points at the target center, the curve arrives tangent to that direction, and the head is a
//! triangle built on the floor and projected (never an SVG `marker`).

mod collision;
mod crossings;
mod edges;
mod elbow;
pub(crate) mod framing;
pub(crate) mod labels;
pub mod legend;
mod lint;
mod midway;
mod nodes;
pub(crate) mod ports;
mod rails;
mod routing;
mod visibility;

use dioramic_spec::{
    BorderStyle, Density, Diagnostics, EdgeId, EdgeKind, FlowMode, GridOptions, GridStyle, Model, NodeId, Rotation,
    RotationSpec, ThemeName, ZoneId,
};

use crate::canvas::{ItemRole, SvgDef};
use crate::color::Color;
use crate::geometry::{
    Affine, ArcLengthTable, CubicBezier, DEFAULT_TILE_PX, PlanPoint, PlanRect, Projection, Rect, ScreenPoint,
    ScreenVector, rotate_about,
};
use crate::icons::{self, IconRegistry, NodeShape};
use crate::layout::{self, PlanLayout};
use crate::theme::{ArrowPosition, Theme, ZoneColors};

pub use crossings::{CrossingPair, EdgeCrossings, crossing_pairs, edge_crossings};
pub use legend::{LegendEntry, SceneLegend};
use lint::EdgeReadability;

/// Margin (px) under which two boxes count as touching in the collision score (prototype `m = 3`).
pub const COLLISION_MARGIN_PX: f64 = 3.0;
/// Arrow head length on the floor (tiles).
pub const ARROW_LENGTH: f64 = 0.3;
/// Arrow head half width on the floor (tiles).
pub const ARROW_HALF_WIDTH: f64 = 0.13;
/// Height of edge strokes above the floor (tiles), so they sit on top of zones.
pub const EDGE_Z: f64 = 0.04;
/// One lane (tiles): the gap kept between parallel rails of different edges sharing a corridor, and under
/// which two parallel strokes read as glued together (T-407).
pub const RAIL_LANE: f64 = 0.3;
/// Maximum angle error (degrees) between an arrow head and its target center / curve tangent.
pub const ARROW_ANGLE_TOLERANCE_DEG: f64 = 1.0;
/// Segments of the arc-length table used to move along curves.
const ARC_SAMPLES: usize = 200;

/// Options of a scene build.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneOptions {
    /// Theme.
    pub theme: ThemeName,
    /// Camera rotation (fixed or auto).
    pub rotation: RotationSpec,
    /// Density.
    pub density: Density,
    /// Tile size (px).
    pub tile_px: f64,
    /// Margin around the content (px).
    pub padding_px: f64,
    /// Space reserved above the content for title/subtitle (px), when there is a title.
    pub title_space_px: f64,
}

impl SceneOptions {
    /// Options taken from the model (`theme`, `view.rotation`, `layout.density`) with default sizes.
    pub fn from_model(model: &Model) -> Self {
        Self {
            theme: model.theme,
            rotation: model.view.rotation,
            density: model.layout.density,
            tile_px: DEFAULT_TILE_PX,
            padding_px: 60.0,
            title_space_px: 130.0,
        }
    }
}

/// Collision score of each rotation tried by `auto` (px², lower is better).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RotationScores {
    /// `(rotation, score)` for 0, 90, 180, 270.
    pub scores: [(Rotation, f64); 4],
}

/// Reference to a placed item, for collision reports and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ItemRef {
    /// Role (node, node-label, edge-label, zone-label, edge).
    pub role: ItemRole,
    /// Spec id.
    pub id: String,
}

/// One residual overlap.
#[derive(Debug, Clone, PartialEq)]
pub struct Overlap {
    /// First item.
    pub a: ItemRef,
    /// Second item.
    pub b: ItemRef,
    /// Overlap area (px², with [`COLLISION_MARGIN_PX`]).
    pub area: f64,
}

/// Residual collisions of the scene. `score == 0` means nothing overlaps.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CollisionReport {
    /// Total residual area (px²).
    pub score: f64,
    /// Individual overlaps (for `W_LABEL_OVERLAP` and tests).
    pub overlaps: Vec<Overlap>,
}

/// Kind of placed label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LabelKind {
    /// White rounded box above a node with a vertical leader line.
    Node,
    /// Dark pill on an edge.
    Edge,
    /// Pill lying on the floor at a zone's front border.
    Zone,
}

/// A label after collision resolution.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedLabel {
    /// Kind.
    pub kind: LabelKind,
    /// Lines (1 or 2).
    pub lines: Vec<String>,
    /// Screen bounding box (for zone labels: bbox of the transformed pill).
    pub rect: Rect,
    /// Local box size `(width, height)` in px before `transform`.
    pub size: (f64, f64),
    /// Floor transform for zone labels (`None` for screen-aligned labels).
    pub transform: Option<Affine>,
}

/// A zone in screen space.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneZone {
    /// Id.
    pub id: ZoneId,
    /// Floor polygon (back, right, front, left corners).
    pub polygon: [ScreenPoint; 4],
    /// Rectangle in tiles, rotation-0 world (for `validate_diagram` summaries).
    pub plan_rect: PlanRect,
    /// Colors.
    pub colors: ZoneColors,
    /// Border.
    pub border: BorderStyle,
    /// Nesting depth.
    pub depth: u32,
    /// Label.
    pub label: Option<PlacedLabel>,
}

/// A node in screen space.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneNode {
    /// Id.
    pub id: NodeId,
    /// Center in tiles, rotation-0 world (for summaries and desktop drag).
    pub plan_center: PlanPoint,
    /// Center on the floor, screen.
    pub floor_center: ScreenPoint,
    /// Footprint, height and screen bbox.
    pub shape: NodeShape,
    /// Pre-drawn body markup (icon, shadow, replicas, badge), wrapped in `<g data-id data-role="node">`.
    pub markup: String,
    /// Label.
    pub label: Option<PlacedLabel>,
    /// Leader line (from icon top to label bottom).
    pub leader: Option<(ScreenPoint, ScreenPoint)>,
    /// Flow rank (build animation order).
    pub rank: u32,
}

/// Path of an edge in screen space.
#[derive(Debug, Clone, PartialEq)]
pub enum EdgePath {
    /// `curved` (default) and `straight` (degenerate cubic).
    Curve(CubicBezier),
    /// `orthogonal`: rounded polyline following the grid.
    Polyline(Vec<ScreenPoint>),
}

impl EdgePath {
    /// Point at `fraction` of the arc length (uniform speed for packets, label sliding).
    pub fn point_at_fraction(&self, fraction: f64) -> ScreenPoint {
        let fraction = fraction.clamp(0.0, 1.0);
        match self {
            Self::Curve(curve) => curve.point_at(curve.arc_length_table(ARC_SAMPLES).t_at_fraction(fraction)),
            Self::Polyline(points) => polyline_point_at_fraction(points, fraction),
        }
    }

    /// Direction in which the path arrives at its end.
    pub fn end_direction(&self) -> ScreenVector {
        match self {
            Self::Curve(curve) => curve.end_direction(),
            Self::Polyline(points) => {
                let mut legs = points.windows(2).rev().map(|pair| pair[1] - pair[0]);
                legs.find(|leg| leg.length() > f64::EPSILON).unwrap_or_default()
            }
        }
    }

    /// Direction in which the path leaves its start (used to check `arrow: start` heads, reversed).
    pub fn start_direction(&self) -> ScreenVector {
        match self {
            Self::Curve(curve) => {
                let direction = curve.points[1] - curve.points[0];
                if direction.length() > f64::EPSILON { direction } else { curve.tangent_at(0.0) }
            }
            Self::Polyline(points) => {
                let mut legs = points.windows(2).map(|pair| pair[1] - pair[0]);
                legs.find(|leg| leg.length() > f64::EPSILON).unwrap_or_default()
            }
        }
    }

    /// Total length in px.
    pub fn length(&self) -> f64 {
        match self {
            Self::Curve(curve) => curve.arc_length_table(ARC_SAMPLES).total_length(),
            Self::Polyline(points) => points.windows(2).map(|pair| pair[0].distance(pair[1])).sum(),
        }
    }

    /// Points along the path, `segments + 1` of them for curves (uniform in `t`) and every vertex plus
    /// subdivisions no longer than `max_step` px for polylines. Used for collision boxes and lint.
    pub fn sample(&self, segments: usize, max_step: f64) -> Vec<ScreenPoint> {
        match self {
            Self::Curve(curve) => {
                let segments = segments.max(1);
                (0..=segments).map(|index| curve.point_at(index as f64 / segments as f64)).collect()
            }
            Self::Polyline(points) => subdivide(points, max_step),
        }
    }

    /// Screen bounding box of the stroke centerline.
    pub fn bounds(&self) -> Rect {
        Rect::from_points(self.sample(48, 8.0))
    }

    /// Arc-length table of a curve (callers that sample many fractions reuse it).
    pub fn arc_length_table(&self) -> Option<ArcLengthTable> {
        match self {
            Self::Curve(curve) => Some(curve.arc_length_table(ARC_SAMPLES)),
            Self::Polyline(_) => None,
        }
    }
}

/// Moves along a path by arc-length fraction, reusing one arc-length table for many queries.
pub(crate) struct PathWalker<'a> {
    path: &'a EdgePath,
    table: Option<ArcLengthTable>,
}

impl<'a> PathWalker<'a> {
    /// Walker over `path`.
    pub(crate) fn new(path: &'a EdgePath) -> Self {
        Self { path, table: path.arc_length_table() }
    }

    /// Same as [`EdgePath::point_at_fraction`].
    pub(crate) fn point_at(&self, fraction: f64) -> ScreenPoint {
        match (self.path, &self.table) {
            (EdgePath::Curve(curve), Some(table)) => curve.point_at(table.t_at_fraction(fraction.clamp(0.0, 1.0))),
            _ => self.path.point_at_fraction(fraction),
        }
    }
}

fn polyline_point_at_fraction(points: &[ScreenPoint], fraction: f64) -> ScreenPoint {
    let Some(first) = points.first() else {
        return ScreenPoint::default();
    };
    let total: f64 = points.windows(2).map(|pair| pair[0].distance(pair[1])).sum();
    let mut remaining = fraction * total;
    for pair in points.windows(2) {
        let leg = pair[0].distance(pair[1]);
        if remaining <= leg && leg > f64::EPSILON {
            return pair[0] + (pair[1] - pair[0]) * (remaining / leg);
        }
        remaining -= leg;
    }
    points.last().copied().unwrap_or(*first)
}

fn subdivide(points: &[ScreenPoint], max_step: f64) -> Vec<ScreenPoint> {
    let mut samples: Vec<ScreenPoint> = points.first().copied().into_iter().collect();
    for pair in points.windows(2) {
        let pieces = (pair[0].distance(pair[1]) / max_step.max(1.0)).ceil().max(1.0) as usize;
        for piece in 1..=pieces {
            samples.push(pair[0] + (pair[1] - pair[0]) * (piece as f64 / pieces as f64));
        }
    }
    samples
}

/// An arrow head: a floor triangle projected to screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrowHead {
    /// Tip.
    pub tip: ScreenPoint,
    /// Base corners.
    pub base: [ScreenPoint; 2],
    /// Screen point the head aims at (at [`EDGE_Z`]): the center of its node; on rails, whose heads point
    /// square into a face, the point of the body straight ahead of the head (T-406). A rails head arriving at
    /// a face hidden behind its node stands where the rail comes out of the node's silhouette (T-407).
    pub target_center: ScreenPoint,
}

impl SceneEdge {
    /// The heads that are painted: [`SceneEdge::midway`] when the theme draws them halfway, else [`SceneEdge::heads`].
    pub fn painted_heads(&self) -> &[ArrowHead] {
        if self.midway.is_empty() { &self.heads } else { &self.midway }
    }
}

impl ArrowHead {
    /// Direction from base midpoint to tip.
    pub fn direction(&self) -> ScreenVector {
        self.tip - self.base[0].midpoint(self.base[1])
    }
}

/// An edge in screen space.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneEdge {
    /// Id.
    pub id: EdgeId,
    /// Source.
    pub from: NodeId,
    /// Target.
    pub to: NodeId,
    /// Stroke path (ends at the base of the arrow heads, never under the tip).
    pub path: EdgePath,
    /// Arrow heads (0, 1 or 2) at the ends: ports, aim and lint always use these.
    pub heads: Vec<ArrowHead>,
    /// Heads painted halfway along the stroke instead of `heads` (theme `arrow_position: middle`, T-408);
    /// empty in the classic themes. When set, the stroke is painted on to the tips of `heads`.
    pub midway: Vec<ArrowHead>,
    /// Connection class (T-409), inferred or written.
    pub kind: EdgeKind,
    /// Stroke color.
    pub color: Color,
    /// Stroke width (px).
    pub width: f64,
    /// Dashed.
    pub dashed: bool,
    /// Traffic animation.
    pub flow: FlowMode,
    /// Pill label.
    pub label: Option<PlacedLabel>,
}

/// Title block.
#[derive(Debug, Clone, PartialEq)]
pub struct TitleBlock {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Baseline of the title.
    pub position: ScreenPoint,
}

/// The scene.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    /// Theme.
    pub theme: Theme,
    /// Rotation used (resolved when `auto`).
    pub rotation: Rotation,
    /// Density used.
    pub density: Density,
    /// Tile size (px).
    pub tile_px: f64,
    /// Auto-fit viewBox (content bbox + padding + title space).
    pub viewbox: Rect,
    /// Title.
    pub title: Option<TitleBlock>,
    /// Zones, parents before children.
    pub zones: Vec<SceneZone>,
    /// Nodes in paint order (`x + y` of the back corner, then `z`).
    pub nodes: Vec<SceneNode>,
    /// Edges in spec order.
    pub edges: Vec<SceneEdge>,
    /// Defs (gradients, filters), sorted by id.
    pub defs: Vec<SvgDef>,
    /// Residual collisions.
    pub collision: CollisionReport,
    /// Scores of the 4 angles when `rotation: auto` was requested.
    pub rotation_scores: Option<RotationScores>,
    /// Lint findings (`W_LABEL_OVERLAP`, `W_EDGE_THROUGH_NODE`, `W_EDGE_CROSSING`).
    pub diagnostics: Diagnostics,
    /// Background grid (`view.grid`, T-404).
    pub grid: SceneGrid,
    /// Legend of the connection classes (`view.legend`, T-409).
    pub legend: Option<SceneLegend>,
}

/// The background grid, resolved against the theme. Lines (or dots) sit on the plan lattice
/// `origin + k · size` tiles, where `origin` is the layout's world origin carried through the camera
/// rotation: the grid is fixed to the diagram (zones and nodes keep their place on it) in every rotation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneGrid {
    /// Side of a cell in plan tiles (on screen: `size · tile_px` along each iso axis).
    pub size: f64,
    /// Lines, dots or none.
    pub style: GridStyle,
    /// Stroke / dot color.
    pub color: Color,
    /// Opacity, 0 to 1.
    pub opacity: f64,
    /// A lattice point, in the turned plan of the scene (the world origin of the layout).
    pub origin: PlanPoint,
}

impl SceneGrid {
    /// Grid of `options` in `theme` (no color → the theme's grid color), anchored at the plan origin.
    pub fn of(options: &GridOptions, theme: &Theme) -> Self {
        Self {
            size: options.size,
            style: options.style,
            color: options.color.as_ref().map_or(theme.grid, |color| theme.resolve(color)),
            opacity: options.opacity,
            origin: PlanPoint::new(0.0, 0.0),
        }
    }

    /// The same grid anchored at `origin`.
    pub fn anchored_at(self, origin: PlanPoint) -> Self {
        Self { origin, ..self }
    }
}

/// Builds the scene of a model (handles `rotation: auto`). Fails only on `E_*` (e.g. unknown icon).
pub fn build_scene(model: &Model, options: &SceneOptions) -> Result<Scene, Diagnostics> {
    build_scene_with(model, options, IconRegistry::empty())
}

/// [`build_scene`] with flat icons (provider packs, project icons) available to the nodes.
pub fn build_scene_with(model: &Model, options: &SceneOptions, icons: &IconRegistry) -> Result<Scene, Diagnostics> {
    let base = layout::layout_with(model, options.density, icons)?;
    let styling = Styling { theme: Theme::builtin(options.theme), kinds: icons::edge_kinds(model, icons) };
    match options.rotation {
        RotationSpec::Fixed(rotation) => Ok(build_view(model, &base, &styling, rotation, options).scene),
        RotationSpec::Auto => Ok(best_rotation(model, &base, &styling, options)),
    }
}

/// The theme and the connection class of every edge (spec order): how the scene is dressed.
struct Styling {
    theme: Theme,
    kinds: Vec<EdgeKind>,
}

/// `rotation: auto`: the same layout seen from the 4 angles. The lowest collision score wins; ties go to
/// the view with fewer misread edges (through nodes, hidden), then to 0° (the reading direction of the
/// spec), then to the smallest viewBox area, then to the lowest angle.
fn best_rotation(model: &Model, base: &PlanLayout, styling: &Styling, options: &SceneOptions) -> Scene {
    let candidates = all_views(model, base, styling, options);
    let scores = RotationScores {
        scores: std::array::from_fn(|index| (Rotation::ALL[index], candidates[index].scene.collision.score)),
    };
    let [first, rest @ ..] = candidates;
    let mut best = first;
    for candidate in rest {
        if is_better(&candidate, &best) {
            best = candidate;
        }
    }
    best.scene.rotation_scores = Some(scores);
    best.scene
}

/// The 4 views of `rotation: auto`. They share nothing but the inputs, so each builds on its own thread
/// (T-406: the slowest view, not the sum of the four, sets the time); the result is the same as building
/// them one after the other. Targets without threads (wasm32) build them in turn.
#[cfg(not(target_arch = "wasm32"))]
fn all_views(model: &Model, base: &PlanLayout, styling: &Styling, options: &SceneOptions) -> [View; 4] {
    std::thread::scope(|scope| {
        let [first, rest @ ..] = Rotation::ALL;
        let others = rest.map(|rotation| scope.spawn(move || build_view(model, base, styling, rotation, options)));
        let first = build_view(model, base, styling, first, options);
        let [second, third, fourth] = others.map(|handle| match handle.join() {
            Ok(view) => view,
            // A panic in a view is a bug: carry it to the caller as if the view had been built here.
            Err(panic) => std::panic::resume_unwind(panic),
        });
        [first, second, third, fourth]
    })
}

/// [`all_views`] one after the other (no threads on this target).
#[cfg(target_arch = "wasm32")]
fn all_views(model: &Model, base: &PlanLayout, styling: &Styling, options: &SceneOptions) -> [View; 4] {
    Rotation::ALL.map(|rotation| build_view(model, base, styling, rotation, options))
}

/// A scene and how readable its edges are.
struct View {
    scene: Scene,
    readability: EdgeReadability,
}

fn is_better(candidate: &View, current: &View) -> bool {
    let key = |view: &View| (view.readability.problems(), view.scene.rotation != Rotation::R0);
    candidate
        .scene
        .collision
        .score
        .total_cmp(&current.scene.collision.score)
        .then_with(|| key(candidate).cmp(&key(current)))
        .then_with(|| candidate.scene.viewbox.area().total_cmp(&current.scene.viewbox.area()))
        .is_lt()
}

/// Builds the scene for one fixed rotation from an existing rotation-0 layout (used by `auto`, which
/// lays out once and tries 4 angles). Connection classes see the built-in icons only.
pub fn build_scene_at(
    model: &Model,
    layout: &PlanLayout,
    theme: &Theme,
    rotation: Rotation,
    options: &SceneOptions,
) -> Scene {
    let styling = Styling { theme: theme.clone(), kinds: icons::edge_kinds(model, IconRegistry::empty()) };
    build_view(model, layout, &styling, rotation, options).scene
}

/// Slides the midway heads (theme `arrow_position: middle`) off the placed labels, other strokes and other
/// heads, then forgets the label × head overlaps the slide resolved (the placement saw the heads where they
/// started).
fn settle_midway_heads(
    placement: &mut labels::Placement,
    edges: &mut [SceneEdge],
    scene_nodes: &[SceneNode],
    zones: &[SceneZone],
    projection: &Projection,
) {
    let label_boxes: Vec<Rect> = scene_nodes
        .iter()
        .filter_map(|node| node.label.as_ref())
        .chain(edges.iter().filter_map(|edge| edge.label.as_ref()))
        .chain(zones.iter().filter_map(|zone| zone.label.as_ref()))
        .map(|label| label.rect)
        .collect();
    midway::clear_of_labels(edges, &label_boxes, projection);
    let label_rect = |item: &ItemRef| {
        let id = item.id.as_str();
        let label = match item.role {
            ItemRole::NodeLabel => {
                scene_nodes.iter().find(|node| node.id.as_str() == id).and_then(|node| node.label.as_ref())
            }
            ItemRole::EdgeLabel => {
                edges.iter().find(|edge| edge.id.as_str() == id).and_then(|edge| edge.label.as_ref())
            }
            ItemRole::ZoneLabel => {
                zones.iter().find(|zone| zone.id.as_str() == id).and_then(|zone| zone.label.as_ref())
            }
            _ => None,
        };
        label.map(|label| label.rect)
    };
    midway::forget_cleared_heads(&mut placement.report, &mut placement.diagnostics, edges, label_rect);
}

fn build_view(
    model: &Model,
    layout: &PlanLayout,
    styling: &Styling,
    rotation: Rotation,
    options: &SceneOptions,
) -> View {
    let theme = &styling.theme;
    let projection = Projection::new(options.tile_px);
    let turned = layout::rotate(layout, rotation);
    let drawn = nodes::draw_nodes(model, layout, &turned, theme, &projection);
    let mut edges = edges::build_edges(model, &turned, &drawn, theme, &projection, &styling.kinds);
    let mut zones = framing::scene_zones(model, layout, &turned, theme, &projection);
    let mut scene_nodes = drawn.nodes;
    let mut placement = labels::place_labels(
        model,
        &labels::LabelInputs { theme, projection: &projection, lift_px: turned.params.label_lift_px },
        &mut scene_nodes,
        &mut edges,
        &mut zones,
        &drawn.silhouettes,
        &labels::DrawnExtras { outlines: &drawn.outlines, badges: &drawn.badges, anchors: &drawn.label_anchors },
    );
    if theme.edge_drawing.arrow_position == ArrowPosition::Middle {
        settle_midway_heads(&mut placement, &mut edges, &scene_nodes, &zones, &projection);
        midway::report_head_pairs(&mut placement.report, &mut placement.diagnostics, model, &edges);
    }
    let mut diagnostics = placement.diagnostics;
    let (edge_findings, readability) = lint::lint_edges(model, &edges, &scene_nodes, &drawn.silhouettes);
    diagnostics.extend(edge_findings);
    let content = framing::content_bounds(&zones, &scene_nodes, &edges);
    let obstacles = legend::LegendObstacles { zones: &zones, nodes: &scene_nodes, edges: &edges };
    let legend = legend::place_legend(model, theme, &obstacles, content);
    let content = legend.as_ref().map_or(content, |legend| content.union(legend.rect));
    let (viewbox, title) = framing::frame(model, content, theme, options);
    let scene = Scene {
        theme: theme.clone(),
        rotation,
        density: layout.density,
        tile_px: options.tile_px,
        viewbox,
        title,
        zones,
        nodes: scene_nodes,
        edges,
        defs: drawn.defs,
        collision: placement.report,
        rotation_scores: None,
        diagnostics,
        grid: SceneGrid::of(&model.view.grid, theme).anchored_at(rotate_about(
            PlanPoint::new(0.0, 0.0),
            layout.pivot(),
            rotation,
        )),
        legend,
    };
    View { scene, readability }
}
