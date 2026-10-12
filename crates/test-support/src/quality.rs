//! Scene quality numbers that the collision score does not capture (`docs/work/reviews/UI-motor.md`):
//! edges crossing nodes, edges crossing each other (T-400), long label leaders and text that shrinks to
//! nothing on wide scenes.
//!
//! The gallery writes them to `report.json`; tests of other crates can assert on them directly.

use dioramic_core::geometry::{PlanPoint, Projection};
use dioramic_core::scene::{EDGE_Z, EdgePath, RAIL_LANE, Scene, SceneEdge, crossing_pairs, edge_crossings};
use dioramic_spec::{DiagnosticCode, Diagnostics};

const PERCENT: f64 = 100.0;

/// Quality numbers of one compiled view.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SceneQuality {
    /// Edges flagged with `W_EDGE_THROUGH_NODE` (one diagnostic per edge, however many nodes it crosses).
    pub edge_through_node: usize,
    /// Longest leader line from an icon to its label (screen px; 0 when no node has a leader).
    pub max_leader_px: f64,
    /// Node label font size over the viewBox width, in percent (the demo of the README is ~1.3%).
    pub text_to_width_percent: f64,
    /// Crossing points between any two edges (T-400).
    pub crossings: usize,
    /// The part of [`SceneQuality::crossings`] between edges that share a node (should be 0).
    pub crossings_shared_node: usize,
    /// Most 90° bends of one edge (T-406: elbow rails aim at ≤ 3; a curve counts its turning in quarters).
    pub bends_max: usize,
    /// Mean bends per edge (0 without edges).
    pub bends_mean: f64,
    /// Longest route relative to the Manhattan distance between its two ends on the floor plan (T-407: rails aim
    /// at ≤ 1.4; 1 for a straight, L or Z route, more for a U or a detour; 0 without edges).
    pub detour_max: f64,
    /// Length (tiles) of stroke running parallel to another edge closer than one lane ([`RAIL_LANE`]):
    /// strokes glued together or on top of each other (T-407, goal 0).
    pub overlap_len: f64,
    /// Longest route relative to the Manhattan distance between the **centers** of its two nodes (T-421): unlike
    /// [`SceneQuality::detour_max`] it sees a rail that leaves and arrives by faces far apart (a U to a neighbour).
    pub detour_center_max: f64,
    /// Longest stretch (tiles) of one route past its two nodes along an axis on which they stand apart
    /// ([`edge_stray`], T-421): a U that overshoots its neighbour, or a walk round the target to a far face.
    pub stray_max: f64,
    /// Pairs of an edge and a node label box the edge's stroke runs under ([`edges_under_node_labels`], T-422).
    pub under_node_label: usize,
    /// Pairs of edges that cross each other twice or more: two strokes plaited together (T-422, goal 0).
    pub double_crossings: usize,
    /// Where the sources (nodes no edge enters) stand in the reading of the screen, 0 = read first (top left),
    /// 1 = last (bottom right); `None` without sources ([`source_reading_rank`], T-415).
    pub source_reading_rank: Option<f64>,
    /// Most bends of one elbow route above the fewest its two end faces allow ([`edge_excess_bends`], T-421): a
    /// step just before the target, or a walk around a node, shows up here.
    pub excess_bends_max: usize,
}

impl SceneQuality {
    /// Measures `scene`; `diagnostics` are the diagram's (spec warnings + scene lint).
    pub fn of(scene: &Scene, diagnostics: &Diagnostics) -> Self {
        let crossings = edge_crossings(&scene.edges);
        let bends: Vec<usize> = scene.edges.iter().map(|edge| edge_bends(scene, edge)).collect();
        Self {
            bends_max: bends.iter().copied().max().unwrap_or(0),
            bends_mean: if bends.is_empty() { 0.0 } else { bends.iter().sum::<usize>() as f64 / bends.len() as f64 },
            detour_max: scene.edges.iter().map(|edge| edge_detour(scene, edge)).fold(0.0, f64::max),
            overlap_len: overlap_length(scene),
            detour_center_max: scene.edges.iter().map(|edge| edge_center_detour(scene, edge)).fold(0.0, f64::max),
            under_node_label: edges_under_node_labels(scene).len(),
            double_crossings: crossing_pairs(&scene.edges).iter().filter(|pair| pair.count >= 2).count(),
            source_reading_rank: source_reading_rank(scene),
            stray_max: scene.edges.iter().map(|edge| edge_stray(scene, edge)).fold(0.0, f64::max),
            excess_bends_max: scene.edges.iter().map(|edge| edge_excess_bends(scene, edge)).max().unwrap_or(0),
            crossings: crossings.total,
            crossings_shared_node: crossings.shared_node,
            edge_through_node: edge_through_node_count(diagnostics),
            max_leader_px: max_leader_px(scene),
            text_to_width_percent: text_to_width_percent(scene),
        }
    }
}

