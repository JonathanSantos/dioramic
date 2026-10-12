//! Flat icons on an isometric base: lying on top of a low block (`mount: top`) or standing on the front
//! face of a thin panel (`mount: stand`). The SVG is registered once as a def and placed with `<use>` and
//! an affine transform that maps its view box onto the target face (floor matrix for the top).

use dioramic_spec::{NodeSize, Rotation};

use super::{FlatIcon, Mount, ViewBox};
use crate::canvas::{IsoCanvas, Stroke, Style, fmt_matrix, fmt_ratio};
use crate::color::Color;
use crate::geometry::{Affine, PlanPoint, Rect, ScreenPoint};
use crate::icons::primitives::{Face, FaceRegion, IconFrame, LocalBox, solid_box};
use crate::icons::{Footprint, IconMetrics};
use crate::theme::Theme;

/// `top`: half side of the square block (tiles).
const TOP_HALF: f64 = 0.44;
/// `top`: block height (tiles). Low on purpose: arrows may end behind it.
const TOP_HEIGHT: f64 = 0.22;
/// `top`: free border around a transparent icon on the block's top (share of the side).
const TOP_ICON_INSET: f64 = 0.12;
/// `stand`: plinth half width (along the face) and half depth (tiles).
const STAND_HALF_WIDTH: f64 = 0.42;
const STAND_HALF_DEPTH: f64 = 0.2;
/// `stand`: plinth height (tiles).
const STAND_PLINTH_HEIGHT: f64 = 0.1;
/// `stand`: panel half width, thickness and height (tiles). The panel is square, like the icons.
const STAND_PANEL_HALF_WIDTH: f64 = 0.38;
const STAND_PANEL_DEPTH: f64 = 0.08;
const STAND_PANEL_HEIGHT: f64 = 0.76;
/// `stand`: free border around a transparent icon on the panel (share of the side).
const STAND_ICON_INSET: f64 = 0.1;
/// Share of the side kept free around an icon with its own backdrop when the node is tinted.
const TINTED_BACKDROP_INSET: f64 = 0.08;

/// Footprint and height of a flat icon (rectangular stand footprints swap on 90°/270°).
pub fn flat_metrics(flat: &FlatIcon, size: NodeSize, rotation: Rotation) -> IconMetrics {
    let scale = size.scale();
    match flat.mount {
        Mount::Top => IconMetrics {
            footprint: Footprint::Rect { half_width: TOP_HALF * scale, half_depth: TOP_HALF * scale },
            height: TOP_HEIGHT * scale,
        },
        Mount::Stand => {
            let (half_width, half_depth) = if rotation.quarter_turns() % 2 == 1 {
                (STAND_HALF_DEPTH, STAND_HALF_WIDTH)
            } else {
                (STAND_HALF_WIDTH, STAND_HALF_DEPTH)
            };
            IconMetrics {
                footprint: Footprint::Rect { half_width: half_width * scale, half_depth: half_depth * scale },
                height: (STAND_PLINTH_HEIGHT + STAND_PANEL_HEIGHT) * scale,
            }
        }
    }
}

/// Draws the body of a flat icon in `frame` (shadow, replicas and badge are drawn by the caller).
pub fn draw_flat(canvas: &mut IsoCanvas, theme: &Theme, frame: &IconFrame, flat: &FlatIcon, tint: Option<Color>) {
    match flat.mount {
        Mount::Top => draw_top(canvas, theme, frame, flat, tint),
        Mount::Stand => draw_stand(canvas, theme, frame, flat, tint),
    }
}

/// Colors of the base: `body` paints the solid (its sides), `face` the face the icon lies on, and `inset`
/// is the share of that face left free around the icon.
struct BaseColors {
    body: Color,
    face: Color,
    inset: f64,
}

