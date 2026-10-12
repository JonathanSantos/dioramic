//! Nodes: paint order, icon drawing, badges and screen silhouettes.

use std::collections::BTreeMap;

use dioramic_spec::{Model, NodeId};

use crate::canvas::{GroupAttributes, IsoCanvas, ItemRole, ItemTag, Paint, Style, SvgDef, TextAnchor};
use crate::geometry::{COS_30, PlanPoint, PlanVector, Projection, Rect, ScreenPoint};
use crate::icons::{self, Footprint, IconDraw, IconKind, Mount};
use crate::layout::{NodePlace, PlanLayout};
use crate::text::{self, FontFamily, FontSpec};
use crate::theme::Theme;

use super::SceneNode;

/// Sides of the polygon approximating a circular footprint in silhouettes.
const CIRCLE_SIDES: usize = 24;
/// Badge pill: font size, horizontal padding, height and corner radius (px).
const BADGE_FONT_SIZE: f64 = 10.0;
const BADGE_FONT_WEIGHT: u16 = 700;
const BADGE_PADDING_X: f64 = 6.0;
const BADGE_HEIGHT: f64 = 16.0;
const BADGE_RADIUS: f64 = 8.0;
/// Baseline of the badge text below the pill center (px).
const BADGE_BASELINE_OFFSET: f64 = 3.5;

/// Geometry of the `×N` replica pill drawn by `icons::draw` (`icons/draw.rs` and `icons/primitives.rs`
/// keep these private): lift above the body top, share of the top half-diagonal it is pushed right, and
/// the pill's font size, glyph width ratio, padding and height (px). Mirrored here so labels can avoid
/// the pill; `replica_badge_matches_the_drawn_pill` fails if the drawing moves.
const REPLICA_BADGE_LIFT: f64 = 6.0;
const REPLICA_BADGE_RIGHT_SHARE: f64 = 0.8;
const REPLICA_BADGE_FONT_SIZE: f64 = 10.0;
const REPLICA_BADGE_GLYPH_WIDTH: f64 = 0.62;
const REPLICA_BADGE_PADDING_X: f64 = 6.0;
const REPLICA_BADGE_HEIGHT: f64 = 16.0;

/// Plan geometry of a node in the rotated world (what ports and routes need).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeGeometry {
    /// Center (rotated world).
    pub center: PlanPoint,
    /// Base height (tiles).
    pub z: f64,
    /// Footprint (already swapped for the rotation).
    pub footprint: Footprint,
    /// Body height (tiles).
    pub height: f64,
    /// Tall nodes hide what is behind them: arrows must arrive on visible faces.
    pub tall: bool,
    /// Plan offset of the last replica copy drawn behind the node (zero without `replicas`).
    pub replica_offset: PlanVector,
}

/// Drawn nodes in paint order, with the data the later steps need.
pub struct DrawnNodes {
    /// Scene nodes in paint order (labels not placed yet).
    pub nodes: Vec<SceneNode>,
    /// Plan geometry, same order.
    pub geometry: Vec<NodeGeometry>,
    /// Screen silhouette (convex hull of the body prism), same order.
    pub silhouettes: Vec<Vec<ScreenPoint>>,
    /// Outline of what is painted of each body, same order: its silhouette, or for a body floating above its
    /// footprint (the cloud) the outline of the drawing, which leaves the floor under it bare (T-425).
    pub bodies: Vec<Vec<ScreenPoint>>,
    /// Silhouette grown over the back replica copies (equal to the silhouette without `replicas`), same
    /// order. Labels avoid it; edges keep using the body silhouette, where their arrows arrive (T-381).
    pub outlines: Vec<Vec<ScreenPoint>>,
    /// Screen boxes of the pills drawn on each node (`badge:` and the `×N` replica count), same order.
    /// They stick out of the silhouette, so labels must avoid them separately (T-381).
    pub badges: Vec<Vec<Rect>>,
    /// Where each node's label leader starts, same order (see [`label_anchor`]).
    pub label_anchors: Vec<ScreenPoint>,
    /// Defs registered by the icons, sorted by id.
    pub defs: Vec<SvgDef>,
}

impl DrawnNodes {
    /// Paint-order index of a node id.
    pub fn index_of(&self, id: &NodeId) -> Option<usize> {
        self.nodes.iter().position(|node| &node.id == id)
    }
}