/// Samples and longest step (px) used to follow an edge when counting its bends.
const BEND_SAMPLES: usize = 64;
const BEND_STEP_PX: f64 = 2.0;
/// Pieces shorter than this (tiles) carry no direction.
const BEND_MIN_PIECE: f64 = 1e-6;

/// Bends of an edge: its total turning on the floor plan, in quarter turns (a rounded 90° rail bend is one,
/// whatever the samples on its arc).
pub fn edge_bends(scene: &Scene, edge: &SceneEdge) -> usize {
    let points = plan_samples(scene, edge);
    let directions: Vec<(f64, f64)> = points
        .windows(2)
        .map(|pair| (pair[1].x - pair[0].x, pair[1].y - pair[0].y))
        .filter(|(x, y)| x.hypot(*y) > BEND_MIN_PIECE)
        .collect();
    let turning: f64 = directions
        .windows(2)
        .map(|pair| {
            let ((ax, ay), (bx, by)) = (pair[0], pair[1]);
            (ax * by - ay * bx).atan2(ax * bx + ay * by).abs()
        })
        .sum();
    (turning / std::f64::consts::FRAC_PI_2).round() as usize
}

/// Spans (tiles) under which the Manhattan distance between the ends of an edge says nothing about a detour.
const DETOUR_MIN_SPAN: f64 = 1e-3;

/// Length of an edge on the floor plan over the Manhattan distance between its two ends (T-407). A self-loop,
/// or an edge whose ends meet, counts as 1.
pub fn edge_detour(scene: &Scene, edge: &SceneEdge) -> f64 {
    let points = plan_samples(scene, edge);
    let (Some(first), Some(last)) = (points.first(), points.last()) else { return 1.0 };
    let span = (last.x - first.x).abs() + (last.y - first.y).abs();
    if edge.from == edge.to || span < DETOUR_MIN_SPAN {
        return 1.0;
    }
    let length: f64 = points.windows(2).map(|pair| pair[0].distance(pair[1])).sum();
    length / span
}

/// Length of an edge on the floor plan over the Manhattan distance between the centers of its two nodes
/// (T-421). A rail that leaves by the front of its node and arrives by the back of a neighbour has ports far
/// apart, so [`edge_detour`] reads it as direct; measured from the centers the U shows. Self-loops and
/// nodes standing on one spot count as 1.
pub fn edge_center_detour(scene: &Scene, edge: &SceneEdge) -> f64 {
    let (Some(from), Some(to)) =
        (node_floor_center(scene, edge.from.as_str()), node_floor_center(scene, edge.to.as_str()))
    else {
        return 1.0;
    };
    let span = (to.x - from.x).abs() + (to.y - from.y).abs();
    if edge.from == edge.to || span < DETOUR_MIN_SPAN {
        return 1.0;
    }
    let points = plan_samples(scene, edge);
    let length: f64 = points.windows(2).map(|pair| pair[0].distance(pair[1])).sum();
    length / span
}

/// How far (tiles) past its end nodes a route may run before it strays: the lead of a rail out of a face
/// square to the way it goes, and a hair (the router's own margin, `scene::rails`).
const STRAY_MARGIN: f64 = 0.5;
/// Gap (tiles) between two footprints along an axis under which the nodes stand in line on it (a route steps
/// aside along that axis to pass a node in its way, which is no stray).
const IN_LINE_GAP: f64 = 0.6;

