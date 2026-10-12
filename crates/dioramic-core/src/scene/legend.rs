//! Edge legend (`view.legend`, T-409): one discreet row listing the connection classes in use, each with
//! a sample of its stroke. It takes a free bottom corner of the content, or goes under it, and joins the
//! framing (`docs/06-renderizacao.md#cores-semânticas-de-conexão`).

use dioramic_spec::{EdgeColors, EdgeKind, LegendMode, Model, ValueSource};

use crate::color::Color;
use crate::geometry::{Rect, ScreenPoint};
use crate::text::{self, FontFamily, FontSpec};
use crate::theme::{LinePattern, Theme};

use super::{SceneEdge, SceneNode, SceneZone};

/// Inner padding of the legend box (px).
const PADDING_X: f64 = 12.0;
/// Height of the legend row (px).
const ROW_HEIGHT: f64 = 28.0;
/// Length of a stroke sample (px).
const SWATCH_LENGTH: f64 = 26.0;
/// Space between a sample and its name, and between entries (px).
const SWATCH_GAP: f64 = 7.0;
const ENTRY_GAP: f64 = 16.0;
/// Text size and weight of the class names.
const FONT_SIZE: f64 = 11.5;
const FONT_WEIGHT: u16 = 600;
/// Baseline below the row's vertical center (px): centers Inter's cap height.
const BASELINE_DROP: f64 = 4.0;
/// Space kept between the legend and anything else (px).
const CLEARANCE: f64 = 10.0;
/// Step (px) when sampling zone borders for the free-corner test.
const BORDER_STEP_PX: f64 = 6.0;
/// Legend strokes are a little thinner than the edges they stand for: a sample, not a connection.
const SWATCH_WIDTH_SHARE: f64 = 0.8;

/// One class of the legend.
#[derive(Debug, Clone, PartialEq)]
pub struct LegendEntry {
    /// Class.
    pub kind: EdgeKind,
    /// Name shown (`Data`).
    pub name: String,
    /// Stroke color.
    pub color: Color,
    /// Stroke pattern.
    pub line: LinePattern,
    /// Sample stroke width (px).
    pub width: f64,
    /// Sample from → to.
    pub swatch: (ScreenPoint, ScreenPoint),
    /// Baseline origin of the name (start-anchored).
    pub text_origin: ScreenPoint,
}

/// The placed legend.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneLegend {
    /// Box (screen).
    pub rect: Rect,
    /// Entries, in [`EdgeKind::ALL`] order.
    pub entries: Vec<LegendEntry>,
}

/// Font of the class names.
pub fn legend_font() -> FontSpec {
    FontSpec { family: FontFamily::Inter, weight: FONT_WEIGHT, size: FONT_SIZE, letter_spacing: 0.0 }
}

/// What the legend avoids when it looks for a free corner.
pub struct LegendObstacles<'a> {
    /// Zones (polygons and labels).
    pub zones: &'a [SceneZone],
    /// Nodes (bodies and labels).
    pub nodes: &'a [SceneNode],
    /// Edges (strokes, heads and pills).
    pub edges: &'a [SceneEdge],
}

/// The legend for these edges, placed against `content`, or `None` when the model does not want one.
pub fn place_legend(
    model: &Model,
    theme: &Theme,
    obstacles: &LegendObstacles<'_>,
    content: Rect,
) -> Option<SceneLegend> {
    let kinds = classes_shown(model, theme, obstacles.edges)?;
    let font = legend_font();
    let widths: Vec<f64> = kinds.iter().map(|kind| text::measure(kind.title(), &font).width).collect();
    let inner: f64 = widths.iter().map(|width| SWATCH_LENGTH + SWATCH_GAP + width).sum::<f64>()
        + ENTRY_GAP * (kinds.len().saturating_sub(1)) as f64;
    let size = (inner + 2.0 * PADDING_X, ROW_HEIGHT);
    let origin =
        free_corner(size, content, obstacles).unwrap_or_else(|| ScreenPoint::new(content.x0, content.y1 + CLEARANCE));
    let rect = Rect::new(origin.x, origin.y, origin.x + size.0, origin.y + size.1);
    let middle = rect.center().y;
    let mut x = rect.x0 + PADDING_X;
    let entries = kinds
        .iter()
        .zip(widths)
        .map(|(kind, text_width)| {
            let look = class_look(model, theme, *kind);
            let swatch = (ScreenPoint::new(x, middle), ScreenPoint::new(x + SWATCH_LENGTH, middle));
            let text_origin = ScreenPoint::new(x + SWATCH_LENGTH + SWATCH_GAP, middle + BASELINE_DROP);
            x += SWATCH_LENGTH + SWATCH_GAP + text_width + ENTRY_GAP;
            LegendEntry {
                kind: *kind,
                name: kind.title().to_owned(),
                color: look.0,
                line: look.1,
                width: look.2 * SWATCH_WIDTH_SHARE,
                swatch,
                text_origin,
            }
        })
        .collect();
    Some(SceneLegend { rect, entries })
}

