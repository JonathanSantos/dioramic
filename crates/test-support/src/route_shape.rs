//! The shape of an elbow route as a reader sees it (T-425): the steps ("S") and hooks next to a port, the
//! U-turns, and the arrivals by the back of the target when a front face was within an L or a Z. The quality
//! numbers of [`crate::quality`] measure lengths and bends; these name the defects the user pointed at in the
//! `schematic` views ("minimum turns, no detours").

use dioramic_core::geometry::{COS_30, PlanPoint, Projection, SIN_30};
use dioramic_core::scene::{EDGE_Z, EdgePath, Scene, SceneEdge};

use crate::quality::{node_floor_box, plan_samples, straight_stretches};

/// Straight stretches shorter than this (tiles) are part of a rounded bend, not a leg.
const LEG_MIN: f64 = 0.02;
/// A stretch whose direction is off a plan axis by more than this (sine) is no leg of an elbow.
const AXIS_SINE: f64 = 0.01;
/// A step ("S", two opposite bends) with a leg shorter than this (tiles, corner to corner) only shifts the rail
/// over by a lane or two, or turns again right at a port: it reads as a jog, not as a Z between two nodes
/// (the router's own limit, `scene::rails`).
pub const STEP_LEG: f64 = 1.2;
/// A step whose first or last leg (port to corner) is shorter than this (tiles) turns again right after leaving
/// or right before reaching a port: an "S" at the port. A Z through the corridor between two rows of nodes keeps
/// longer end legs.
pub const STEP_END_LEG: f64 = 0.8;
/// A Z whose two bends are sharp (on screen the rail runs back and forth: up-right, up-left, up-right) is a
/// zigzag, a step, unless its middle leg is at least this long (tiles); a Z with gentle bends reads as a stair.
pub const ZIGZAG_LEG: f64 = 2.75;
/// A sharp bend (the two legs run back against each other on screen) followed by a last leg (or preceded by a
/// first leg) shorter than this (tiles, corner to port) reads as a hook at the port: the rail passes the node and
/// turns back into it.
pub const HOOK_LEG: f64 = 1.5;
/// Room (tiles) a straight candidate leg keeps from the footprints of the nodes it passes.
const CANDIDATE_CLEARANCE: f64 = 0.1;

/// The defects of one route's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RouteShape {
    /// Steps: a Z (two opposite bends) around a middle leg shorter than [`STEP_LEG`], with a first or last leg
    /// shorter than [`STEP_END_LEG`], or zigzagging (both bends sharp) around a middle leg shorter than
    /// [`ZIGZAG_LEG`].
    pub steps: usize,
    /// U-turns: a leg that runs back against the one two legs before it, whatever the lengths.
    pub u_turns: usize,
    /// Hooks at a port: a first or last leg shorter than [`HOOK_LEG`] next to a sharp bend, or followed by a leg
    /// running back against it (the rail turns round right at the port).
    pub port_hooks: usize,
}

impl RouteShape {
    /// True when the route has none of the defects.
    pub fn is_clean(&self) -> bool {
        *self == Self::default()
    }
}

/// Corners of an elbow route on the floor plan: its two ends and the corner of every bend (where its two legs
/// would meet without the rounding). A jog too short to keep a straight stretch between its two rounded bends
/// (a lane over) still counts as its two corners. `None` for curves and self-loops.
pub fn edge_corners(scene: &Scene, edge: &SceneEdge) -> Option<Vec<PlanPoint>> {
    if !matches!(edge.path, EdgePath::Polyline(_)) || edge.from == edge.to {
        return None;
    }
    let points = plan_samples(scene, edge);
    let (first, last) = stroke_ends(scene, edge, *points.first()?, *points.last()?);
    let mut legs: Vec<Leg> = Vec::new();
    for (start, end) in straight_stretches(&points) {
        let Some(axis) = Axis::of(start, end) else { continue };
        let leg = Leg { axis, start, end };
        match legs.last_mut() {
            Some(previous) if previous.axis == axis && previous.lateral_gap(&leg) < LEG_MIN => previous.end = end,
            _ => legs.push(leg),
        }
    }
    let mut corners = vec![first];
    for pair in legs.windows(2) {
        corners.extend(pair[0].corners_to(&pair[1]));
    }
    corners.push(last);
    Some(corners)
}