/// Length (tiles, on the floor plan) of an edge's path past the box around its two nodes' footprints, grown by
/// [`STRAY_MARGIN`], along each axis on which the footprints stand apart (T-421): behind the start or beyond the
/// target, where a U to a neighbour or a walk round the target goes. Along an axis on which the nodes stand in
/// line (footprints closer than [`IN_LINE_GAP`]) a route may step aside to pass something in the way: not counted. 0 for self-loops.
pub fn edge_stray(scene: &Scene, edge: &SceneEdge) -> f64 {
    let (Some(from), Some(to)) = (node_floor_box(scene, edge.from.as_str()), node_floor_box(scene, edge.to.as_str()))
    else {
        return 0.0;
    };
    if edge.from == edge.to {
        return 0.0;
    }
    let range = |low_a: f64, high_a: f64, low_b: f64, high_b: f64| {
        if low_a.max(low_b) - high_a.min(high_b) < IN_LINE_GAP {
            (f64::NEG_INFINITY, f64::INFINITY)
        } else {
            (low_a.min(low_b) - STRAY_MARGIN, high_a.max(high_b) + STRAY_MARGIN)
        }
    };
    let (x_range, y_range) = (range(from.0.x, from.1.x, to.0.x, to.1.x), range(from.0.y, from.1.y, to.0.y, to.1.y));
    let inside =
        |point: PlanPoint| (x_range.0..=x_range.1).contains(&point.x) && (y_range.0..=y_range.1).contains(&point.y);
    plan_samples(scene, edge)
        .windows(2)
        .filter(|pair| !inside(PlanPoint::new((pair[0].x + pair[1].x) / 2.0, (pair[0].y + pair[1].y) / 2.0)))
        .map(|pair| pair[0].distance(pair[1]))
        .sum()
}

/// Floor box `(low, high)` of node `id`'s footprint (rotation applied).
pub(crate) fn node_floor_box(scene: &Scene, id: &str) -> Option<(PlanPoint, PlanPoint)> {
    let node = scene.nodes.iter().find(|node| node.id.as_str() == id)?;
    let center = node_floor_center(scene, id)?;
    let (half_width, half_depth) = node.shape.footprint.half_extents();
    Some((
        PlanPoint::new(center.x - half_width, center.y - half_depth),
        PlanPoint::new(center.x + half_width, center.y + half_depth),
    ))
}

/// Center of node `id` on the floor plan of the scene (rotation applied).
fn node_floor_center(scene: &Scene, id: &str) -> Option<PlanPoint> {
    let projection = Projection::new(scene.tile_px);
    scene.nodes.iter().find(|node| node.id.as_str() == id).map(|node| projection.unproject(node.floor_center, 0.0))
}

/// Lateral offsets (tiles) under which two axis legs count as aligned when working out the fewest bends.
const ALIGNED_TILES: f64 = 0.05;

/// Bends of an elbow route (a polyline on the plan axes) above the fewest that join its first leg to its last
/// one ([`fewest_bends`]); 0 for curves. A rail with two bends more than it needs reads as a step (an "S")
/// or a walk around something (T-421).
pub fn edge_excess_bends(scene: &Scene, edge: &SceneEdge) -> usize {
    if !matches!(edge.path, EdgePath::Polyline(_)) || edge.from == edge.to {
        return 0;
    }
    let stretches = straight_stretches(&plan_samples(scene, edge));
    let (Some(first), Some(last)) = (stretches.first(), stretches.last()) else { return 0 };
    let (Some(out), Some(arrive)) = (axis_of(first.0, first.1), axis_of(last.0, last.1)) else { return 0 };
    edge_bends(scene, edge).saturating_sub(fewest_bends(first.0, out, last.1, arrive))
}

/// The plan axis (unit vector) `from → to` runs along, if it runs along one.
fn axis_of(from: PlanPoint, to: PlanPoint) -> Option<(f64, f64)> {
    let (x, y) = (to.x - from.x, to.y - from.y);
    let length = x.hypot(y);
    if length <= BEND_MIN_PIECE {
        return None;
    }
    let (x, y) = (x / length, y / length);
    if x.abs() >= 1.0 - STRAIGHT_SINE {
        Some((x.signum(), 0.0))
    } else if y.abs() >= 1.0 - STRAIGHT_SINE {
        Some((0.0, y.signum()))
    } else {
        None
    }
}