/// Paint order key: `x + y` of the back corner, then `z`, then spec order (deterministic).
fn paint_key(place: &NodePlace) -> (f64, f64, usize) {
    let (half_width, half_depth) = place.metrics.footprint.half_extents();
    (place.center.x - half_width + place.center.y - half_depth, place.z, place.index)
}

/// Draws every node of the rotated layout. `base` is the rotation-0 layout (for `plan_center`).
pub fn draw_nodes(
    model: &Model,
    base: &PlanLayout,
    turned: &PlanLayout,
    theme: &Theme,
    projection: &Projection,
) -> DrawnNodes {
    let mut order: Vec<&NodePlace> = turned.nodes.iter().collect();
    order.sort_by(|a, b| {
        let (ka, kb) = (paint_key(a), paint_key(b));
        ka.0.total_cmp(&kb.0).then(ka.1.total_cmp(&kb.1)).then(ka.2.cmp(&kb.2))
    });
    let mut defs = BTreeMap::new();
    let mut drawn = DrawnNodes {
        nodes: Vec::new(),
        geometry: Vec::new(),
        silhouettes: Vec::new(),
        bodies: Vec::new(),
        outlines: Vec::new(),
        badges: Vec::new(),
        label_anchors: Vec::new(),
        defs: Vec::new(),
    };
    for place in order {
        let Some(node) = model.nodes.get(place.index) else { continue };
        let mut canvas = IsoCanvas::new(*projection);
        canvas.set_motion_phase(icons::motion::node_phase(node.id.as_str()));
        canvas.begin_item();
        canvas.open_group(&GroupAttributes {
            tag: Some(ItemTag::new(node.id.as_str(), ItemRole::Node)),
            ..GroupAttributes::default()
        });
        let request = IconDraw {
            center: place.center,
            z: place.z,
            rotation: turned.rotation,
            size: place.size,
            tint: node.color.as_ref().map(|color| theme.resolve(color)),
            replicas: node.replicas,
            theme,
            animate: icons_move(model),
        };
        let mut shape = icons::draw(&mut canvas, place.icon, &request);
        let floating = icons::floating_outline(place.icon, &request, projection);
        let mut badges: Vec<Rect> =
            replica_badge_rect(projection, place, shape.height, node.replicas).into_iter().collect();
        if let Some(badge) = node.badge.as_deref() {
            badges.push(draw_badge(&mut canvas, theme, place, shape.height, (node.id.as_str(), badge)));
        }
        canvas.close_group();
        shape.screen_bbox = canvas.end_item();
        let output = canvas.finish();
        for def in output.defs {
            defs.entry(def.id).or_insert(def.markup);
        }
        let geometry = NodeGeometry {
            center: place.center,
            z: place.z,
            footprint: shape.footprint,
            height: shape.height,
            tall: shape.is_tall(),
            replica_offset: icons::replica_offset(node.replicas, place.size.scale()),
        };
        let body_silhouette = silhouette(projection, &geometry);
        drawn.bodies.push(floating.unwrap_or_else(|| body_silhouette.clone()));
        drawn.silhouettes.push(body_silhouette);
        drawn.outlines.push(outline(projection, &geometry));
        drawn.badges.push(badges);
        drawn.label_anchors.push(label_anchor(projection, place, &geometry));
        drawn.geometry.push(geometry);
        drawn.nodes.push(SceneNode {
            id: node.id.clone(),
            plan_center: base.node(&node.id).map_or(place.center, |original| original.center),
            floor_center: projection.project(place.center, place.z),
            shape,
            markup: output.body,
            label: None,
            leader: None,
            rank: place.rank,
        });
    }
    drawn.defs = defs.into_iter().map(|(id, markup)| SvgDef { id, markup }).collect();
    drawn
}

/// Icons get motion hooks unless the spec turns them off (`animation.icons: false`). Hooks cost nothing in a
/// static SVG (the writer strips them), and a caller may still animate a spec without an `animation` block.
fn icons_move(model: &Model) -> bool {
    model.animation.as_ref().is_none_or(|animation| animation.icons)
}