/// Where the stroke of `edge` starts and ends on the floor plan: the ends `first` and `last` of its path, or the
/// tips of its end heads when the heads are drawn midway (the stroke then runs on from the path's end, the base
/// of the unpainted head, to its tip at the port).
fn stroke_ends(scene: &Scene, edge: &SceneEdge, first: PlanPoint, last: PlanPoint) -> (PlanPoint, PlanPoint) {
    if edge.midway.is_empty() {
        return (first, last);
    }
    let projection = Projection::new(scene.tile_px);
    let (mut start, mut end) = (first, last);
    for head in &edge.heads {
        let (tip, base) =
            (projection.unproject(head.tip, EDGE_Z), projection.unproject(head.base[0].midpoint(head.base[1]), EDGE_Z));
        if base.distance(last) <= base.distance(first) {
            end = tip;
        } else {
            start = tip;
        }
    }
    (start, end)
}

/// A straight stretch of a route along a plan axis.
#[derive(Debug, Clone, Copy)]
struct Leg {
    axis: Axis,
    start: PlanPoint,
    end: PlanPoint,
}

impl Leg {
    /// Coordinate of the leg's line across its axis.
    fn lateral(&self) -> f64 {
        if self.axis.is_along_x() { self.start.y } else { self.start.x }
    }

    /// Coordinate of `point` along the leg's axis.
    fn along(&self, point: PlanPoint) -> f64 {
        if self.axis.is_along_x() { point.x } else { point.y }
    }

    /// The point of the leg's line at `along`.
    fn at(&self, along: f64) -> PlanPoint {
        if self.axis.is_along_x() {
            PlanPoint::new(along, self.lateral())
        } else {
            PlanPoint::new(self.lateral(), along)
        }
    }

    /// Distance between the lines of two parallel legs.
    fn lateral_gap(&self, other: &Leg) -> f64 {
        (self.lateral() - other.lateral()).abs()
    }

    /// Corners between this leg and the next one: where they meet when they are perpendicular; two corners, half
    /// way between their ends, when they are parallel (a jog or a U-turn whose middle leg is all bend).
    fn corners_to(&self, next: &Leg) -> Vec<PlanPoint> {
        if self.axis.is_along_x() != next.axis.is_along_x() {
            return vec![next.at(self.lateral())];
        }
        let middle = (self.along(self.end) + self.along(next.start)) / 2.0;
        vec![self.at(middle), next.at(middle)]
    }
}

/// The shape defects of `edge` ([`RouteShape`]); clean for curves.
pub fn edge_route_shape(scene: &Scene, edge: &SceneEdge) -> RouteShape {
    edge_corners(scene, edge).map_or_else(RouteShape::default, |corners| route_shape(&corners))
}

/// The shape defects of the axis route through `corners`.
pub fn route_shape(corners: &[PlanPoint]) -> RouteShape {
    let legs: Vec<(f64, f64)> = corners.windows(2).map(|pair| (pair[1].x - pair[0].x, pair[1].y - pair[0].y)).collect();
    let length = |leg: (f64, f64)| leg.0.hypot(leg.1);
    let dot = |a: (f64, f64), b: (f64, f64)| a.0 * b.0 + a.1 * b.1;
    let mut shape = RouteShape::default();
    let last = legs.len().saturating_sub(3);
    for (index, triple) in legs.windows(3).enumerate() {
        let along = dot(triple[0], triple[2]);
        let short_end =
            (index == 0 && length(triple[0]) < STEP_END_LEG) || (index == last && length(triple[2]) < STEP_END_LEG);
        if along < 0.0 {
            shape.u_turns += 1;
        } else if along > 0.0 && (length(triple[1]) < STEP_LEG || short_end || is_zigzag(triple)) {
            shape.steps += 1;
        }
    }
    if legs.len() >= 2 {
        let reversed: Vec<(f64, f64)> = legs.iter().rev().map(|&(x, y)| (-x, -y)).collect();
        shape.port_hooks = [&legs, &reversed]
            .iter()
            .filter(|legs| {
                let (end, next) = (legs[0], legs[1]);
                let turns_back = legs.get(2).is_some_and(|&after| dot(after, end) < 0.0);
                length(end) < HOOK_LEG && (screen_dot(end, next) < 0.0 || turns_back)
            })
            .count();
    }
    shape
}