/// Fewest 90° bends of an axis path that leaves `start` heading `out` and reaches `end` heading `arrive`
/// (unit axis vectors): 0 straight ahead, 1 for an L, 2 for a Z or an open U, 3 when the L would have to
/// double back, 4 for a Z that must turn around first.
pub fn fewest_bends(start: PlanPoint, out: (f64, f64), end: PlanPoint, arrive: (f64, f64)) -> usize {
    let offset = (end.x - start.x, end.y - start.y);
    let dot = |a: (f64, f64), b: (f64, f64)| a.0 * b.0 + a.1 * b.1;
    let ahead = dot(offset, out);
    let across = (offset.0 * out.1 - offset.1 * out.0).abs();
    let turn = dot(out, arrive);
    if turn > 0.5 {
        if across <= ALIGNED_TILES && ahead > 0.0 {
            0
        } else if ahead > 0.0 {
            2
        } else {
            4
        }
    } else if turn < -0.5 {
        if across <= ALIGNED_TILES { 4 } else { 2 }
    } else if ahead > ALIGNED_TILES && dot(offset, arrive) > ALIGNED_TILES {
        1
    } else {
        3
    }
}

/// Points of an edge's path on the floor plan.
pub(crate) fn plan_samples(scene: &Scene, edge: &SceneEdge) -> Vec<PlanPoint> {
    let projection = Projection::new(scene.tile_px);
    edge.path.sample(BEND_SAMPLES, BEND_STEP_PX).into_iter().map(|point| projection.unproject(point, EDGE_Z)).collect()
}

/// Two directions closer than this (sine of the angle) are parallel.
const PARALLEL_SINE: f64 = 0.05;
/// Pieces of a path turning less than this (sine) are merged into one straight stretch.
const STRAIGHT_SINE: f64 = 0.01;
/// Glued length (tiles) under which a pair of edges does not count: the arc of a rounded bend that turns
/// across the neighbouring lane is briefly near-parallel to it without running along it.
const GLUED_MIN: f64 = 0.1;
/// Gaps this much under [`RAIL_LANE`] still count as a full lane (about a pixel; the router keeps rails apart
/// by more, so a lane laid out a hair short is not counted).
const LANE_SLACK: f64 = 0.02;

/// Total length (tiles, on the floor plan) of stroke that runs parallel to a stroke of another edge closer
/// than one lane ([`RAIL_LANE`]), each glued stretch counted once; pairs glued over less than [`GLUED_MIN`]
/// do not count (T-407).
pub fn overlap_length(scene: &Scene) -> f64 {
    glued_pairs(scene).iter().fold(0.0, |total, &(_, _, glued)| total + glued)
}

/// The pairs of edges behind [`overlap_length`]: `(first, second, glued tiles)`, `first < second`, each pair
/// glued over at least [`GLUED_MIN`].
pub fn glued_pairs(scene: &Scene) -> Vec<(usize, usize, f64)> {
    let stretches: Vec<Vec<(PlanPoint, PlanPoint)>> =
        scene.edges.iter().map(|edge| straight_stretches(&plan_samples(scene, edge))).collect();
    let mut pairs = Vec::new();
    for (index, mine) in stretches.iter().enumerate() {
        for (offset, theirs) in stretches[index + 1..].iter().enumerate() {
            let glued: f64 = mine
                .iter()
                .flat_map(|&(a0, a1)| theirs.iter().map(move |&(b0, b1)| glued_length(a0, a1, b0, b1)))
                .sum();
            if glued >= GLUED_MIN {
                pairs.push((index, index + 1 + offset, glued));
            }
        }
    }
    pairs
}

/// A sampled path as straight stretches (pieces turning less than [`STRAIGHT_SINE`] merged).
pub(crate) fn straight_stretches(points: &[PlanPoint]) -> Vec<(PlanPoint, PlanPoint)> {
    let mut stretches: Vec<(PlanPoint, PlanPoint)> = Vec::new();
    for pair in points.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if start.distance(end) <= BEND_MIN_PIECE {
            continue;
        }
        if let Some(last) = stretches.last_mut()
            && sine_between(last.0, last.1, start, end) < STRAIGHT_SINE
        {
            last.1 = end;
            continue;
        }
        stretches.push((start, end));
    }
    stretches
}

/// |sin| of the angle between `a0 → a1` and `b0 → b1`.
fn sine_between(a0: PlanPoint, a1: PlanPoint, b0: PlanPoint, b1: PlanPoint) -> f64 {
    let (ax, ay, bx, by) = (a1.x - a0.x, a1.y - a0.y, b1.x - b0.x, b1.y - b0.y);
    let lengths = ax.hypot(ay) * bx.hypot(by);
    if lengths <= f64::EPSILON { 0.0 } else { (ax * by - ay * bx).abs() / lengths }
}