/// Small pill at the top-right corner of the body (`×3`, `v2`). Text is always outlined: the node body
/// is pre-drawn markup, so it must look the same in both text modes. Returns the pill box.
fn draw_badge(
    canvas: &mut IsoCanvas,
    theme: &Theme,
    place: &NodePlace,
    height: f64,
    (node_id, badge): (&str, &str),
) -> Rect {
    let (half_width, half_depth) = place.metrics.footprint.half_extents();
    let corner = canvas
        .project_quiet(PlanPoint::new(place.center.x + half_width, place.center.y - half_depth), place.z + height);
    let font =
        FontSpec { family: FontFamily::Inter, weight: BADGE_FONT_WEIGHT, size: BADGE_FONT_SIZE, letter_spacing: 0.0 };
    let width = text::measure(badge, &font).width + 2.0 * BADGE_PADDING_X;
    let rect = Rect::centered(corner, width.max(BADGE_HEIGHT), BADGE_HEIGHT);
    canvas.open_group(&GroupAttributes {
        tag: Some(ItemTag::new(node_id, ItemRole::Badge)),
        ..GroupAttributes::default()
    });
    canvas.rect(rect, BADGE_RADIUS, &Style::fill(theme.nodes.accent));
    let baseline = ScreenPoint::new(corner.x, corner.y + BADGE_BASELINE_OFFSET);
    let glyphs = text::outline_path(badge, &font, baseline, TextAnchor::Middle);
    canvas.path(&glyphs, &Style { fill: Paint::Solid(theme.pill.text), ..Style::default() });
    canvas.close_group();
    rect
}

/// Box of the `×N` pill `icons::draw` puts above the right of the body top when `replicas > 1`.
fn replica_badge_rect(projection: &Projection, place: &NodePlace, height: f64, replicas: u32) -> Option<Rect> {
    if replicas <= 1 {
        return None;
    }
    let (half_width, half_depth) = place.metrics.footprint.half_extents();
    let top = projection.project(place.center, place.z + height);
    let right = (half_width + half_depth) * COS_30 * projection.tile * REPLICA_BADGE_RIGHT_SHARE;
    let center = ScreenPoint::new(top.x + right, top.y - REPLICA_BADGE_LIFT);
    let text = format!("×{replicas}");
    let width = text.chars().count() as f64 * REPLICA_BADGE_FONT_SIZE * REPLICA_BADGE_GLYPH_WIDTH
        + 2.0 * REPLICA_BADGE_PADDING_X;
    Some(Rect::centered(center, width.max(REPLICA_BADGE_HEIGHT), REPLICA_BADGE_HEIGHT))
}

/// Where the label leader of a node starts: the center of the body top, except on flat icons lying on
/// top of their block (`mount: top`), whose top is the logo itself (T-395). There it starts at the back
/// vertex of the top face (of the hindmost replica copy), the highest point of the block, so the vertical
/// leader goes up from the block's edge and never crosses the glyph. `mount: stand` keeps the center of the
/// panel top: the logo is on the panel's front face, entirely below it on screen.
fn label_anchor(projection: &Projection, place: &NodePlace, geometry: &NodeGeometry) -> ScreenPoint {
    let top = geometry.z + geometry.height;
    match place.icon.kind {
        IconKind::Flat(flat) if flat.0.mount == Mount::Top => {
            let (half_width, half_depth) = geometry.footprint.half_extents();
            let back = geometry.center + geometry.replica_offset + PlanVector::new(-half_width, -half_depth);
            projection.project(back, top)
        }
        _ => {
            let floor = projection.project(geometry.center, geometry.z);
            ScreenPoint::new(floor.x, floor.y - geometry.height * projection.tile)
        }
    }
}

/// Convex hull of the projected body prism (footprint at `z` and at `z + height`).
pub fn silhouette(projection: &Projection, geometry: &NodeGeometry) -> Vec<ScreenPoint> {
    prism_hull(projection, geometry, &[PlanVector::default()])
}

/// [`silhouette`] grown over the last replica copy behind the node (the copies in between lie inside).
pub fn outline(projection: &Projection, geometry: &NodeGeometry) -> Vec<ScreenPoint> {
    prism_hull(projection, geometry, &[PlanVector::default(), geometry.replica_offset])
}

