//! SVG emission (`docs/06-renderizacao.md#ordem-de-pintura`, `docs/07-animacao.md`). Contract: C-005.
//! Implementation: T-104.
//!
//! Paint order: background → grid → zones (parents first) → zone labels → edges → packets → nodes →
//! leader lines → node labels → edge labels → title/caption. Only the resvg-supported SVG subset is used
//! (no `foreignObject`, no CSS beyond the animated mode). Arrow heads are polygons, never markers.

mod icon_motion;
mod inline_text;
mod motion;
mod writing;

use std::collections::BTreeMap;
use std::fmt::Write as _;

use dioramic_spec::GridStyle;

use crate::animation::AnimationPlan;
use crate::canvas::{IsoCanvas, ItemRole, ItemTag, Paint, PathData, Stroke, Style, fmt_num};
use crate::geometry::{PlanPoint, Projection, Rect, ScreenPoint};
use crate::scene::{Scene, SceneEdge, SceneZone};
use crate::theme::{DashPattern, LeaderStyle};

use motion::{Motion, MotionKind};
use writing::{LabelWriter, path_data};

/// Renderer version written in `<metadata>` (traceability of visual diffs).
pub const RENDERER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What kind of SVG to emit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SvgMode {
    /// No animation.
    Static,
    /// Animation evaluated at normalized time `t ∈ [0, 1)` (raster frames; resvg ignores CSS/SMIL).
    Frame {
        /// Normalized time.
        t: f64,
    },
    /// Declarative animation: CSS `@keyframes` for dashes, SMIL `<animateMotion>` for packets.
    Animated,
}

/// How text is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextRendering {
    /// `<text>` with `font-family="Inter"` (raster path: resvg uses the embedded fonts).
    #[default]
    Text,
    /// Glyph outlines as `<path>` (exported SVGs look right without Inter installed).
    Outlines,
}

/// Emission options.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvgOptions {
    /// Mode.
    pub mode: SvgMode,
    /// Text rendering.
    pub text: TextRendering,
    /// Radial vignette (never for GIF: banding with 256 colors).
    pub vignette: bool,
    /// Paint the background rect (false = transparent).
    pub background: bool,
    /// Include `<metadata>` with [`RENDERER_VERSION`].
    pub metadata: bool,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self { mode: SvgMode::Static, text: TextRendering::Text, vignette: false, background: true, metadata: true }
    }
}

/// Grid line width and zone outline width / dash (px).
const GRID_STROKE_WIDTH: f64 = 0.7;
/// Radius of the dots of `view.grid.style: dots` (px).
const GRID_DOT_RADIUS: f64 = 1.1;
const ZONE_STROKE_WIDTH: f64 = 1.4;
const ZONE_DASH: (f64, f64) = (6.0, 5.0);
/// Static dash of `dashed: true` edges (on, off) in px.
const EDGE_DASH: (f64, f64) = (8.0, 6.0);
/// Distance between the dots of a dotted edge, in stroke widths (`edge.dash: dots`).
const DOT_SPACING: f64 = 2.2;
/// Distance between the dots of a dotted leader line (px).
const LEADER_DOT_SPACING: f64 = 3.6;
/// Outline of arrow heads (px): same color as the stroke, rounds the corners slightly.
const HEAD_OUTLINE_WIDTH: f64 = 1.0;
/// Vignette gradient stops (start of fade, edge opacity).
const VIGNETTE_START: f64 = 0.6;
const VIGNETTE_OPACITY: f64 = 0.5;