/// Length of `a0 → a1` running parallel to `b0 → b1` closer than one lane, measured along `a`.
fn glued_length(a0: PlanPoint, a1: PlanPoint, b0: PlanPoint, b1: PlanPoint) -> f64 {
    let length = a0.distance(a1);
    if length <= BEND_MIN_PIECE || sine_between(a0, a1, b0, b1) >= PARALLEL_SINE {
        return 0.0;
    }
    let (ux, uy) = ((a1.x - a0.x) / length, (a1.y - a0.y) / length);
    let along = |point: PlanPoint| (point.x - a0.x) * ux + (point.y - a0.y) * uy;
    let across = |point: PlanPoint| ((point.y - a0.y) * ux - (point.x - a0.x) * uy).abs();
    if across(b0).max(across(b1)) >= RAIL_LANE - LANE_SLACK {
        return 0.0;
    }
    let (low, high) = (along(b0).min(along(b1)), along(b0).max(along(b1)));
    (high.min(length) - low.max(0.0)).max(0.0)
}

/// How far (tiles, on the floor plan) an edge's path strays outside the rectangle spanned by its two ends:
/// 0 for an L or a Z, about a node's depth for a rail that arrives by a face behind its node, more for a route
/// that overshoots and comes back or goes around something (T-413).
pub fn edge_overshoot(scene: &Scene, edge: &SceneEdge) -> f64 {
    let points = plan_samples(scene, edge);
    let (Some(first), Some(last)) = (points.first(), points.last()) else { return 0.0 };
    let (low, high) = (
        PlanPoint::new(first.x.min(last.x), first.y.min(last.y)),
        PlanPoint::new(first.x.max(last.x), first.y.max(last.y)),
    );
    points
        .iter()
        .map(|point| {
            let outside_x = (low.x - point.x).max(point.x - high.x).max(0.0);
            let outside_y = (low.y - point.y).max(point.y - high.y).max(0.0);
            outside_x.max(outside_y)
        })
        .fold(0.0, f64::max)
}

/// Sides of a zone an edge must run along to count as going around it (T-413).
const WRAP_SIDES: usize = 3;
/// Share of a side's length an edge must run along, outside the zone, to count that side.
const WRAP_SIDE_SHARE: f64 = 0.5;
/// How far outside a side (tiles) a stretch still runs "along" it; a little inside counts too (the stroke sits
/// on the zone border).
const WRAP_BAND_OUT: f64 = 2.0;
const WRAP_BAND_IN: f64 = 0.3;

/// Edges that go around a whole zone: their path runs along at least three sides of one zone's floor
/// rectangle, over half of each side (T-413: "no edge circles a zone"). Returns `(edge index, zone index)`.
pub fn zone_wraps(scene: &Scene) -> Vec<(usize, usize)> {
    let projection = Projection::new(scene.tile_px);
    let zones: Vec<(PlanPoint, PlanPoint)> = scene
        .zones
        .iter()
        .map(|zone| {
            let corners = zone.polygon.map(|corner| projection.unproject(corner, 0.0));
            let low = corners
                .iter()
                .fold(PlanPoint::new(f64::MAX, f64::MAX), |low, c| PlanPoint::new(low.x.min(c.x), low.y.min(c.y)));
            let high = corners
                .iter()
                .fold(PlanPoint::new(f64::MIN, f64::MIN), |high, c| PlanPoint::new(high.x.max(c.x), high.y.max(c.y)));
            (low, high)
        })
        .collect();
    let mut wraps = Vec::new();
    for (edge_index, edge) in scene.edges.iter().enumerate() {
        let stretches = straight_stretches(&plan_samples(scene, edge));
        for (zone_index, &(low, high)) in zones.iter().enumerate() {
            if sides_run_along(&stretches, low, high) >= WRAP_SIDES {
                wraps.push((edge_index, zone_index));
            }
        }
    }
    wraps
}