/// Convex hull of the body prism drawn at each plan shift.
fn prism_hull(projection: &Projection, geometry: &NodeGeometry, shifts: &[PlanVector]) -> Vec<ScreenPoint> {
    let outline: Vec<PlanPoint> = match geometry.footprint {
        Footprint::Rect { half_width, half_depth } => [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .iter()
            .map(|(sx, sy)| geometry.center + PlanVector::new(sx * half_width, sy * half_depth))
            .collect(),
        Footprint::Circle { radius } => (0..CIRCLE_SIDES)
            .map(|index| {
                let angle = std::f64::consts::TAU * index as f64 / CIRCLE_SIDES as f64;
                geometry.center + PlanVector::new(angle.cos(), angle.sin()) * radius
            })
            .collect(),
    };
    let points: Vec<ScreenPoint> = shifts
        .iter()
        .flat_map(|shift| outline.iter().map(move |point| *point + *shift))
        .flat_map(|point| {
            [projection.project(point, geometry.z), projection.project(point, geometry.z + geometry.height)]
        })
        .collect();
    convex_hull(points)
}

/// Monotone-chain convex hull, counter-clockwise in screen coordinates.
fn convex_hull(mut points: Vec<ScreenPoint>) -> Vec<ScreenPoint> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup_by(|a, b| a.distance(*b) < 1e-9);
    if points.len() < 3 {
        return points;
    }
    let cross = |o: ScreenPoint, a: ScreenPoint, b: ScreenPoint| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut hull: Vec<ScreenPoint> = Vec::with_capacity(points.len() * 2);
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &ScreenPoint>> =
            if pass == 0 { Box::new(points.iter()) } else { Box::new(points.iter().rev()) };
        for &point in iter {
            while hull.len() >= start + 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], point) <= 0.0 {
                hull.pop();
            }
            hull.push(point);
        }
        hull.pop();
    }
    hull
}

/// True when `point` is strictly inside the convex polygon (counter-clockwise or clockwise), by more
/// than `inset` px from every side.
pub fn inside_convex(polygon: &[ScreenPoint], point: ScreenPoint, inset: f64) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let mut sign = 0.0;
    for (index, &a) in polygon.iter().enumerate() {
        let b = polygon[(index + 1) % polygon.len()];
        let edge = b - a;
        let length = edge.length();
        if length <= f64::EPSILON {
            continue;
        }
        let side = (edge.x * (point.y - a.y) - edge.y * (point.x - a.x)) / length;
        if side.abs() <= inset {
            return false;
        }
        if sign == 0.0 {
            sign = side.signum();
        } else if side.signum() != sign {
            return false;
        }
    }
    true
}

