//! Per-icon metrics, the node drawer (shadow, replicas, `×N` badge) and the contact sheet.

use dioramic_spec::{NodeSize, Rotation};

use super::ProceduralIcon;
use super::drawers::{LocalFootprint, Painter, draw_body, shape_of};
use super::flat::{draw_flat, flat_metrics};
use super::primitives::{IconFrame, badge, cloud_outline, node_shadow};
use super::{Footprint, IconDraw, IconEntry, IconKind, IconMetrics, NodeShape};
use crate::canvas::{IsoCanvas, Style, escape_xml, fmt_num, fmt_ratio};
use crate::geometry::{COS_30, PlanPoint, PlanVector, Projection, Rect, ScreenPoint};
use crate::theme::Theme;

/// Copies drawn for `replicas: N` (the badge carries the exact count).
const MAX_VISIBLE_REPLICAS: u32 = 3;
/// Plan step (tiles, at scale 1) of each copy behind the previous one: straight along plan `−x`, so the
/// deck follows a grid line (up and to the left on screen) and every copy shows the same thin slice (T-401).
const REPLICA_STEP: PlanVector = PlanVector::new(-0.17, 0.0);
/// Brightness of the copies behind the node (a linear `feComponentTransfer` slope in sRGB): they recede
/// a little, so the front copy reads as the node and the slices as "more of the same" (T-401).
const REPLICA_DIM_SLOPE: f64 = 0.82;
/// Id of the filter that dims the back copies.
const REPLICA_DIM_FILTER: &str = "replica-dim";
/// Shadow radius beyond the footprint (tiles).
const SHADOW_MARGIN: f64 = 0.12;
/// Badge center above the body top (px).
const BADGE_LIFT: f64 = 6.0;

/// Footprint and height of an icon at a size and camera rotation (footprints swap on 90°/270°).
pub fn metrics(entry: &IconEntry, size: NodeSize, rotation: Rotation) -> IconMetrics {
    let icon = match entry.kind {
        IconKind::Procedural(icon) => icon,
        IconKind::Flat(flat) => return flat_metrics(&flat, size, rotation),
    };
    let shape = shape_of(icon);
    let scale = size.scale();
    let footprint = match shape.footprint {
        LocalFootprint::Circle(radius) => Footprint::Circle { radius: radius * scale },
        LocalFootprint::Rect(half_width, half_depth) => {
            let (half_width, half_depth) =
                if rotation.quarter_turns() % 2 == 1 { (half_depth, half_width) } else { (half_width, half_depth) };
            Footprint::Rect { half_width: half_width * scale, half_depth: half_depth * scale }
        }
    };
    IconMetrics { footprint, height: shape.height * scale }
}

/// Draws the icon (shadow, body, replicas, badge) and returns its shape. The caller wraps it in an item
/// (`begin_item`/`end_item`) if it needs the bbox separately; `screen_bbox` is computed here as well.
pub fn draw(canvas: &mut IsoCanvas, entry: &IconEntry, request: &IconDraw<'_>) -> NodeShape {
    let metrics = metrics(entry, request.size, request.rotation);
    let scale = request.size.scale();
    let tile = canvas.tile();
    let copies = request.replicas.clamp(1, MAX_VISIBLE_REPLICAS);
    // Back copies first; copy 0 is the node itself, at its center.
    let frames: Vec<(u32, IconFrame)> = (0..copies)
        .rev()
        .map(|copy| {
            let center = request.center + replica_shift(copy, scale);
            (copy, IconFrame::new(center, request.z, scale, request.rotation, tile))
        })
        .collect();
    let (half_width, half_depth) = metrics.footprint.half_extents();
    canvas.begin_item();
    // All shadows before any body, so no shadow darkens a copy drawn behind.
    for (_, frame) in &frames {
        node_shadow(canvas, request.theme, frame, half_width.max(half_depth) / scale + SHADOW_MARGIN);
    }
    for (copy, frame) in &frames {
        let behind = *copy > 0;
        if behind {
            canvas.define(REPLICA_DIM_FILTER, replica_dim_filter());
            canvas.raw(&format!("<g filter=\"url(#{REPLICA_DIM_FILTER})\">"), Rect::EMPTY);
        }
        match entry.kind {
            IconKind::Procedural(icon) => {
                // Only the front copy moves: the deck behind stays calm.
                let mut painter = Painter {
                    canvas: &mut *canvas,
                    theme: request.theme,
                    frame: *frame,
                    tint: request.tint,
                    animate: request.animate && !behind,
                };
                draw_body(&mut painter, icon);
            }
            IconKind::Flat(flat) => draw_flat(canvas, request.theme, frame, &flat, request.tint),
        }
        if behind {
            canvas.raw("</g>", Rect::EMPTY);
        }
    }
    if request.replicas > 1 {
        let top = canvas.project_quiet(request.center, request.z + metrics.height);
        let half_screen = (half_width + half_depth) * COS_30 * tile * 0.8;
        let anchor = ScreenPoint::new(top.x + half_screen, top.y - BADGE_LIFT);
        badge(canvas, request.theme, anchor, &format!("×{}", request.replicas));
    }
    let screen_bbox = canvas.end_item();
    NodeShape { footprint: metrics.footprint, height: metrics.height, screen_bbox }
}