/// Emits a complete standalone SVG document. `animation` is required for `Frame`/`Animated` to have
/// any effect; with `None` they behave like `Static`. Output is deterministic (fixed precision, sorted defs).
pub fn emit_svg(scene: &Scene, animation: Option<&AnimationPlan>, options: &SvgOptions) -> String {
    let motion = Motion::new(scene, animation, options.mode);
    let mut canvas = IsoCanvas::new(Projection::new(scene.tile_px));
    let labels = LabelWriter { theme: &scene.theme, text: options.text };
    if options.background {
        canvas.rect(scene.viewbox, 0.0, &Style::fill(scene.theme.background));
    }
    draw_grid(&mut canvas, scene);
    draw_zones(&mut canvas, scene, &motion);
    draw_zone_labels(&mut canvas, scene, &motion, &labels);
    draw_edges(&mut canvas, scene, &motion);
    motion.draw_packets(&mut canvas, scene);
    draw_nodes(&mut canvas, scene, &motion, options.text);
    draw_leaders(&mut canvas, scene, &motion);
    draw_node_labels(&mut canvas, scene, &motion, &labels);
    draw_edge_labels(&mut canvas, scene, &motion, &labels);
    if let Some(legend) = &scene.legend {
        labels.legend(&mut canvas, legend);
    }
    if options.vignette {
        canvas.rect(scene.viewbox, 0.0, &Style { fill: Paint::Reference("vignette".to_owned()), ..Style::default() });
    }
    labels.title(&mut canvas, scene);
    if options.vignette {
        canvas.define("vignette", vignette_def(scene));
    }
    motion.define(&mut canvas, scene);
    let output = canvas.finish();
    let mut defs: BTreeMap<String, String> =
        scene.defs.iter().map(|def| (def.id.clone(), def.markup.clone())).collect();
    for def in output.defs {
        defs.entry(def.id).or_insert(def.markup);
    }
    document(scene, options, &motion, &defs, &output.body)
}

fn document(
    scene: &Scene,
    options: &SvgOptions,
    motion: &Motion<'_>,
    defs: &BTreeMap<String, String>,
    body: &str,
) -> String {
    let viewbox = scene.viewbox;
    let (width, height) = (fmt_num(viewbox.width()), fmt_num(viewbox.height()));
    let mut svg = String::with_capacity(body.len() + 4096);
    let _ = write!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"{} {} {width} {height}\" font-family=\"Inter, sans-serif\">",
        fmt_num(viewbox.x0),
        fmt_num(viewbox.y0),
    );
    if options.metadata {
        let _ = write!(svg, "<metadata>dioramic {RENDERER_VERSION}</metadata>");
    }
    svg.push_str(&motion.stylesheet());
    if !defs.is_empty() {
        svg.push_str("<defs>");
        for markup in defs.values() {
            svg.push_str(markup);
        }
        svg.push_str("</defs>");
    }
    svg.push_str(body);
    svg.push_str("</svg>\n");
    svg
}

fn vignette_def(scene: &Scene) -> String {
    let color = scene.theme.vignette;
    format!(
        "<radialGradient id=\"vignette\" cx=\"0.5\" cy=\"0.45\" r=\"0.75\"><stop offset=\"{VIGNETTE_START}\" stop-color=\"{color}\" stop-opacity=\"0\"/><stop offset=\"1\" stop-color=\"{color}\" stop-opacity=\"{VIGNETTE_OPACITY}\"/></radialGradient>"
    )
}