/// Square provider icons (AWS) bring their own backdrop: the whole base takes it and the icon covers the
/// face edge to edge. Transparent icons (Azure, GCP, Kubernetes) lie on a light card whose sides take the
/// icon's brand color, so every base still reads as "that service" from the side.
fn base_colors(theme: &Theme, flat: &FlatIcon, tint: Option<Color>, transparent_inset: f64) -> BaseColors {
    let neutral_body = theme.nodes.plinth.mix(theme.stroke, NEUTRAL_BODY_DARKEN);
    let (body, face, inset) = match (tint, flat.svg.backdrop) {
        (Some(tint), Some(_)) => (tint, tint, TINTED_BACKDROP_INSET),
        (Some(tint), None) => (tint, theme.nodes.plinth, transparent_inset),
        (None, Some(backdrop)) => (backdrop, backdrop, 0.0),
        (None, None) => (flat.svg.accent.unwrap_or(neutral_body), theme.nodes.plinth, transparent_inset),
    };
    let saturation = theme.icon_saturation.clamp(0.0, 1.0);
    BaseColors { body: desaturate(body, saturation), face: desaturate(face, saturation), inset }
}

/// Moves a color towards its grey (Rec. 709 luma) by `1 - saturation`.
fn desaturate(color: Color, saturation: f64) -> Color {
    if saturation >= 1.0 {
        return color;
    }
    let luma = (0.2126 * f64::from(color.r) + 0.7152 * f64::from(color.g) + 0.0722 * f64::from(color.b)).round();
    let grey = luma.clamp(0.0, 255.0) as u8;
    Color::rgb(grey, grey, grey).mix(color, saturation)
}

fn outline(theme: &Theme) -> Stroke {
    Stroke::new(theme.stroke, theme.stroke_width)
}

fn draw_top(canvas: &mut IsoCanvas, theme: &Theme, frame: &IconFrame, flat: &FlatIcon, tint: Option<Color>) {
    let colors = base_colors(theme, flat, tint, TOP_ICON_INSET);
    solid_box(canvas, theme, frame, &LocalBox::centered(TOP_HALF, TOP_HALF, 0.0, TOP_HEIGHT), colors.body);
    // The top is square, so it ignores the frame's quarter-turn swap: the icon keeps reading up-right
    // (floor matrix) from every camera angle instead of turning with the world.
    let half = frame.tiles(TOP_HALF * (1.0 - 2.0 * colors.inset));
    let z = frame.elevation(TOP_HEIGHT);
    let corner = |dx: f64, dy: f64| canvas.project_quiet(PlanPoint::new(frame.center.x + dx, frame.center.y + dy), z);
    let target = Parallelogram { origin: corner(-half, half), right: corner(-half, -half), down: corner(half, half) };
    let outer = frame.tiles(TOP_HALF);
    let edges = [corner(-outer, -outer), corner(outer, -outer), corner(outer, outer), corner(-outer, outer)];
    if colors.face != colors.body {
        canvas.polygon(&edges, &Style::fill(colors.face.shade(theme.faces.top)).with_stroke(outline(theme)));
    }
    place_icon(canvas, theme, flat, &target);
    if colors.inset == 0.0 {
        // A full-bleed icon hides the block's top edges: draw them again so the solid stays crisp.
        canvas.polygon(&edges, &Style::stroke(outline(theme)));
    }
}

fn draw_stand(canvas: &mut IsoCanvas, theme: &Theme, frame: &IconFrame, flat: &FlatIcon, tint: Option<Color>) {
    let colors = base_colors(theme, flat, tint, STAND_ICON_INSET);
    let plinth = LocalBox::centered(STAND_HALF_WIDTH, STAND_HALF_DEPTH, 0.0, STAND_PLINTH_HEIGHT);
    solid_box(canvas, theme, frame, &plinth, colors.body);
    let panel = LocalBox::new(
        -STAND_PANEL_HALF_WIDTH,
        -STAND_PANEL_DEPTH / 2.0,
        STAND_PLINTH_HEIGHT,
        2.0 * STAND_PANEL_HALF_WIDTH,
        STAND_PANEL_DEPTH,
        STAND_PANEL_HEIGHT,
    );
    solid_box(canvas, theme, frame, &panel, colors.body);
    // The card face is not shaded like a side: the icon must keep the colors it was designed with.
    let front = FaceRegion::of(panel, Face::Front);
    front.fill_rect(canvas, frame, (0.0, 1.0), (0.0, 1.0), &Style::fill(colors.face).with_stroke(outline(theme)));
    let face = front.inset(colors.inset, 1.0 - colors.inset, colors.inset, 1.0 - colors.inset);
    let point = |u: f64, v: f64| {
        let (a, b, z) = face.local(frame, u, v);
        canvas.project_quiet(frame.plan(a, b), frame.elevation(z))
    };
    let target = Parallelogram { origin: point(0.0, 1.0), right: point(1.0, 1.0), down: point(0.0, 0.0) };
    place_icon(canvas, theme, flat, &target);
}