/// Screen outline of what [`draw`] paints of a body that floats above its footprint (the cloud), or `None` for
/// a body standing on it, whose silhouette is the prism's (T-425: rails reach the drawn outline).
pub(crate) fn floating_outline(
    entry: &IconEntry,
    request: &IconDraw<'_>,
    projection: &Projection,
) -> Option<Vec<ScreenPoint>> {
    match entry.kind {
        IconKind::Procedural(ProceduralIcon::Cloud) => {
            let frame =
                IconFrame::new(request.center, request.z, request.size.scale(), request.rotation, projection.tile);
            Some(cloud_outline(&frame, projection))
        }
        _ => None,
    }
}

/// Plan shift of replica copy `copy` (0 = the node itself) at `scale`: `copy` equal steps along plan `−x`.
fn replica_shift(copy: u32, scale: f64) -> PlanVector {
    REPLICA_STEP * (f64::from(copy) * scale)
}

/// Plan offset of the hindmost copy [`draw`] paints for `replicas` at `scale` (zero for a single node).
/// The scene grows label obstacles and anchors over it.
pub fn replica_offset(replicas: u32, scale: f64) -> PlanVector {
    replica_shift(replicas.clamp(1, MAX_VISIBLE_REPLICAS) - 1, scale)
}

fn replica_dim_filter() -> String {
    let slope = fmt_ratio(REPLICA_DIM_SLOPE);
    format!(
        "<filter id=\"{REPLICA_DIM_FILTER}\" color-interpolation-filters=\"sRGB\"><feComponentTransfer><feFuncR type=\"linear\" slope=\"{slope}\"/><feFuncG type=\"linear\" slope=\"{slope}\"/><feFuncB type=\"linear\" slope=\"{slope}\"/></feComponentTransfer></filter>"
    )
}

/// Layout of the contact sheet (px).
const SHEET_COLUMNS: usize = 6;
const SHEET_CELL_WIDTH: f64 = 176.0;
const SHEET_PADDING: f64 = 28.0;
const SHEET_LABEL_GAP: f64 = 22.0;
const SHEET_ROW_GAP: f64 = 20.0;
const SHEET_LABEL_SIZE: f64 = 13.0;
/// Floor tile under each icon (tiles, half side).
const SHEET_FLOOR_HALF: f64 = 0.8;

struct SheetIcon {
    body: String,
    bounds: Rect,
}