/// Iso grid covering the viewBox (`view.grid`, T-404): lines or dots on the plan lattice
/// `origin + k · size` tiles ([`crate::scene::SceneGrid`]), fixed to the diagram in every rotation.
fn draw_grid(canvas: &mut IsoCanvas, scene: &Scene) {
    let grid = scene.grid;
    if grid.style == GridStyle::None || grid.opacity <= 0.0 || grid.size <= 0.0 {
        return;
    }
    let projection = canvas.projection();
    let viewbox = scene.viewbox;
    let corners = [
        ScreenPoint::new(viewbox.x0, viewbox.y0),
        ScreenPoint::new(viewbox.x1, viewbox.y0),
        ScreenPoint::new(viewbox.x1, viewbox.y1),
        ScreenPoint::new(viewbox.x0, viewbox.y1),
    ]
    .map(|corner| projection.unproject(corner, 0.0));
    // Lattice indices (integers, so no float drift accumulates along the grid).
    let index_range = |coordinate: fn(&PlanPoint) -> f64| {
        let low = corners.iter().map(coordinate).fold(f64::INFINITY, f64::min);
        let high = corners.iter().map(coordinate).fold(f64::NEG_INFINITY, f64::max);
        let origin = coordinate(&grid.origin);
        (((low - origin) / grid.size).floor() as i64, ((high - origin) / grid.size).ceil() as i64)
    };
    let (first_x, last_x) = index_range(|point| point.x);
    let (first_y, last_y) = index_range(|point| point.y);
    let at_x = |index: i64| grid.origin.x + index as f64 * grid.size;
    let at_y = |index: i64| grid.origin.y + index as f64 * grid.size;
    let mut body = String::new();
    match grid.style {
        GridStyle::Lines => {
            let mut line = |from: PlanPoint, to: PlanPoint| {
                let (a, b) = (projection.project(from, 0.0), projection.project(to, 0.0));
                let _ = write!(
                    body,
                    "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>",
                    fmt_num(a.x),
                    fmt_num(a.y),
                    fmt_num(b.x),
                    fmt_num(b.y)
                );
            };
            for column in first_x..=last_x {
                line(PlanPoint::new(at_x(column), at_y(first_y)), PlanPoint::new(at_x(column), at_y(last_y)));
            }
            for row in first_y..=last_y {
                line(PlanPoint::new(at_x(first_x), at_y(row)), PlanPoint::new(at_x(last_x), at_y(row)));
            }
        }
        GridStyle::Dots => {
            for column in first_x..=last_x {
                for row in first_y..=last_y {
                    let center = projection.project(PlanPoint::new(at_x(column), at_y(row)), 0.0);
                    if viewbox.inflate(GRID_DOT_RADIUS).contains(center) {
                        let _ = write!(
                            body,
                            "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"/>",
                            fmt_num(center.x),
                            fmt_num(center.y),
                            fmt_num(GRID_DOT_RADIUS)
                        );
                    }
                }
            }
        }
        GridStyle::None => {}
    }
    let paint = match grid.style {
        GridStyle::Dots => format!("fill=\"{}\"", grid.color),
        _ => format!("stroke=\"{}\" stroke-width=\"{}\"", grid.color, GRID_STROKE_WIDTH),
    };
    // Full opacity writes no attribute, so the default grid is byte-identical to v1.
    let opacity = if grid.opacity < 1.0 { format!(" opacity=\"{}\"", fmt_num(grid.opacity)) } else { String::new() };
    let markup = format!("<g data-role=\"{}\" {paint}{opacity}>{body}</g>", ItemRole::Grid.as_str());
    // The grid is decoration: it must not grow the tracked bounds.
    canvas.raw(&markup, Rect::EMPTY);
}

fn zone_style(scene: &Scene, zone: &SceneZone) -> Style {
    // `zone_fill: outline` (schematic): the border alone draws the zone.
    let style = if scene.theme.fills_zones() {
        Style::fill(zone.colors.fill).with_fill_opacity(scene.theme.zone_opacity)
    } else {
        Style::default()
    };
    match zone.border {
        dioramic_spec::BorderStyle::Dashed => {
            style.with_stroke(Stroke::new(zone.colors.stroke, ZONE_STROKE_WIDTH).dashed(ZONE_DASH.0, ZONE_DASH.1))
        }
        dioramic_spec::BorderStyle::Solid => style.with_stroke(Stroke::new(zone.colors.stroke, ZONE_STROKE_WIDTH)),
        dioramic_spec::BorderStyle::None => style,
    }
}

fn draw_zones(canvas: &mut IsoCanvas, scene: &Scene, motion: &Motion<'_>) {
    for zone in &scene.zones {
        motion.open(canvas, ItemTag::new(zone.id.as_str(), ItemRole::Zone), MotionKind::Zone);
        canvas.polygon(&zone.polygon, &zone_style(scene, zone));
        canvas.close_group();
    }
}

fn draw_zone_labels(canvas: &mut IsoCanvas, scene: &Scene, motion: &Motion<'_>, labels: &LabelWriter<'_>) {
    for zone in &scene.zones {
        let Some(label) = &zone.label else { continue };
        motion.open(canvas, ItemTag::new(zone.id.as_str(), ItemRole::ZoneLabel), MotionKind::Zone);
        labels.zone_label(canvas, label);
        canvas.close_group();
    }
}

fn edge_stroke(scene: &Scene, edge: &SceneEdge) -> Stroke {
    let stroke = Stroke::new(edge.color, edge.width).round_cap();
    match (edge.dashed, scene.theme.edge_drawing.dash) {
        (false, _) => stroke,
        (true, DashPattern::Dashes) => stroke.dashed(EDGE_DASH.0, EDGE_DASH.1),
        // Zero-length dashes with round caps: dots as wide as the stroke.
        (true, DashPattern::Dots) => stroke.dashed(0.0, edge.width * DOT_SPACING),
    }
}