/// Classes to list, in legend order; `None` when the legend is off, edges are not colored by class, or
/// too few classes are in use (`auto`: 2, `on`: 1).
fn classes_shown(model: &Model, theme: &Theme, edges: &[SceneEdge]) -> Option<Vec<EdgeKind>> {
    if theme.edge_colors(model.layout.edge_colors) != EdgeColors::Semantic {
        return None;
    }
    let minimum = match model.view.legend {
        LegendMode::Off => return None,
        LegendMode::Auto => 2,
        LegendMode::On => 1,
    };
    let colored_by_class = |edge: &&SceneEdge| {
        model.edges.iter().find(|spec| spec.id == edge.id).is_some_and(|spec| {
            spec.color.is_none()
                && (spec.style_source == ValueSource::Default
                    || EdgeKind::ALL.iter().any(|kind| kind.as_str() == spec.style))
        })
    };
    let used: Vec<EdgeKind> = edges.iter().filter(colored_by_class).map(|edge| edge.kind).collect();
    let kinds: Vec<EdgeKind> = EdgeKind::ALL.into_iter().filter(|kind| used.contains(kind)).collect();
    (kinds.len() >= minimum).then_some(kinds)
}

/// Color, pattern and width of a class: the theme's look, overridden by a style named after the class.
pub fn class_look(model: &Model, theme: &Theme, kind: EdgeKind) -> (Color, LinePattern, f64) {
    let look = theme.edges.kinds.get(kind);
    let named = model.styles.get(kind.as_str());
    let color = named.and_then(|style| style.color.as_ref()).map_or(look.color, |color| theme.resolve(color));
    let line = match named.and_then(|style| style.dashed) {
        Some(true) => LinePattern::Dashed,
        Some(false) => LinePattern::Solid,
        None => look.line,
    };
    let width = named.and_then(|style| style.width).unwrap_or(theme.edge_width);
    (color, line, width)
}

/// Top-left of a box of `size` in a bottom corner of `content` (left first) where nothing is drawn.
fn free_corner(size: (f64, f64), content: Rect, obstacles: &LegendObstacles<'_>) -> Option<ScreenPoint> {
    let (width, height) = size;
    if width > content.width() {
        return None;
    }
    [ScreenPoint::new(content.x0, content.y1 - height), ScreenPoint::new(content.x1 - width, content.y1 - height)]
        .into_iter()
        .find(|origin| {
            let rect = Rect::new(origin.x, origin.y, origin.x + width, origin.y + height).inflate(CLEARANCE);
            is_free(rect, obstacles)
        })
}

fn is_free(rect: Rect, obstacles: &LegendObstacles<'_>) -> bool {
    let touches = |other: Rect| other.overlap_area(&rect, 0.0) > 0.0;
    let nodes_clear = obstacles
        .nodes
        .iter()
        .all(|node| !touches(node.shape.screen_bbox) && node.label.as_ref().is_none_or(|label| !touches(label.rect)));
    let edges_clear = obstacles.edges.iter().all(|edge| {
        let stroke_clear =
            edge.path.sample(48, BORDER_STEP_PX).iter().all(|point| !rect.inflate(edge.width).contains(*point));
        let heads_clear =
            edge.painted_heads().iter().all(|head| !touches(Rect::from_points([head.tip, head.base[0], head.base[1]])));
        stroke_clear && heads_clear && edge.label.as_ref().is_none_or(|label| !touches(label.rect))
    });
    let zones_clear = obstacles.zones.iter().all(|zone| {
        border_points(&zone.polygon).all(|point| !rect.contains(point))
            && zone.label.as_ref().is_none_or(|label| !touches(label.rect))
    });
    nodes_clear && edges_clear && zones_clear
}

/// Points along a closed polygon's border, every [`BORDER_STEP_PX`].
fn border_points(polygon: &[ScreenPoint; 4]) -> impl Iterator<Item = ScreenPoint> + '_ {
    (0..polygon.len()).flat_map(move |index| {
        let (start, end) = (polygon[index], polygon[(index + 1) % polygon.len()]);
        let steps = (start.distance(end) / BORDER_STEP_PX).ceil().max(1.0) as usize;
        (0..=steps).map(move |step| start + (end - start) * (step as f64 / steps as f64))
    })
}