fn sheet_icon(entry: &IconEntry, theme: &Theme, defs: &mut std::collections::BTreeMap<String, String>) -> SheetIcon {
    let mut canvas = IsoCanvas::new(Projection::default());
    let floor = [
        PlanPoint::new(-SHEET_FLOOR_HALF, -SHEET_FLOOR_HALF),
        PlanPoint::new(SHEET_FLOOR_HALF, -SHEET_FLOOR_HALF),
        PlanPoint::new(SHEET_FLOOR_HALF, SHEET_FLOOR_HALF),
        PlanPoint::new(-SHEET_FLOOR_HALF, SHEET_FLOOR_HALF),
    ];
    let floor_style = Style::fill(theme.zones.gray.fill).with_fill_opacity(theme.zone_opacity);
    canvas.plan_polygon(&floor, 0.0, &floor_style);
    let request = IconDraw {
        center: PlanPoint::new(0.0, 0.0),
        z: 0.0,
        rotation: Rotation::R0,
        size: NodeSize::M,
        tint: None,
        replicas: 1,
        theme,
        animate: false,
    };
    draw(&mut canvas, entry, &request);
    let output = canvas.finish();
    for def in output.defs {
        defs.entry(def.id).or_insert(def.markup);
    }
    SheetIcon { body: output.body, bounds: output.bounds }
}