fn draw_edges(canvas: &mut IsoCanvas, scene: &Scene, motion: &Motion<'_>) {
    for (index, edge) in scene.edges.iter().enumerate() {
        motion.open(canvas, ItemTag::new(edge.id.as_str(), ItemRole::Edge), MotionKind::Edge);
        let stroke = motion.edge_stroke(index, edge, edge_stroke(scene, edge));
        let mut style = Style::stroke(stroke);
        if let Some(class) = motion.edge_class(index, edge) {
            style = style.with_class(class);
        }
        canvas.path(&path_data(&edge.path), &style);
        if !edge.midway.is_empty() {
            // The end heads are not painted: the stroke runs on to their tips, up to the port.
            for head in &edge.heads {
                let base = head.base[0].midpoint(head.base[1]);
                canvas.path(&PathData::new().move_to(base).line_to(head.tip), &style);
            }
        }
        let head_color = scene.theme.edge_drawing.arrow_color.unwrap_or(edge.color);
        for head in edge.painted_heads() {
            let head_style = Style::fill(head_color).with_stroke(Stroke::new(head_color, HEAD_OUTLINE_WIDTH));
            canvas.polygon(&[head.tip, head.base[0], head.base[1]], &head_style);
        }
        canvas.close_group();
    }
}

fn draw_nodes(canvas: &mut IsoCanvas, scene: &Scene, motion: &Motion<'_>, text: TextRendering) {
    let max_rank = max_rank(scene);
    for node in &scene.nodes {
        let wrapped = motion.wrap_node(node, max_rank);
        if let Some(open) = &wrapped {
            canvas.raw(open, Rect::EMPTY);
        }
        // Node bodies are pre-drawn with motion hooks (T-402) and the replica pill as `<text>`: resolve the
        // hooks for this mode, then outline the text like every label.
        let moving = icon_motion::resolve_hooks(&node.markup, motion.icon_hooks());
        let markup = match text {
            TextRendering::Text => moving,
            TextRendering::Outlines => std::borrow::Cow::Owned(inline_text::outline_inline_text(&moving).into_owned()),
        };
        canvas.raw(&markup, node.shape.screen_bbox);
        if wrapped.is_some() {
            canvas.raw("</g>", Rect::EMPTY);
        }
    }
}

fn max_rank(scene: &Scene) -> u32 {
    scene.nodes.iter().map(|node| node.rank).max().unwrap_or(0)
}

fn leader_stroke(scene: &Scene) -> Stroke {
    let stroke = Stroke::new(scene.theme.label.leader, writing::LEADER_WIDTH);
    match scene.theme.label_leader {
        LeaderStyle::Solid => stroke,
        LeaderStyle::Dotted => stroke.round_cap().dashed(0.0, LEADER_DOT_SPACING),
    }
}

fn draw_leaders(canvas: &mut IsoCanvas, scene: &Scene, motion: &Motion<'_>) {
    let max_rank = max_rank(scene);
    for node in &scene.nodes {
        let Some((from, to)) = node.leader else { continue };
        motion.open(canvas, ItemTag::new(node.id.as_str(), ItemRole::Leader), MotionKind::Node(node.rank, max_rank));
        canvas.line(from, to, &leader_stroke(scene));
        canvas.close_group();
    }
}

fn draw_node_labels(canvas: &mut IsoCanvas, scene: &Scene, motion: &Motion<'_>, labels: &LabelWriter<'_>) {
    let max_rank = max_rank(scene);
    for node in &scene.nodes {
        let Some(label) = &node.label else { continue };
        motion.open(canvas, ItemTag::new(node.id.as_str(), ItemRole::NodeLabel), MotionKind::Node(node.rank, max_rank));
        labels.node_label(canvas, label);
        canvas.close_group();
    }
}

fn draw_edge_labels(canvas: &mut IsoCanvas, scene: &Scene, motion: &Motion<'_>, labels: &LabelWriter<'_>) {
    for edge in &scene.edges {
        let Some(label) = &edge.label else { continue };
        motion.open(canvas, ItemTag::new(edge.id.as_str(), ItemRole::EdgeLabel), MotionKind::Edge);
        labels.pill(canvas, label);
        canvas.close_group();
    }
}