/// How many sides of the rectangle `low..high` the stretches run along (over [`WRAP_SIDE_SHARE`] of the side,
/// within the band just outside it).
fn sides_run_along(stretches: &[(PlanPoint, PlanPoint)], low: PlanPoint, high: PlanPoint) -> usize {
    // (fixed coordinate is x?, side line, outward sign, span low, span high)
    let sides = [
        (false, low.y, -1.0, low.x, high.x),
        (false, high.y, 1.0, low.x, high.x),
        (true, low.x, -1.0, low.y, high.y),
        (true, high.x, 1.0, low.y, high.y),
    ];
    sides
        .iter()
        .filter(|&&(vertical, line, outward, from, to)| {
            let length = to - from;
            if length <= BEND_MIN_PIECE {
                return false;
            }
            let covered: f64 = stretches
                .iter()
                .map(|&(a, b)| {
                    let (fixed_a, fixed_b, run_a, run_b) =
                        if vertical { (a.x, b.x, a.y, b.y) } else { (a.y, b.y, a.x, b.x) };
                    if (fixed_a - fixed_b).abs() > PARALLEL_SINE * (run_a - run_b).abs() {
                        return 0.0;
                    }
                    let offset = ((fixed_a + fixed_b) / 2.0 - line) * outward;
                    if !(-WRAP_BAND_IN..=WRAP_BAND_OUT).contains(&offset) {
                        return 0.0;
                    }
                    (run_a.max(run_b).min(to) - run_a.min(run_b).max(from)).max(0.0)
                })
                .sum();
            covered >= WRAP_SIDE_SHARE * length
        })
        .count()
}

/// Where the sources of the scene stand in its reading order, from 0 (their centroid is read first: top left
/// of the screen) to 1 (last: bottom right), measured along screen `x + y` over the floor centers of the nodes
/// (T-415). Sources are the nodes no edge enters (self-loops aside) that some edge leaves; `None` without any.
pub fn source_reading_rank(scene: &Scene) -> Option<f64> {
    let projection = Projection::new(scene.tile_px);
    let centers: Vec<PlanPoint> = scene.nodes.iter().map(|node| projection.unproject(node.floor_center, 0.0)).collect();
    let entered = |node: &dioramic_core::scene::SceneNode| {
        scene.edges.iter().any(|edge| edge.to == node.id && edge.from != edge.to)
    };
    let leaves = |node: &dioramic_core::scene::SceneNode| scene.edges.iter().any(|edge| edge.from == node.id);
    let sources: Vec<usize> = scene
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| !entered(node) && leaves(node))
        .map(|(index, _)| index)
        .collect();
    dioramic_core::layout::source_reading_rank(&centers, &sources)
}

/// Stroke (px) of an edge inside a node label box above which the edge reads as running under the label.
const UNDER_LABEL_MIN_PX: f64 = 4.0;
/// Samples and longest step (px) used to follow an edge under the label boxes.
const LABEL_SAMPLES: usize = 64;
const LABEL_STEP_PX: f64 = 1.0;

/// The edges whose stroke runs under a node's label box (over [`UNDER_LABEL_MIN_PX`] of it), as
/// `(edge index, node index)` (T-422): the label hides the line, which only shows again past the box.
pub fn edges_under_node_labels(scene: &Scene) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    for (edge_index, edge) in scene.edges.iter().enumerate() {
        let points = edge.path.sample(LABEL_SAMPLES, LABEL_STEP_PX);
        for (node_index, node) in scene.nodes.iter().enumerate() {
            let Some(label) = &node.label else { continue };
            let inside: f64 = points
                .windows(2)
                .filter(|pair| label.rect.contains(pair[0].midpoint(pair[1])))
                .map(|pair| pair[0].distance(pair[1]))
                .sum();
            if inside > UNDER_LABEL_MIN_PX {
                pairs.push((edge_index, node_index));
            }
        }
    }
    pairs
}

/// Number of `W_EDGE_THROUGH_NODE` diagnostics.
pub fn edge_through_node_count(diagnostics: &Diagnostics) -> usize {
    diagnostics.iter().filter(|diagnostic| diagnostic.code == DiagnosticCode::EdgeThroughNode).count()
}

/// Length of the longest node leader line (px), 0 without leaders.
pub fn max_leader_px(scene: &Scene) -> f64 {
    scene.nodes.iter().filter_map(|node| node.leader).map(|(start, end)| start.distance(end)).fold(0.0, f64::max)
}

/// Node label font size as a percentage of the viewBox width (0 for an empty viewBox).
pub fn text_to_width_percent(scene: &Scene) -> f64 {
    let width = scene.viewbox.width();
    if width <= f64::EPSILON {
        return 0.0;
    }
    scene.theme.typography.node_label_size / width * PERCENT
}

#[cfg(test)]
mod tests {
    use dioramic_core::geometry::ScreenPoint;
    use dioramic_spec::Diagnostic;