/// Mix of the plinth color towards the outline for the sides of a neutral base (icon without brand color).
const NEUTRAL_BODY_DARKEN: f64 = 0.18;

/// Screen parallelogram the icon's (square-fitted) view box maps to: `origin` is its top-left corner,
/// `right` the top-right and `down` the bottom-left.
struct Parallelogram {
    origin: ScreenPoint,
    right: ScreenPoint,
    down: ScreenPoint,
}

impl Parallelogram {
    fn bounds(&self) -> Rect {
        let opposite =
            ScreenPoint::new(self.right.x + self.down.x - self.origin.x, self.right.y + self.down.y - self.origin.y);
        let mut rect = Rect::EMPTY;
        for point in [self.origin, self.right, self.down, opposite] {
            rect.include(point);
        }
        rect
    }

    /// Affine mapping view-box coordinates onto the parallelogram, keeping the icon's aspect ratio
    /// (centered in the unit square).
    fn transform(&self, view_box: &ViewBox) -> Affine {
        let scale = 1.0 / view_box.width.max(view_box.height);
        let offset_u = (1.0 - view_box.width * scale) / 2.0 - view_box.x * scale;
        let offset_v = (1.0 - view_box.height * scale) / 2.0 - view_box.y * scale;
        let (ux, uy) = (self.right.x - self.origin.x, self.right.y - self.origin.y);
        let (vx, vy) = (self.down.x - self.origin.x, self.down.y - self.origin.y);
        Affine {
            a: ux * scale,
            b: uy * scale,
            c: vx * scale,
            d: vy * scale,
            e: self.origin.x + ux * offset_u + vx * offset_v,
            f: self.origin.y + uy * offset_u + vy * offset_v,
        }
    }
}

/// Registers the icon def (and the desaturation filter of themes that ask for it) and places it.
fn place_icon(canvas: &mut IsoCanvas, theme: &Theme, flat: &FlatIcon, target: &Parallelogram) {
    let symbol = canvas.define(flat.svg.symbol_id.clone(), flat.svg.markup.clone());
    let filter = desaturation_filter(canvas, theme);
    let filter_attribute = filter.map(|id| format!(" filter=\"url(#{id})\"")).unwrap_or_default();
    let markup = format!(
        "<use href=\"#{symbol}\" transform=\"{}\"{filter_attribute}/>",
        fmt_matrix(&target.transform(&flat.svg.view_box))
    );
    canvas.raw(&markup, target.bounds());
}