/// Contact sheet (grid of icons with their ids) as a standalone SVG, for `search_icons(preview: true)`
/// and `cargo xtask gallery`. Icons share a floor line per row, so heights compare at a glance.
pub fn contact_sheet_svg(entries: &[&IconEntry], theme: &Theme) -> String {
    let mut defs = std::collections::BTreeMap::new();
    let icons: Vec<SheetIcon> = entries.iter().map(|entry| sheet_icon(entry, theme, &mut defs)).collect();
    let above = icons.iter().map(|icon| -icon.bounds.y0).fold(0.0, f64::max);
    let below = icons.iter().map(|icon| icon.bounds.y1).fold(0.0, f64::max);
    let row_height = above + below + SHEET_LABEL_GAP + SHEET_LABEL_SIZE + SHEET_ROW_GAP;
    let columns = SHEET_COLUMNS.min(entries.len()).max(1);
    let rows = entries.len().div_ceil(columns).max(1);
    let width = 2.0 * SHEET_PADDING + columns as f64 * SHEET_CELL_WIDTH;
    let height = 2.0 * SHEET_PADDING + rows as f64 * row_height - SHEET_ROW_GAP;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" font-family=\"Inter, sans-serif\">",
        w = fmt_num(width),
        h = fmt_num(height)
    );
    if !defs.is_empty() {
        svg.push_str("<defs>");
        for markup in defs.values() {
            svg.push_str(markup);
        }
        svg.push_str("</defs>");
    }
    svg.push_str(&format!(
        "<rect width=\"{}\" height=\"{}\" fill=\"{}\"/>",
        fmt_num(width),
        fmt_num(height),
        theme.background
    ));
    for (index, (entry, icon)) in entries.iter().zip(&icons).enumerate() {
        let (row, column) = (index / columns, index % columns);
        let center_x = SHEET_PADDING + (column as f64 + 0.5) * SHEET_CELL_WIDTH;
        let floor_y = SHEET_PADDING + row as f64 * row_height + above;
        svg.push_str(&format!(
            "<g data-id=\"{}\" transform=\"translate({},{})\">{}</g>",
            escape_xml(entry.id),
            fmt_num(center_x),
            fmt_num(floor_y),
            icon.body
        ));
        svg.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-size=\"{}\" font-weight=\"500\" text-anchor=\"middle\" fill=\"{}\">{}</text>",
            fmt_num(center_x),
            fmt_num(floor_y + below + SHEET_LABEL_GAP),
            fmt_num(SHEET_LABEL_SIZE),
            theme.ink,
            escape_xml(entry.id)
        ));
    }
    svg.push_str("</svg>");
    svg
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icons::{TALL_HEIGHT, catalog, lookup};
    use dioramic_spec::ThemeName;

    fn request(theme: &Theme, rotation: Rotation) -> IconDraw<'_> {
        IconDraw {
            center: PlanPoint::new(3.0, 2.0),
            z: 0.0,
            rotation,
            size: NodeSize::M,
            tint: None,
            replicas: 1,
            theme,
            animate: false,
        }
    }

    fn footprint_outline(footprint: Footprint, center: PlanPoint) -> Vec<PlanPoint> {
        match footprint {
            Footprint::Rect { half_width, half_depth } => vec![
                PlanPoint::new(center.x - half_width, center.y - half_depth),
                PlanPoint::new(center.x + half_width, center.y - half_depth),
                PlanPoint::new(center.x + half_width, center.y + half_depth),
                PlanPoint::new(center.x - half_width, center.y + half_depth),
            ],
            Footprint::Circle { radius } => (0..32)
                .map(|step| {
                    let angle = std::f64::consts::TAU * f64::from(step) / 32.0;
                    PlanPoint::new(center.x + radius * angle.cos(), center.y + radius * angle.sin())
                })
                .collect(),
        }
    }

    #[test]
    fn every_icon_draws_in_every_rotation_and_theme() {
        for name in ThemeName::ALL {
            let theme = Theme::builtin(name);
            for entry in catalog() {
                for rotation in Rotation::ALL {
                    let mut canvas = IsoCanvas::new(Projection::default());
                    let request = request(&theme, rotation);
                    let shape = draw(&mut canvas, entry, &request);
                    let expected = metrics(entry, NodeSize::M, rotation);
                    assert_eq!(shape.footprint, expected.footprint, "{} {rotation:?}", entry.id);
                    assert_eq!(shape.height, expected.height, "{} {rotation:?}", entry.id);
                    assert!(!shape.screen_bbox.is_empty(), "{} {rotation:?}", entry.id);
                    assert_eq!(canvas.bounds(), shape.screen_bbox, "{} draws only inside its item", entry.id);
                    // The body reaches its declared height (label leaders start there).
                    let top = canvas.project_quiet(request.center, expected.height);
                    assert!(
                        shape.screen_bbox.y0 <= top.y + 1.0,
                        "{} {rotation:?} is shorter than its height",
                        entry.id
                    );
                    if theme.has_shadow() {
                        for corner in footprint_outline(shape.footprint, request.center) {
                            let screen = canvas.project_quiet(corner, 0.0);
                            assert!(
                                shape.screen_bbox.inflate(0.5).contains(screen),
                                "{} {rotation:?}: footprint outside bbox",
                                entry.id
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn footprints_swap_on_odd_rotations() {
        let Some(balancer) = lookup("loadbalancer") else { panic!("catalog") };
        let straight = metrics(balancer, NodeSize::M, Rotation::R0).footprint;
        let turned = metrics(balancer, NodeSize::M, Rotation::R90).footprint;
        assert_eq!(straight, Footprint::Rect { half_width: 0.55, half_depth: 0.4 });
        assert_eq!(turned, Footprint::Rect { half_width: 0.4, half_depth: 0.55 });
        assert_eq!(metrics(balancer, NodeSize::M, Rotation::R180).footprint, straight);
        let large = metrics(balancer, NodeSize::L, Rotation::R0);
        assert!((large.height - 0.28 * 1.3).abs() < 1e-9);
    }

    #[test]
    fn tall_and_low_icons() {
        for id in ["server", "database", "cloud", "rack", "atm", "person"] {
            let Some(entry) = lookup(id) else { panic!("{id}") };
            assert!(metrics(entry, NodeSize::M, Rotation::R0).is_tall(), "{id} should be tall");
        }
        for id in ["loadbalancer", "switch", "router", "voip-phone", "k8s-cluster"] {
            let Some(entry) = lookup(id) else { panic!("{id}") };
            assert!(metrics(entry, NodeSize::M, Rotation::R0).height < TALL_HEIGHT, "{id} should be low");
        }
    }

    #[test]
    fn replicas_draw_copies_and_a_badge() {
        let theme = Theme::blueprint();
        let Some(server) = lookup("server") else { panic!("catalog") };
        let mut single = IsoCanvas::new(Projection::default());
        let one = draw(&mut single, server, &request(&theme, Rotation::R0));
        let mut many = IsoCanvas::new(Projection::default());
        let three = draw(&mut many, server, &IconDraw { replicas: 5, ..request(&theme, Rotation::R0) });
        assert!(three.screen_bbox.width() > one.screen_bbox.width());
        assert_eq!(three.footprint, one.footprint);
        let body = many.finish().body;
        assert!(body.contains(">×5</text>"));
        assert_eq!(body.matches("<ellipse").count(), 3, "one shadow per visible copy");
        assert_eq!(body.matches("filter=\"url(#replica-dim)\"").count(), 2, "only the back copies are dimmed");
    }

    #[test]
    fn replica_copies_step_evenly_along_a_grid_line() {
        // T-401: equal steps along plan −x (a grid line), whatever the rotation or size.
        for size in [NodeSize::S, NodeSize::M, NodeSize::L] {
            let scale = size.scale();
            let step = replica_shift(1, scale);
            assert!(step.y.abs() < 1e-12 && step.x < 0.0, "{step:?}");
            assert_eq!(replica_shift(2, scale), step * 2.0);
            assert_eq!(replica_offset(7, scale), step * 2.0, "at most 3 copies are drawn");
            assert_eq!(replica_offset(1, scale), PlanVector::default());
        }
        let theme = Theme::blueprint();
        let Some(server) = lookup("server") else { panic!("catalog") };
        for rotation in Rotation::ALL {
            let mut canvas = IsoCanvas::new(Projection::default());
            draw(&mut canvas, server, &IconDraw { replicas: 3, ..request(&theme, rotation) });
            let output = canvas.finish();
            assert!(output.defs.iter().any(|def| def.id == "replica-dim"), "{rotation:?}");
        }
    }

    #[test]
    fn tint_and_size_change_the_drawing() {
        let theme = Theme::blueprint();
        let Some(block) = lookup("block") else { panic!("catalog") };
        let tint = crate::color::Color::from_u32(0x2c9a8c);
        let mut canvas = IsoCanvas::new(Projection::default());
        draw(&mut canvas, block, &IconDraw { tint: Some(tint), ..request(&theme, Rotation::R0) });
        assert!(canvas.finish().body.contains("#2c9a8c"));
        let mut small = IsoCanvas::new(Projection::default());
        let small_shape = draw(&mut small, block, &IconDraw { size: NodeSize::S, ..request(&theme, Rotation::R0) });
        let mut large = IsoCanvas::new(Projection::default());
        let large_shape = draw(&mut large, block, &IconDraw { size: NodeSize::L, ..request(&theme, Rotation::R0) });
        assert!(large_shape.screen_bbox.width() > small_shape.screen_bbox.width() * 1.5);
    }

    #[test]
    fn mono_draws_no_shadow() {
        let theme = Theme::mono();
        let Some(server) = lookup("server") else { panic!("catalog") };
        let mut canvas = IsoCanvas::new(Projection::default());
        draw(&mut canvas, server, &request(&theme, Rotation::R0));
        let output = canvas.finish();
        assert!(!output.body.contains("filter="));
        assert!(output.defs.is_empty());
    }

    #[test]
    fn drawing_is_deterministic() {
        let theme = Theme::midnight();
        let render = || {
            let mut canvas = IsoCanvas::new(Projection::default());
            for entry in catalog() {
                draw(&mut canvas, entry, &request(&theme, Rotation::R90));
            }
            canvas.finish()
        };
        assert_eq!(render(), render());
    }

    fn full_sheet(theme: &Theme) -> String {
        let entries: Vec<&IconEntry> = catalog().iter().collect();
        contact_sheet_svg(&entries, theme)
    }

    #[test]
    fn contact_sheet_lists_every_icon() {
        let svg = full_sheet(&Theme::blueprint());
        assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"));
        for entry in catalog() {
            assert!(svg.contains(&format!(">{}</text>", entry.id)), "{}", entry.id);
        }
        assert!(contact_sheet_svg(&[], &Theme::blueprint()).ends_with("</svg>"));
    }

    #[test]
    fn contact_sheet_blueprint_snapshot() {
        insta::assert_snapshot!(full_sheet(&Theme::blueprint()));
    }

    #[test]
    fn contact_sheet_midnight_snapshot() {
        insta::assert_snapshot!(full_sheet(&Theme::midnight()));
    }
}