    use super::*;
    use crate::{View, fixture};

    #[test]
    fn glued_parallel_strokes_are_measured_once_along_the_overlap() {
        let point = PlanPoint::new;
        assert!((glued_length(point(0.0, 0.0), point(3.0, 0.0), point(1.0, 0.1), point(5.0, 0.1)) - 2.0).abs() < 1e-9);
        assert_eq!(
            glued_length(point(0.0, 0.0), point(3.0, 0.0), point(1.0, 0.4), point(5.0, 0.4)),
            0.0,
            "a lane apart"
        );
        assert_eq!(glued_length(point(0.0, 0.0), point(3.0, 0.0), point(1.0, -1.0), point(1.0, 1.0)), 0.0, "crossing");
        let bent = [point(0.0, 0.0), point(1.0, 0.0), point(2.0, 0.0), point(2.0, 1.0)];
        assert_eq!(straight_stretches(&bent).len(), 2);
    }

    #[test]
    fn fewest_bends_follow_the_end_faces() {
        let point = PlanPoint::new;
        let (east, south, west) = ((1.0, 0.0), (0.0, 1.0), (-1.0, 0.0));
        assert_eq!(fewest_bends(point(0.0, 0.0), east, point(5.0, 0.0), east), 0, "straight ahead");
        assert_eq!(fewest_bends(point(0.0, 0.0), east, point(5.0, 2.0), east), 2, "a Z");
        assert_eq!(fewest_bends(point(0.0, 0.0), east, point(-5.0, 2.0), east), 4, "turn round first");
        assert_eq!(fewest_bends(point(0.0, 0.0), east, point(3.0, 3.0), south), 1, "an L");
        assert_eq!(fewest_bends(point(0.0, 0.0), east, point(-3.0, 3.0), south), 3, "an L behind");
        assert_eq!(fewest_bends(point(0.0, 0.0), east, point(0.0, 3.0), west), 2, "an open U");
    }

    #[test]
    fn a_label_box_over_a_stroke_is_found() {
        let mut scene = fixture("checkout").scene(View::default());
        for node in &mut scene.nodes {
            node.label = None;
        }
        assert!(edges_under_node_labels(&scene).is_empty());
        let middle = scene.edges[0].path.point_at_fraction(0.5);
        let rect =
            dioramic_core::geometry::Rect::new(middle.x - 30.0, middle.y - 10.0, middle.x + 30.0, middle.y + 10.0);
        let label = dioramic_core::scene::PlacedLabel {
            kind: dioramic_core::scene::LabelKind::Node,
            lines: vec!["Label".to_owned()],
            rect,
            size: (60.0, 20.0),
            transform: None,
        };
        scene.nodes[0].label = Some(label);
        assert!(edges_under_node_labels(&scene).contains(&(0, 0)), "{:?}", edges_under_node_labels(&scene));
    }

    #[test]
    fn counts_only_edge_through_node_diagnostics() {
        let mut diagnostics = Diagnostics::new();
        diagnostics.push(Diagnostic::new(DiagnosticCode::EdgeThroughNode, "edges[0]", "a"));
        diagnostics.push(Diagnostic::new(DiagnosticCode::LabelOverlap, "nodes[0]", "b"));
        diagnostics.push(Diagnostic::new(DiagnosticCode::EdgeThroughNode, "edges[1]", "c"));
        assert_eq!(edge_through_node_count(&diagnostics), 2);
        assert_eq!(edge_through_node_count(&Diagnostics::new()), 0);
    }

    #[test]
    fn leader_and_text_ratio_follow_the_scene() {
        let mut scene = fixture("checkout").scene(View::default());
        let ratio = text_to_width_percent(&scene);
        assert!(ratio > 0.1 && ratio < 10.0, "text/width {ratio}% is implausible");

        for node in &mut scene.nodes {
            node.leader = None;
        }
        assert_eq!(max_leader_px(&scene), 0.0);
        if let Some(node) = scene.nodes.first_mut() {
            node.leader = Some((ScreenPoint { x: 0.0, y: 0.0 }, ScreenPoint { x: 3.0, y: 4.0 }));
        }
        assert_eq!(max_leader_px(&scene), 5.0);

        scene.viewbox.x1 = scene.viewbox.x0;
        assert_eq!(text_to_width_percent(&scene), 0.0);
    }
}