/// A Z whose two bends are sharp on screen (the rail runs back and forth) around a middle leg shorter than
/// [`ZIGZAG_LEG`].
fn is_zigzag(legs: &[(f64, f64)]) -> bool {
    screen_dot(legs[0], legs[1]) < 0.0 && legs[1].0.hypot(legs[1].1) < ZIGZAG_LEG
}

/// Dot product of two plan directions as drawn on screen (rotation applied, so the scene's plan is the
/// screen's): negative when the two legs run back against each other (a sharp bend).
fn screen_dot(a: (f64, f64), b: (f64, f64)) -> f64 {
    let screen = |(x, y): (f64, f64)| {
        let norm = x.hypot(y).max(f64::EPSILON);
        ((x - y) / norm * COS_30, (x + y) / norm * SIN_30)
    };
    let (a, b) = (screen(a), screen(b));
    a.0 * b.0 + a.1 * b.1
}

/// True when `edge` passes its target and comes back into it from behind (T-425): its last leg enters a face
/// hidden behind the target's body (a face looking away from the viewer: the leg heads down-right or down-left on
/// screen) after a sharp bend (on screen the leg before it ran past the target), although its source stands in
/// front of the target and an L or a Z free of other nodes reaches a front face.
pub fn edge_arrives_behind(scene: &Scene, edge: &SceneEdge) -> bool {
    let Some(corners) = edge_corners(scene, edge) else { return false };
    let [.., earlier, before, end] = corners[..] else { return false };
    let (arriving, previous) = ((end.x - before.x, end.y - before.y), (before.x - earlier.x, before.y - earlier.y));
    let behind = arriving.0 > AXIS_SINE * arriving.1.abs() || arriving.1 > AXIS_SINE * arriving.0.abs();
    // Into a back face after a sharp bend: the rail went past the target on screen and turns back into it.
    if !behind || screen_dot(arriving, previous) >= 0.0 {
        return false;
    }
    let (Some(source), Some(target)) =
        (node_floor_box(scene, edge.from.as_str()), node_floor_box(scene, edge.to.as_str()))
    else {
        return false;
    };
    let others: Vec<(PlanPoint, PlanPoint)> = scene
        .nodes
        .iter()
        .filter(|node| node.id != edge.from && node.id != edge.to)
        .filter_map(|node| node_floor_box(scene, node.id.as_str()))
        .collect();
    front_routes(source, target).iter().any(|route| route_is_free(route, &others))
}

/// Candidate L and Z routes (center to center) from the node on `source` to a front face of the node on
/// `target` (footprint boxes `(low, high)`): only along an axis on which the source stands in front.
fn front_routes(source: (PlanPoint, PlanPoint), target: (PlanPoint, PlanPoint)) -> Vec<Vec<PlanPoint>> {
    let center = |(low, high): (PlanPoint, PlanPoint)| PlanPoint::new((low.x + high.x) / 2.0, (low.y + high.y) / 2.0);
    let (from, to) = (center(source), center(target));
    let mut routes = Vec::new();
    if source.0.x > target.1.x {
        // Arriving by the +x face, heading −x.
        let middle = (source.0.x + target.1.x) / 2.0;
        routes.push(vec![from, PlanPoint::new(from.x, to.y), to]);
        routes.push(vec![from, PlanPoint::new(middle, from.y), PlanPoint::new(middle, to.y), to]);
    }
    if source.0.y > target.1.y {
        // Arriving by the +y face, heading −y.
        let middle = (source.0.y + target.1.y) / 2.0;
        routes.push(vec![from, PlanPoint::new(to.x, from.y), to]);
        routes.push(vec![from, PlanPoint::new(from.x, middle), PlanPoint::new(to.x, middle), to]);
    }
    routes
}