#[cfg(test)]
mod tests {
    use dioramic_spec::SourceFormat;

    use super::*;
    use crate::{CompileOptions, compile_source};

    const REPLICATED_AND_BADGED: &str = "diorama: 1\nnodes:\n  - id: api\n    icon: server\n    replicas: 3\n    badge: v7\n  - id: db\n    icon: database\nedges:\n  - api -> db\n";

    fn node_svg(text: TextRendering) -> String {
        let diagram = compile_source(REPLICATED_AND_BADGED, SourceFormat::Yaml, &CompileOptions::default())
            .unwrap_or_else(|errors| panic!("{errors:?}"));
        let svg = emit_svg(&diagram.scene, None, &SvgOptions { text, ..SvgOptions::default() });
        let start = svg.find("data-id=\"api\"").unwrap_or_default();
        svg[start..].to_owned()
    }

    #[test]
    fn outlines_mode_writes_replica_and_badge_pills_as_paths() {
        let svg = node_svg(TextRendering::Outlines);
        assert!(!svg.contains("<text"), "{svg}");
        let badge = &svg[svg.find("data-role=\"badge\"").unwrap_or_default()..];
        assert!(badge.contains("<path d=\"M"), "badge glyphs are outlined");
        let body = &svg[..svg.find("data-role=\"badge\"").unwrap_or(svg.len())];
        assert!(body.contains("<path d=\"M"), "replica pill glyphs are outlined");
    }

    const MOVING_ICONS: &str = "diorama: 1\nnodes:\n  - { id: api, icon: server }\n  - { id: db, icon: database }\n  - { id: k8s, icon: k8s-cluster }\n  - { id: lb, icon: loadbalancer, replicas: 2 }\nedges:\n  - lb -> api\n  - api -> db\n  - api -> k8s\nanimation: { preset: flow }\n";

    fn moving_diagram(icons: bool) -> crate::Diagram {
        let source = MOVING_ICONS.replace("{ preset: flow }", &format!("{{ preset: flow, icons: {icons} }}"));
        compile_source(&source, SourceFormat::Yaml, &CompileOptions::default())
            .unwrap_or_else(|errors| panic!("{errors:?}"))
    }

    #[test]
    fn icon_motion_leaves_the_static_svg_untouched() {
        let (moving, still) = (moving_diagram(true), moving_diagram(false));
        assert!(moving.scene.nodes.iter().any(|node| node.markup.contains("data-icon-motion")));
        assert!(still.scene.nodes.iter().all(|node| !node.markup.contains("data-icon-motion")));
        let static_svg =
            |diagram: &crate::Diagram| emit_svg(&diagram.scene, diagram.animation.as_ref(), &SvgOptions::default());
        assert_eq!(static_svg(&moving), static_svg(&still));
    }

    #[test]
    fn icon_motion_loops_seamlessly_and_animates_the_svg() {
        let diagram = moving_diagram(true);
        let plan = diagram.animation.as_ref();
        let frame = |t: f64| {
            emit_svg(&diagram.scene, plan, &SvgOptions { mode: SvgMode::Frame { t }, ..SvgOptions::default() })
        };
        assert_eq!(frame(0.0), frame(1.0));
        assert_ne!(frame(0.0), frame(0.3), "icons move between frames");
        let animated = emit_svg(&diagram.scene, plan, &SvgOptions { mode: SvgMode::Animated, ..SvgOptions::default() });
        assert!(animated.contains("type=\"rotate\""), "the helm turns");
        assert!(!animated.contains("data-icon-motion"));
        let still = moving_diagram(false);
        let still_frame = emit_svg(
            &still.scene,
            still.animation.as_ref(),
            &SvgOptions { mode: SvgMode::Frame { t: 0.3 }, ..SvgOptions::default() },
        );
        assert!(!still_frame.contains("<g opacity=\"0.3\">"), "icons: false keeps the icons still");
    }

    #[test]
    fn text_mode_keeps_the_replica_pill_as_text() {
        let svg = node_svg(TextRendering::Text);
        assert!(svg.contains(">×3</text>"), "{svg}");
    }
}