/// How far (px) `point` lies outside the convex polygon: the largest distance past one of its sides,
/// negative inside (minus the distance to the nearest side). Near a corner it is a little less than the
/// true distance, which is what a clearance test wants (a sharp-cornered grown polygon).
pub fn outside_distance(polygon: &[ScreenPoint], point: ScreenPoint) -> f64 {
    let doubled_area: f64 =
        polygon.iter().zip(polygon.iter().cycle().skip(1)).map(|(a, b)| a.x * b.y - b.x * a.y).sum();
    // Interior points are on the left of every side of a counter-clockwise polygon (positive cross).
    let orientation = if doubled_area < 0.0 { -1.0 } else { 1.0 };
    polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .filter_map(|(&a, &b)| {
            let side = b - a;
            let length = side.length();
            (length > f64::EPSILON)
                .then(|| -orientation * (side.x * (point.y - a.y) - side.y * (point.x - a.x)) / length)
        })
        .fold(f64::NEG_INFINITY, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `x`, `y`, `width`, `height` of the first `<rect>` with the pill radius in `markup`.
    fn pill_rect_in(markup: &str) -> Option<Rect> {
        let tag = markup.split('<').find(|tag| tag.starts_with("rect ") && tag.contains(" rx=\"8\""))?;
        let attribute = |name: &str| -> Option<f64> {
            let start = tag.find(&format!(" {name}=\""))? + name.len() + 3;
            tag[start..].split('"').next()?.parse().ok()
        };
        let (x, y) = (attribute("x")?, attribute("y")?);
        Some(Rect::new(x, y, x + attribute("width")?, y + attribute("height")?))
    }

    /// Screen corners of every flat icon placed in `markup` (`<use transform="matrix(…)">`), for an icon whose
    /// view box is the square `0 0 side side`.
    fn glyph_corners(markup: &str, side: f64) -> Vec<[ScreenPoint; 4]> {
        markup
            .split("<use ")
            .skip(1)
            .filter_map(|tag| {
                let matrix = tag.split("matrix(").nth(1)?.split(')').next()?;
                let m: Vec<f64> = matrix.split(',').filter_map(|value| value.trim().parse().ok()).collect();
                let [a, b, c, d, e, f] = m[..] else { return None };
                let at = |x: f64, y: f64| ScreenPoint::new(a * x + c * y + e, b * x + d * y + f);
                Some([at(0.0, 0.0), at(side, 0.0), at(side, side), at(0.0, side)])
            })
            .collect()
    }

    /// True when `point` lies inside the convex quadrilateral `corners`, at least `inset` px from its sides.
    fn strictly_inside(corners: &[ScreenPoint; 4], point: ScreenPoint, inset: f64) -> bool {
        let signed = |a: ScreenPoint, b: ScreenPoint| {
            let (ex, ey) = (b.x - a.x, b.y - a.y);
            ((point.x - a.x) * ey - (point.y - a.y) * ex) / ex.hypot(ey)
        };
        let sides: Vec<f64> = (0..4).map(|index| signed(corners[index], corners[(index + 1) % 4])).collect();
        sides.iter().all(|side| *side > inset) || sides.iter().all(|side| *side < -inset)
    }

    #[test]
    fn leaders_of_flat_icons_never_cross_the_glyph() {
        // T-395: the leader used to start at the center of the top face, the middle of the brand logo.
        let registry = crate::icons::flat::fixtures::registry();
        let spec = "diorama: 1\nnodes:\n  - { id: fn, icon: testaws/lambda, label: Orders }\n  \
                    - { id: pods, icon: testaws/lambda, label: Pods, replicas: 3 }\n  \
                    - { id: card, icon: testaws/panel, label: Card }\n  - { id: box, icon: server, label: Box }\n";
        let model = dioramic_spec::load(spec, dioramic_spec::SourceFormat::Yaml)
            .unwrap_or_else(|errors| panic!("{errors:?}"))
            .model;
        for rotation in dioramic_spec::Rotation::ALL {
            let options = crate::scene::SceneOptions {
                rotation: dioramic_spec::RotationSpec::Fixed(rotation),
                ..crate::scene::SceneOptions::from_model(&model)
            };
            let scene = crate::scene::build_scene_with(&model, &options, &registry)
                .unwrap_or_else(|errors| panic!("{errors:?}"));
            for node in &scene.nodes {
                let Some((anchor, end)) = node.leader else { panic!("{}: no leader", node.id) };
                assert!(end.y < anchor.y && (end.x - anchor.x).abs() < 1e-9, "{}: leader goes straight up", node.id);
                let side = if node.id.as_str() == "card" { 18.0 } else { 80.0 };
                let glyphs = glyph_corners(&node.markup, side);
                if node.id.as_str() == "box" {
                    assert!(glyphs.is_empty());
                    let top = node.floor_center.y - node.shape.height * scene.tile_px;
                    assert_eq!(
                        (anchor.x, anchor.y),
                        (node.floor_center.x, top),
                        "procedural icons keep the top center"
                    );
                    continue;
                }
                let copies = if node.id.as_str() == "pods" { 3 } else { 1 };
                assert_eq!(glyphs.len(), copies, "{}: {}", node.id, node.markup);
                for glyph in &glyphs {
                    // `top`: the leader starts above the whole glyph (its back vertex); `stand`: on the panel top,
                    // which may sit below the far top corner of the slanted front face but never over the glyph.
                    let highest = glyph.iter().map(|corner| corner.y).fold(f64::INFINITY, f64::min);
                    let lies_on_top = node.id.as_str() != "card";
                    assert!(
                        !lies_on_top || anchor.y <= highest + 0.5,
                        "{} at {rotation:?}: starts on the glyph",
                        node.id
                    );
                    for step in 0..=40 {
                        let point = ScreenPoint::new(anchor.x, anchor.y + (end.y - anchor.y) * f64::from(step) / 40.0);
                        assert!(
                            !strictly_inside(glyph, point, 0.5),
                            "{} at {rotation:?}: leader over the glyph",
                            node.id
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn replica_badge_matches_the_drawn_pill() {
        let spec = "diorama: 1\nnodes:\n  - { id: api, icon: container, replicas: 3 }\n";
        let model = dioramic_spec::load(spec, dioramic_spec::SourceFormat::Yaml)
            .unwrap_or_else(|errors| panic!("{errors:?}"))
            .model;
        let layout =
            crate::layout::layout_with(&model, dioramic_spec::Density::Spaced, crate::icons::IconRegistry::empty())
                .unwrap_or_else(|errors| panic!("{errors:?}"));
        let theme = Theme::builtin(dioramic_spec::ThemeName::Blueprint);
        let projection = Projection::new(crate::geometry::DEFAULT_TILE_PX);
        let drawn = draw_nodes(&model, &layout, &layout, &theme, &projection);
        let [replica] = drawn.badges[0][..] else { panic!("one replica pill: {:?}", drawn.badges[0]) };
        // `icons::draw` paints the pill; the mirrored geometry must match it to the markup's precision.
        let drawn_pill = pill_rect_in(&drawn.nodes[0].markup).unwrap_or_else(|| panic!("no pill drawn"));
        for (mirrored, painted) in [
            (replica.x0, drawn_pill.x0),
            (replica.y0, drawn_pill.y0),
            (replica.x1, drawn_pill.x1),
            (replica.y1, drawn_pill.y1),
        ] {
            assert!((mirrored - painted).abs() < 0.2, "{replica:?} vs drawn {drawn_pill:?}");
        }
        // The outline covers the back copies (up and to the left): the last copy is the leftmost thing drawn.
        let bbox = drawn.nodes[0].shape.screen_bbox;
        let hull = Rect::from_points(drawn.outlines[0].iter().copied());
        let single = Rect::from_points(drawn.silhouettes[0].iter().copied());
        assert!(hull.x0 < single.x0 - 15.0, "{hull:?} vs {single:?}");
        assert!((hull.x0 - bbox.x0).abs() < 1.0, "{hull:?} vs {bbox:?}");
    }

    #[test]
    fn node_badges_are_reported_with_their_pill() {
        let spec = "diorama: 1\nnodes:\n  - { id: registry, icon: storage, badge: v7 }\n";
        let model = dioramic_spec::load(spec, dioramic_spec::SourceFormat::Yaml)
            .unwrap_or_else(|errors| panic!("{errors:?}"))
            .model;
        let layout =
            crate::layout::layout_with(&model, dioramic_spec::Density::Spaced, crate::icons::IconRegistry::empty())
                .unwrap_or_else(|errors| panic!("{errors:?}"));
        let theme = Theme::builtin(dioramic_spec::ThemeName::Blueprint);
        let projection = Projection::new(crate::geometry::DEFAULT_TILE_PX);
        let drawn = draw_nodes(&model, &layout, &layout, &theme, &projection);
        let [badge] = drawn.badges[0][..] else { panic!("one badge: {:?}", drawn.badges[0]) };
        let silhouette = Rect::from_points(drawn.silhouettes[0].iter().copied());
        assert!(badge.x1 > silhouette.x1, "the badge sticks out of the body: {badge:?} vs {silhouette:?}");
        assert_eq!(drawn.outlines[0], drawn.silhouettes[0], "no replicas, no extra outline");
    }

    #[test]
    fn outside_distance_is_signed_and_orientation_free() {
        let square = [
            ScreenPoint::new(0.0, 0.0),
            ScreenPoint::new(10.0, 0.0),
            ScreenPoint::new(10.0, 10.0),
            ScreenPoint::new(0.0, 10.0),
        ];
        let reversed: Vec<ScreenPoint> = square.iter().rev().copied().collect();
        for polygon in [&square[..], &reversed[..]] {
            assert!((outside_distance(polygon, ScreenPoint::new(5.0, 13.0)) - 3.0).abs() < 1e-9);
            assert!((outside_distance(polygon, ScreenPoint::new(5.0, 8.0)) + 2.0).abs() < 1e-9);
        }
    }

    #[test]
    fn hull_of_a_box_is_a_hexagon_containing_its_center() {
        let projection = Projection::new(56.0);
        let geometry = NodeGeometry {
            center: PlanPoint::new(2.0, 1.0),
            z: 0.0,
            footprint: Footprint::Rect { half_width: 0.42, half_depth: 0.42 },
            height: 1.0,
            tall: true,
            replica_offset: PlanVector::default(),
        };
        let hull = silhouette(&projection, &geometry);
        assert_eq!(hull.len(), 6);
        let middle = projection.project(geometry.center, 0.5);
        assert!(inside_convex(&hull, middle, 0.0));
        let far = projection.project(PlanPoint::new(4.0, 1.0), 0.0);
        assert!(!inside_convex(&hull, far, 0.0));
    }

    #[test]
    fn circle_silhouette_contains_the_top_center() {
        let projection = Projection::new(56.0);
        let geometry = NodeGeometry {
            center: PlanPoint::new(0.0, 0.0),
            z: 0.0,
            footprint: Footprint::Circle { radius: 0.5 },
            height: 0.9,
            tall: true,
            replica_offset: PlanVector::default(),
        };
        let hull = silhouette(&projection, &geometry);
        assert!(inside_convex(&hull, projection.project(geometry.center, 0.9), 0.0));
    }
}