/// True when no leg of `route` runs over one of the footprint boxes `others`.
fn route_is_free(route: &[PlanPoint], others: &[(PlanPoint, PlanPoint)]) -> bool {
    route.windows(2).all(|leg| {
        let (low, high) = (
            PlanPoint::new(leg[0].x.min(leg[1].x), leg[0].y.min(leg[1].y)),
            PlanPoint::new(leg[0].x.max(leg[1].x), leg[0].y.max(leg[1].y)),
        );
        others.iter().all(|(other_low, other_high)| {
            low.x > other_high.x + CANDIDATE_CLEARANCE
                || high.x < other_low.x - CANDIDATE_CLEARANCE
                || low.y > other_high.y + CANDIDATE_CLEARANCE
                || high.y < other_low.y - CANDIDATE_CLEARANCE
        })
    })
}

/// A plan axis direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    PlusX,
    PlusY,
    MinusX,
    MinusY,
}

impl Axis {
    /// The axis `start → end` runs along, if it runs along one and is long enough to be a leg.
    fn of(start: PlanPoint, end: PlanPoint) -> Option<Self> {
        let (x, y) = (end.x - start.x, end.y - start.y);
        let length = x.hypot(y);
        if length < LEG_MIN {
            return None;
        }
        if y.abs() <= AXIS_SINE * length {
            Some(if x > 0.0 { Self::PlusX } else { Self::MinusX })
        } else if x.abs() <= AXIS_SINE * length {
            Some(if y > 0.0 { Self::PlusY } else { Self::MinusY })
        } else {
            None
        }
    }

    fn is_along_x(self) -> bool {
        matches!(self, Self::PlusX | Self::MinusX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corners(points: &[(f64, f64)]) -> Vec<PlanPoint> {
        points.iter().map(|&(x, y)| PlanPoint::new(x, y)).collect()
    }

    #[test]
    fn an_l_and_a_long_z_are_clean() {
        assert!(route_shape(&corners(&[(0.0, 0.0), (4.0, 0.0), (4.0, -3.0)])).is_clean());
        assert!(route_shape(&corners(&[(0.0, 0.0), (2.0, 0.0), (2.0, 3.0), (4.0, 3.0)])).is_clean());
    }

    #[test]
    fn a_z_with_a_short_leg_is_a_step() {
        // T-424 web-platform `mobile -> internet`: up, a lane over, up again into the cloud.
        let step = route_shape(&corners(&[(1.5, 8.1), (1.5, 5.7), (2.7, 5.7), (2.7, 5.1)]));
        assert_eq!(step.steps, 1, "{step:?}");
        assert_eq!(step.u_turns, 0);
    }

    #[test]
    fn a_leg_back_against_the_one_before_last_is_a_u_turn() {
        let u = route_shape(&corners(&[(0.0, 0.0), (0.0, 3.0), (3.0, 3.0), (3.0, 0.5)]));
        assert_eq!(u.u_turns, 1, "{u:?}");
    }

    #[test]
    fn a_short_last_leg_after_a_sharp_bend_is_a_hook() {
        // T-424 web-platform `web -> internet`: up-right on screen (−y), then a short leg up-left (−x).
        let hook = route_shape(&corners(&[(4.4, 8.1), (4.4, 4.8), (3.6, 4.8)]));
        assert_eq!(hook.port_hooks, 1, "{hook:?}");
        // The same bend with a long last leg is a plain L; a gentle bend (−y then +x) never hooks.
        assert!(route_shape(&corners(&[(4.4, 8.1), (4.4, 4.8), (1.6, 4.8)])).is_clean());
        assert!(route_shape(&corners(&[(4.4, 8.1), (4.4, 4.8), (5.0, 4.8)])).is_clean());
    }

    #[test]
    fn front_routes_need_the_source_in_front() {
        let target = (PlanPoint::new(0.0, 0.0), PlanPoint::new(1.0, 1.0));
        let behind = (PlanPoint::new(-3.0, -3.0), PlanPoint::new(-2.0, -2.0));
        assert!(front_routes(behind, target).is_empty());
        let in_front = (PlanPoint::new(3.0, -3.0), PlanPoint::new(4.0, -2.0));
        assert_eq!(front_routes(in_front, target).len(), 2);
        let blocker = (PlanPoint::new(3.0, 0.0), PlanPoint::new(4.0, 1.0));
        let routes = front_routes(in_front, target);
        assert!(!route_is_free(&routes[0], &[blocker]), "the L runs over the node at its corner");
        assert!(route_is_free(&routes[1], &[blocker]), "the Z passes beside it");
    }
}