/// `feColorMatrix saturate` def for themes with `icon_saturation < 1` (`mono` prints brand icons in grey).
fn desaturation_filter(canvas: &mut IsoCanvas, theme: &Theme) -> Option<String> {
    if theme.icon_saturation >= 1.0 {
        return None;
    }
    let value = fmt_ratio(theme.icon_saturation.max(0.0));
    let id = format!("desat{}", value.replace('.', "_"));
    let markup = format!(
        "<filter id=\"{id}\" color-interpolation-filters=\"sRGB\"><feColorMatrix type=\"saturate\" values=\"{value}\"/></filter>"
    );
    Some(canvas.define(id, markup))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use dioramic_spec::ThemeName;

    use super::*;
    use crate::geometry::Projection;
    use crate::icons::flat::fixtures::{GLYPH_SVG, SQUARE_SVG, source};
    use crate::icons::flat::intern;
    use crate::icons::flat::sanitize_svg;

    fn flat(svg: &str, mount: Mount) -> &'static FlatIcon {
        let source = source("drawtest", "icon", svg, mount);
        let entry = intern(&source, sanitize_svg(svg).unwrap());
        match entry.kind {
            crate::icons::IconKind::Flat(flat) => flat.0,
            crate::icons::IconKind::Procedural(_) => unreachable!("interned as flat"),
        }
    }

    fn frame(rotation: Rotation) -> IconFrame {
        IconFrame::new(PlanPoint::new(2.0, 1.0), 0.0, 1.0, rotation, crate::geometry::DEFAULT_TILE_PX)
    }

    #[test]
    fn metrics_are_square_for_top_and_swap_for_stand() {
        let top = flat(SQUARE_SVG, Mount::Top);
        let metrics = flat_metrics(top, NodeSize::M, Rotation::R90);
        assert_eq!(metrics.footprint.half_extents(), (TOP_HALF, TOP_HALF));
        assert!(!metrics.is_tall());
        let stand = flat(GLYPH_SVG, Mount::Stand);
        let r0 = flat_metrics(stand, NodeSize::M, Rotation::R0).footprint.half_extents();
        let r90 = flat_metrics(stand, NodeSize::M, Rotation::R90).footprint.half_extents();
        assert_eq!(r0, (r90.1, r90.0));
        assert!(flat_metrics(stand, NodeSize::M, Rotation::R0).is_tall());
        let large = flat_metrics(top, NodeSize::L, Rotation::R0);
        assert!(large.height > TOP_HEIGHT);
    }

    #[test]
    fn top_maps_the_view_box_onto_the_block_top() {
        let icon = flat(SQUARE_SVG, Mount::Top);
        let theme = Theme::builtin(ThemeName::Blueprint);
        let mut canvas = IsoCanvas::new(Projection::default());
        draw_flat(&mut canvas, &theme, &frame(Rotation::R0), icon, None);
        let output = canvas.finish();
        assert!(output.body.contains(&format!("<use href=\"#{}\"", icon.svg.symbol_id)));
        assert_eq!(output.defs.len(), 1);
        // The view box corners land on the block's top corners: left, top and bottom vertices.
        let affine = Parallelogram {
            origin: ScreenPoint::new(0.0, 0.0),
            right: ScreenPoint::new(10.0, -5.0),
            down: ScreenPoint::new(10.0, 5.0),
        }
        .transform(&icon.svg.view_box);
        let map = |x: f64, y: f64| (affine.a * x + affine.c * y + affine.e, affine.b * x + affine.d * y + affine.f);
        let close = |(x, y): (f64, f64), (ex, ey): (f64, f64)| (x - ex).abs() < 1e-9 && (y - ey).abs() < 1e-9;
        assert!(close(map(0.0, 0.0), (0.0, 0.0)));
        assert!(close(map(80.0, 0.0), (10.0, -5.0)));
        assert!(close(map(0.0, 80.0), (10.0, 5.0)));
    }

    #[test]
    fn wide_view_boxes_are_centered() {
        let target = Parallelogram {
            origin: ScreenPoint::new(0.0, 0.0),
            right: ScreenPoint::new(100.0, 0.0),
            down: ScreenPoint::new(0.0, 100.0),
        };
        let affine = target.transform(&ViewBox { x: 10.0, y: 0.0, width: 200.0, height: 100.0 });
        // 200×100 fitted into 100×100: half scale, 25 px of free space above and below.
        assert!((affine.a - 0.5).abs() < 1e-9 && (affine.d - 0.5).abs() < 1e-9);
        assert!((affine.e + 5.0).abs() < 1e-9 && (affine.f - 25.0).abs() < 1e-9);
    }

    #[test]
    fn mono_desaturates_and_stand_draws_in_every_rotation() {
        let icon = flat(GLYPH_SVG, Mount::Stand);
        let theme = Theme::builtin(ThemeName::Mono);
        for rotation in Rotation::ALL {
            let mut canvas = IsoCanvas::new(Projection::default());
            draw_flat(&mut canvas, &theme, &frame(rotation), icon, None);
            let output = canvas.finish();
            assert!(output.body.contains("filter=\"url(#desat0)\""), "{rotation:?}");
            assert!(output.defs.iter().any(|def| def.id == "desat0"));
            assert!(!output.bounds.is_empty());
        }
        let colored = Theme::builtin(ThemeName::Paper);
        let mut canvas = IsoCanvas::new(Projection::default());
        draw_flat(&mut canvas, &colored, &frame(Rotation::R0), icon, Some(Color::from_u32(0x3e50c8)));
        assert!(!canvas.finish().body.contains("filter"));
    }
}
