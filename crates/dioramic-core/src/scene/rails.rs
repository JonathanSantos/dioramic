//! `routing: rails` (T-403, elbows since T-406): isometric rails.
//!
//! An edge on rails leaves its port square to the face, runs on straight legs along the plan axes only
//! (on screen: the lines of the iso grid), turns with a constant rounded 90° bend and arrives square to
//! the target face, with as few bends as it can ([`super::elbow`] finds them). Everything is built in the
//! plan and projected, so the bends are iso ellipse arcs that lie on the floor like the zones.

use std::f64::consts::{PI, TAU};

use crate::geometry::{COS_30, PlanPoint, PlanRect, PlanVector, Projection, SIN_30};
use crate::icons::Footprint;

use super::ports::{angle_of, direction_at, face_normal};
use super::{EDGE_Z, EdgePath};

/// Straight lead out of / into a port (tiles), so the rail meets the component square to its face: the
/// shortest stub before the first bend and before the head (T-407).
pub const RAIL_LEAD: f64 = 0.4;
/// Radius of every bend (tiles); smaller when the legs around it are short.
pub const RAIL_BEND_RADIUS: f64 = 0.4;
/// Radius of the bends of `routing: orthogonal`, the same elbows with tighter corners (T-407).
pub const ORTHOGONAL_BEND_RADIUS: f64 = 0.15;
/// Comfortable clearance between a rail and any node it does not connect (tiles).
pub const RAIL_CLEARANCE: f64 = 0.35;
/// Points sampled on each rounded bend.
const BEND_SAMPLES: usize = 8;
/// Cosine above which two legs are collinear (no bend).
const COLLINEAR_COSINE: f64 = 0.9995;
/// Geometry costs (route cost units): length, and each bend (fewer bends read cleaner: one bend saved is
/// worth five tiles of length).
const LENGTH_COST: f64 = 6.0;
/// Cost of one bend (see [`LENGTH_COST`]).
pub const BEND_COST: f64 = 30.0;
/// Longest rail relative to the Manhattan distance between its ends that reads as direct (T-407).
pub const DETOUR_LIMIT: f64 = 1.4;
/// Cost per tile of rail beyond [`DETOUR_LIMIT`] (a detour reads worse than a long straight rail).
const DETOUR_COST: f64 = 1000.0;
/// Length (tiles) a rail may exceed [`DETOUR_LIMIT`] by before it counts as a detour (rounding).
const DETOUR_SLACK: f64 = 1e-6;
/// Gap (tiles) between two footprints along an axis under which the nodes stand in line on it: a rail
/// passing a node in its way steps aside along that axis, which is no detour (T-421).
const IN_LINE_GAP: f64 = 0.6;
/// Stray length (tiles) that still reads as staying between the end nodes (a rounded corner, rounding).
const STRAY_SLACK: f64 = 0.05;
/// Most bends a rail takes without reading as a detour (T-407).
pub const BENDS_LIMIT: usize = 3;
/// Cost of every bend beyond [`BENDS_LIMIT`], on top of [`BEND_COST`].
const EXTRA_BEND_COST: f64 = 800.0;
/// A half turn around a leg shorter than this (tiles) reads as a hook.
const SHORT_U_TURN_LEG: f64 = 1.2;
/// Cost of such a hook (more than two extra bends: an open U or another face is better).
const SHORT_U_TURN_COST: f64 = 80.0;
/// How far (tiles) past the box around its two end nodes a rail may run before it counts as straying: a lead
/// out of a face square to the way it goes stays inside (T-421).
pub const STRAY_MARGIN: f64 = RAIL_LEAD + 0.1;
/// Cost per tile of rail beyond [`STRAY_MARGIN`] of its two end nodes: a U that runs past its neighbour and
/// comes back, or a walk around a node to reach a face on its far side, reads as a detour even when the ports
/// it joins are far apart (T-421). More than a bend per tile, so the fewest bends that stay between the
/// nodes win.
const STRAY_COST: f64 = 45.0;
/// A step (two opposite bends) around a leg shorter than this (tiles) only moves the rail over a lane or two:
/// it reads as an "S" just before the port (T-421).
const SHORT_JOG_LEG: f64 = 1.2;
/// A step whose first or last leg (port to corner) is shorter than this (tiles) turns again right after its
/// lead: an "S" at the port (T-425). A Z through the corridor between two rows of nodes keeps longer end legs.
const SHORT_END_LEG: f64 = 0.8;
/// A Z whose two bends are sharp (on screen the rail runs back and forth: up-right, up-left, up-right) reads as a
/// zigzag next to a node unless its middle leg is at least this long (tiles); a Z with gentle bends is a stair
/// step and only needs [`SHORT_JOG_LEG`] (T-425).
const ZIGZAG_LEG: f64 = 2.75;
/// Cost of such a step on top of its two bends: a straight rail in another lane, or a port slid over, is
/// better.
const SHORT_JOG_COST: f64 = 60.0;
/// A first or last leg (port to corner) shorter than this (tiles) next to a sharp bend, one whose two legs run
/// back against each other on screen, reads as a hook at the port: the rail passes the node and turns back
/// into it (T-425: the web-platform clients arriving at the cloud).
const PORT_HOOK_LEG: f64 = 1.5;
/// Cost of such a hook: more than another bend and a face turn, so a Z with long legs or another face wins.
const PORT_HOOK_COST: f64 = 60.0;
/// Cost per tile of leg closer than [`RAIL_CLEARANCE`] to a node it does not connect.
const CLEARANCE_COST: f64 = 60.0;
/// Samples per tile when measuring clearance.
const CLEARANCE_SAMPLES_PER_TILE: f64 = 8.0;

/// Distance (tiles, along a face) between the ports of two rails on the same face: their lanes.
const RAIL_SLOT_SPACING: f64 = 0.34;
/// Share of a face the ports of its rails may spread over (the corners stay free).
pub const RAIL_FACE_USE: f64 = 0.8;

/// An end of a rail at a node, before its port is placed on a face.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceEnd {
    /// Border angle chosen so far (its face is the face crossed in that direction).
    pub angle: f64,
    /// Plan angle from the node toward the node at the other end of the edge.
    pub toward: f64,
}

/// Lays out the ends of rails that share a face of one node (T-406): arrivals and departures alike get
/// their own lane, evenly spaced around the middle of the face and ordered like the nodes they lead to,
/// so two rails of one node neither start on the same spot nor cross on their way out. Ends alone on
/// their face keep their angle. Returns the new angle of every end, in input order.
pub fn share_faces(footprint: &Footprint, ends: &[FaceEnd]) -> Vec<f64> {
    let mut angles: Vec<f64> = ends.iter().map(|end| end.angle).collect();
    let normals: Vec<PlanVector> = ends.iter().map(|end| face_normal(footprint, direction_at(end.angle))).collect();
    for normal in
        [PlanVector::new(1.0, 0.0), PlanVector::new(0.0, 1.0), PlanVector::new(-1.0, 0.0), PlanVector::new(0.0, -1.0)]
    {
        let mut members: Vec<usize> = (0..ends.len()).filter(|&index| normals[index] == normal).collect();
        if members.len() < 2 {
            continue;
        }
        let axis = angle_of(normal);
        // Increasing angle runs along the face tangent: order by where the other node lies seen from the face.
        let key = |index: usize| wrap_angle(ends[index].toward - axis);
        members.sort_by(|&a, &b| key(a).total_cmp(&key(b)).then(a.cmp(&b)));
        for (rank, angle) in members.iter().zip(face_slots(footprint, normal, members.len())) {
            angles[*rank] = angle;
        }
    }
    angles
}

/// Angles of `count` ports spread evenly around the middle of the face with outward `normal`.
fn face_slots(footprint: &Footprint, normal: PlanVector, count: usize) -> Vec<f64> {
    let tangent = PlanVector::new(-normal.y, normal.x);
    let (half_width, half_depth) = footprint.half_extents();
    let (along_normal, along_face) = if normal.x != 0.0 { (half_width, half_depth) } else { (half_depth, half_width) };
    let spacing = RAIL_SLOT_SPACING.min(2.0 * along_face * RAIL_FACE_USE / (count - 1) as f64);
    (0..count)
        .map(|rank| {
            let offset = (rank as f64 - (count - 1) as f64 / 2.0) * spacing;
            match footprint {
                Footprint::Circle { radius } => angle_of(normal) + offset / radius,
                Footprint::Rect { .. } => angle_of(normal * along_normal + tangent * offset),
            }
        })
        .collect()
}

/// `angle` wrapped into `(-π, π]`.
fn wrap_angle(angle: f64) -> f64 {
    let wrapped = angle.rem_euclid(TAU);
    if wrapped > PI { wrapped - TAU } else { wrapped }
}

/// A rail in the plan: its corner points (before rounding) and its rounded centerline.
#[derive(Debug, Clone, PartialEq)]
pub struct RailGeometry {
    /// Start, lead end, corners, lead start, end (collinear points removed).
    pub corners: Vec<PlanPoint>,
    /// Rounded centerline.
    pub points: Vec<PlanPoint>,
}

impl RailGeometry {
    /// Rail through `corners`, rounded with [`RAIL_BEND_RADIUS`].
    #[cfg(test)]
    pub fn from_corners(corners: Vec<PlanPoint>) -> Self {
        Self::from_corners_with_radius(corners, RAIL_BEND_RADIUS)
    }

    /// Rail through `corners` whose bends have radius `radius` (tiles).
    pub fn from_corners_with_radius(corners: Vec<PlanPoint>, radius: f64) -> Self {
        let corners = without_collinear(&corners);
        let points = rounded(&corners, radius);
        Self { corners, points }
    }

    /// Screen path.
    pub fn project(&self, projection: &Projection) -> EdgePath {
        EdgePath::Polyline(self.points.iter().map(|point| projection.project(*point, EDGE_Z)).collect())
    }

    /// Geometric cost: length, bends, stretches closer than [`RAIL_CLEARANCE`] to the `others` (footprints of
    /// nodes the edge does not connect), steps and stretches past the box around the two `ends` (footprints of
    /// the nodes it connects).
    /// `reach` is how far (tiles) the stroke runs on past the first and the last corner point (to the tip of a head
    /// whose base the rail ends at).
    pub fn cost(&self, others: &[PlanRect], ends: [PlanRect; 2], reach: [f64; 2]) -> f64 {
        let length = self.length();
        let extra_bends = self.bends().saturating_sub(BENDS_LIMIT);
        LENGTH_COST * length
            + DETOUR_COST * (length - DETOUR_LIMIT * self.span()).max(0.0)
            + BEND_COST * self.bends() as f64
            + EXTRA_BEND_COST * extra_bends as f64
            + SHORT_U_TURN_COST * self.short_u_turns() as f64
            + SHORT_JOG_COST * self.short_jogs() as f64
            + PORT_HOOK_COST * self.port_hooks(reach) as f64
            + STRAY_COST * self.stray_length(ends)
            + CLEARANCE_COST * self.crowded_length(others)
    }

    /// Steps made of two opposite bends with a leg shorter than [`SHORT_JOG_LEG`]: around a short middle leg the
    /// rail only moves over a lane or two (T-421); with a short first or last leg it turns again right at a port,
    /// and with two sharp bends around a middle leg shorter than [`ZIGZAG_LEG`] it zigzags (T-425). Each reads as
    /// an "S" next to a node, not as a Z between two.
    pub fn short_jogs(&self) -> usize {
        self.corners
            .windows(4)
            .filter(|quad| {
                let legs = [quad[0].to(quad[1]), quad[1].to(quad[2]), quad[2].to(quad[3])];
                let turn = |a: PlanVector, b: PlanVector| (a.x * b.y - a.y * b.x).signum();
                let opposite = turn(legs[0], legs[1]) == -turn(legs[1], legs[2]) && turn(legs[0], legs[1]) != 0.0;
                let short_end = |leg: &PlanVector| leg.length() < SHORT_END_LEG;
                let at_port = (quad[0] == self.corners[0] && short_end(&legs[0]))
                    || (quad[3] == self.corners[self.corners.len() - 1] && short_end(&legs[2]));
                // Both bends sharp: on screen the rail runs back and forth, a zigzag unless the middle leg is long.
                let zigzag = is_sharp_bend(legs[0], legs[1]) && legs[1].length() < ZIGZAG_LEG;
                opposite && (legs[1].length() < SHORT_JOG_LEG || at_port || zigzag)
            })
            .count()
    }

    /// Hooks at the ports (T-425): a first or last leg shorter than [`PORT_HOOK_LEG`] (with the stroke's `reach`
    /// past the first and the last corner point) next to a sharp bend, or one that the leg after the next runs back
    /// against (the rail turns round right at the port).
    pub fn port_hooks(&self, reach: [f64; 2]) -> usize {
        let legs: Vec<PlanVector> = self.corners.windows(2).map(|pair| pair[0].to(pair[1])).collect();
        if legs.len() < 2 {
            return 0;
        }
        let reversed: Vec<PlanVector> = legs.iter().rev().map(|leg| *leg * -1.0).collect();
        [(&legs, reach[0]), (&reversed, reach[1])]
            .iter()
            .filter(|(legs, reach)| {
                let (end, next) = (legs[0], legs[1]);
                let turns_back = legs.get(2).is_some_and(|after| after.dot(end) < 0.0);
                end.length() + reach < PORT_HOOK_LEG && (is_sharp_bend(end, next) || turns_back)
            })
            .count()
    }

    /// Length (tiles) of the corner path past its two end nodes `ends` (footprints) by more than
    /// [`STRAY_MARGIN`], along an axis on which the nodes stand apart: behind the start or beyond the target,
    /// where a U or a walk round a node goes (T-421). Along an axis on which the two footprints overlap (nodes
    /// in line) a rail may step aside freely: that is how it passes a node in the way.
    pub fn stray_length(&self, ends: [PlanRect; 2]) -> f64 {
        let around = between(ends);
        let rail = Self { corners: self.corners.clone(), points: self.corners.clone() };
        rail.length() - rail.length_inside(&[around])
    }

    /// Half turns made of two bends the same way around a leg shorter than [`SHORT_U_TURN_LEG`]: a hook that
    /// doubles back right after passing the node (T-406), never an open U.
    pub fn short_u_turns(&self) -> usize {
        self.corners
            .windows(4)
            .filter(|quad| {
                let legs = [quad[0].to(quad[1]), quad[1].to(quad[2]), quad[2].to(quad[3])];
                let turn = |a: PlanVector, b: PlanVector| (a.x * b.y - a.y * b.x).signum();
                let same_way = turn(legs[0], legs[1]) == turn(legs[1], legs[2]) && turn(legs[0], legs[1]) != 0.0;
                same_way && legs[1].length() < SHORT_U_TURN_LEG
            })
            .count()
    }

    /// True when the rail through `corners` takes more than [`BENDS_LIMIT`] bends or two more than the
    /// fewest its end nodes allow ([`fewest_bends_between`]), is longer than
    /// [`DETOUR_LIMIT`] times the Manhattan distance between its ends (T-407), steps over just before a port
    /// or strays past the box around its two end nodes `ends` (T-421).
    pub fn reads_as_detour(corners: &[PlanPoint], ends: [PlanRect; 2]) -> bool {
        let corners = without_collinear(corners);
        let rail = Self { points: corners.clone(), corners };
        rail.bends() > BENDS_LIMIT
            || rail.bends() > fewest_bends_between(ends) + 1
            || rail.length() > DETOUR_LIMIT * rail.span() + DETOUR_SLACK
            || rail.short_jogs() > 0
            || rail.port_hooks([0.0; 2]) > 0
            || rail.stray_length(ends) > STRAY_SLACK
    }

    /// Length (tiles) along the corners.
    pub fn length(&self) -> f64 {
        self.corners.windows(2).map(|pair| pair[0].distance(pair[1])).sum()
    }

    /// Manhattan distance (tiles) between the two ends.
    pub fn span(&self) -> f64 {
        match (self.corners.first(), self.corners.last()) {
            (Some(first), Some(last)) => (last.x - first.x).abs() + (last.y - first.y).abs(),
            _ => 0.0,
        }
    }

    /// Number of bends.
    pub fn bends(&self) -> usize {
        self.corners.len().saturating_sub(2)
    }

    /// Length (tiles) of the centerline closer than [`RAIL_CLEARANCE`] to one of `others`.
    fn crowded_length(&self, others: &[PlanRect]) -> f64 {
        let zones: Vec<PlanRect> = others.iter().map(|rect| rect.inflate(RAIL_CLEARANCE)).collect();
        self.length_inside(&zones)
    }

    /// Length (tiles) of the centerline inside one of `zones`.
    pub fn length_inside(&self, zones: &[PlanRect]) -> f64 {
        let mut crowded = 0.0;
        for pair in self.points.windows(2) {
            let length = pair[0].distance(pair[1]);
            let samples = (length * CLEARANCE_SAMPLES_PER_TILE).ceil().max(1.0);
            let step = length / samples;
            for index in 0..samples as usize {
                let point = pair[0] + pair[0].to(pair[1]) * ((index as f64 + 0.5) / samples);
                if zones.iter().any(|zone| zone.contains(point)) {
                    crowded += step;
                }
            }
        }
        crowded
    }
}

/// True when the legs `a` and `b` (plan, rotation applied) run back against each other on screen: a bend of
/// 120° on screen between `+x` and `+y` (or `−x` and `−y`), against 60° between `+x` and `−y`.
fn is_sharp_bend(a: PlanVector, b: PlanVector) -> bool {
    let screen = |vector: PlanVector| {
        let unit = vector.normalized();
        ((unit.x - unit.y) * COS_30, (unit.x + unit.y) * SIN_30)
    };
    let (a, b) = (screen(a), screen(b));
    a.0 * b.0 + a.1 * b.1 < 0.0
}

/// Fewest bends a rail between two nodes standing on `ends` could take with free faces: none when their
/// footprints overlap along an axis (they face each other), one (an L) otherwise.
pub fn fewest_bends_between(ends: [PlanRect; 2]) -> usize {
    let [a, b] = ends;
    let facing = a.x1.min(b.x1) > a.x0.max(b.x0) || a.y1.min(b.y1) > a.y0.max(b.y0);
    usize::from(!facing)
}

/// The floor a rail between the nodes standing on `ends` keeps to: the box around both footprints grown by
/// [`STRAY_MARGIN`] along each axis on which they stand apart, unbounded along an axis on which they stand in
/// line (their footprints overlap or nearly meet, [`IN_LINE_GAP`]).
fn between(ends: [PlanRect; 2]) -> PlanRect {
    let [a, b] = ends;
    let span = |low_a: f64, high_a: f64, low_b: f64, high_b: f64| {
        let in_line = low_a.max(low_b) - high_a.min(high_b) < IN_LINE_GAP;
        if in_line {
            (f64::NEG_INFINITY, f64::INFINITY)
        } else {
            (low_a.min(low_b) - STRAY_MARGIN, high_a.max(high_b) + STRAY_MARGIN)
        }
    };
    let (x0, x1) = span(a.x0, a.x1, b.x0, b.x1);
    let (y0, y1) = span(a.y0, a.y1, b.y0, b.y1);
    PlanRect { x0, y0, x1, y1 }
}

/// Drops repeated points and the middle of straight runs (both ends kept).
fn without_collinear(points: &[PlanPoint]) -> Vec<PlanPoint> {
    const SAME: f64 = 1e-6;
    let mut result: Vec<PlanPoint> = Vec::with_capacity(points.len());
    for &point in points {
        if result.last().is_some_and(|last| last.distance(point) <= SAME) {
            continue;
        }
        while let [.., before, corner] = result[..] {
            let (incoming, outgoing) = (before.to(corner).normalized(), corner.to(point).normalized());
            if incoming.dot(outgoing) < COLLINEAR_COSINE {
                break;
            }
            result.pop();
        }
        result.push(point);
    }
    result
}

/// Centerline with every inner corner replaced by a rounded bend of `radius` (less when the
/// legs are short: a bend never takes more than half of a leg, so consecutive bends never overlap).
fn rounded(corners: &[PlanPoint], radius: f64) -> Vec<PlanPoint> {
    let Some(&first) = corners.first() else { return Vec::new() };
    let mut points = vec![first];
    for triple in corners.windows(3) {
        let [before, corner, after] = [triple[0], triple[1], triple[2]];
        let (incoming, outgoing) = (before.to(corner), corner.to(after));
        let turn = incoming.normalized().dot(outgoing.normalized()).clamp(-1.0, 1.0).acos();
        let wanted = radius * (turn / 2.0).tan();
        let reach = wanted.min(incoming.length() / 2.0).min(outgoing.length() / 2.0);
        if reach <= f64::EPSILON {
            points.push(corner);
            continue;
        }
        let enter = corner - incoming.normalized() * reach;
        let leave = corner + outgoing.normalized() * reach;
        // A quadratic Bézier tangent to both legs: indistinguishable from the arc at this size.
        for index in 0..=BEND_SAMPLES {
            let t = index as f64 / BEND_SAMPLES as f64;
            let a = enter + enter.to(corner) * t;
            let b = corner + corner.to(leave) * t;
            points.push(a + a.to(b) * t);
        }
    }
    if let Some(&last) = corners.last()
        && corners.len() > 1
    {
        points.push(last);
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bends_are_rounded_and_stay_near_the_corners() {
        let corners = vec![PlanPoint::new(0.5, 0.0), PlanPoint::new(4.0, 0.0), PlanPoint::new(4.0, 3.5)];
        let rail = RailGeometry::from_corners(corners);
        assert_eq!(rail.bends(), 1);
        assert!(rail.points.len() >= rail.corners.len() + BEND_SAMPLES, "{}", rail.points.len());
        let corner = PlanPoint::new(4.0, 0.0);
        let nearest = rail.points.iter().map(|point| point.distance(corner)).fold(f64::INFINITY, f64::min);
        assert!(nearest > 0.05 && nearest < RAIL_BEND_RADIUS, "{nearest}");
    }

    #[test]
    fn fewer_bends_cost_less_and_crowding_costs_more() {
        let straight = RailGeometry::from_corners(vec![PlanPoint::new(0.0, 0.0), PlanPoint::new(5.0, 0.0)]);
        let zigzag = RailGeometry::from_corners(vec![
            PlanPoint::new(0.0, 0.0),
            PlanPoint::new(2.0, 0.0),
            PlanPoint::new(2.0, 0.5),
            PlanPoint::new(5.0, 0.5),
        ]);
        let ends = [
            PlanRect::around(PlanPoint::new(-0.5, 0.0), 0.5, 0.5),
            PlanRect::around(PlanPoint::new(5.5, 0.5), 0.5, 0.5),
        ];
        assert!(straight.cost(&[], ends, [0.0; 2]) < zigzag.cost(&[], ends, [0.0; 2]));
        let blocker = PlanRect::around(PlanPoint::new(2.5, 0.2), 0.3, 0.1);
        assert!(straight.cost(&[blocker], ends, [0.0; 2]) > straight.cost(&[], ends, [0.0; 2]) + CLEARANCE_COST * 0.5);
    }

    #[test]
    fn hooks_are_short_u_turns_and_open_us_are_not() {
        let hook = RailGeometry::from_corners(vec![
            PlanPoint::new(0.0, 0.0),
            PlanPoint::new(3.0, 0.0),
            PlanPoint::new(3.0, 0.8),
            PlanPoint::new(2.4, 0.8),
        ]);
        assert_eq!(hook.short_u_turns(), 1);
        let open = RailGeometry::from_corners(vec![
            PlanPoint::new(0.0, 0.0),
            PlanPoint::new(0.0, 1.0),
            PlanPoint::new(3.0, 1.0),
            PlanPoint::new(3.0, 0.4),
        ]);
        assert_eq!(open.short_u_turns(), 0);
        let zigzag = RailGeometry::from_corners(vec![
            PlanPoint::new(0.0, 0.0),
            PlanPoint::new(2.0, 0.0),
            PlanPoint::new(2.0, 0.5),
            PlanPoint::new(4.0, 0.5),
        ]);
        assert_eq!(zigzag.short_u_turns(), 0, "a Z is not a half turn");
    }

    #[test]
    fn a_u_past_a_neighbour_strays_and_costs_more_than_the_l() {
        // A node right below and to the side of the start (T-421: `teller_sw -> ws2`): the L stays between the
        // two nodes; the U runs a tile past the neighbour and comes back.
        let start = PlanRect::around(PlanPoint::new(0.0, 0.0), 0.4, 0.4);
        let neighbour = PlanRect::around(PlanPoint::new(2.0, 2.0), 0.4, 0.4);
        let ends = [start, neighbour];
        let l = RailGeometry::from_corners(vec![
            PlanPoint::new(0.0, 0.4),
            PlanPoint::new(0.0, 2.0),
            PlanPoint::new(1.6, 2.0),
        ]);
        let u = RailGeometry::from_corners(vec![
            PlanPoint::new(0.0, 0.4),
            PlanPoint::new(0.0, 3.4),
            PlanPoint::new(2.0, 3.4),
            PlanPoint::new(2.0, 2.4),
        ]);
        assert!(l.stray_length(ends) < 1e-9, "{}", l.stray_length(ends));
        assert!(u.stray_length(ends) > 0.8, "{}", u.stray_length(ends));
        assert!(u.cost(&[], ends, [0.0; 2]) > l.cost(&[], ends, [0.0; 2]) + STRAY_COST * 0.8);
        assert!(RailGeometry::reads_as_detour(&u.corners, ends));
        assert!(!RailGeometry::reads_as_detour(&l.corners, ends));
        assert_eq!(fewest_bends_between(ends), 1);
    }

    #[test]
    fn stepping_aside_past_a_node_in_line_is_no_stray() {
        // Two nodes in line with a third between them: the rail steps a lane aside to pass it.
        let ends = [
            PlanRect::around(PlanPoint::new(0.0, 0.0), 0.4, 0.4),
            PlanRect::around(PlanPoint::new(0.0, 6.0), 0.4, 0.4),
        ];
        let around = RailGeometry::from_corners(vec![
            PlanPoint::new(0.4, 0.0),
            PlanPoint::new(1.2, 0.0),
            PlanPoint::new(1.2, 6.0),
            PlanPoint::new(0.4, 6.0),
        ]);
        assert!(around.stray_length(ends) < 1e-9, "{}", around.stray_length(ends));
        assert_eq!(fewest_bends_between(ends), 0);
    }

    #[test]
    fn a_step_just_before_the_port_is_a_short_jog() {
        let step = RailGeometry::from_corners(vec![
            PlanPoint::new(0.0, 0.0),
            PlanPoint::new(0.0, 4.0),
            PlanPoint::new(0.5, 4.0),
            PlanPoint::new(0.5, 4.6),
        ]);
        assert_eq!(step.short_jogs(), 1);
        assert_eq!(step.short_u_turns(), 0);
        let wide = RailGeometry::from_corners(vec![
            PlanPoint::new(0.0, 0.0),
            PlanPoint::new(0.0, 2.0),
            PlanPoint::new(3.0, 2.0),
            PlanPoint::new(3.0, 4.0),
        ]);
        assert_eq!(wide.short_jogs(), 0, "a Z with a long middle leg is a plain elbow pair");
    }

    #[test]
    fn a_short_last_leg_after_a_sharp_bend_is_a_hook_at_the_port() {
        // Up-right on screen (−y), then a short leg up-left (−x): the T-424 `web -> internet` into the cloud.
        let hook = RailGeometry::from_corners(vec![
            PlanPoint::new(4.4, 8.0),
            PlanPoint::new(4.4, 4.8),
            PlanPoint::new(3.6, 4.8),
        ]);
        assert_eq!(hook.port_hooks([0.0; 2]), 1);
        assert_eq!(hook.port_hooks([0.0, 1.0]), 0, "a head's reach past the base makes the leg long enough");
        let gentle = RailGeometry::from_corners(vec![
            PlanPoint::new(4.4, 8.0),
            PlanPoint::new(4.4, 4.8),
            PlanPoint::new(5.2, 4.8),
        ]);
        assert_eq!(gentle.port_hooks([0.0; 2]), 0, "a 60° bend on screen is no hook");
    }

    #[test]
    fn a_z_with_two_sharp_bends_around_a_short_middle_zigzags() {
        // +y, +x, +y: down-left, down-right, down-left on screen.
        let zigzag = RailGeometry::from_corners(vec![
            PlanPoint::new(0.0, 0.0),
            PlanPoint::new(0.0, 4.0),
            PlanPoint::new(2.0, 4.0),
            PlanPoint::new(2.0, 6.0),
        ]);
        assert_eq!(zigzag.short_jogs(), 1);
        // +y, −x, +y with the same legs: a stair step.
        let stair = RailGeometry::from_corners(vec![
            PlanPoint::new(0.0, 0.0),
            PlanPoint::new(0.0, 4.0),
            PlanPoint::new(-2.0, 4.0),
            PlanPoint::new(-2.0, 6.0),
        ]);
        assert_eq!(stair.short_jogs(), 0);
        let at_port = RailGeometry::from_corners(vec![
            PlanPoint::new(0.0, 0.0),
            PlanPoint::new(0.0, 4.0),
            PlanPoint::new(-2.0, 4.0),
            PlanPoint::new(-2.0, 4.5),
        ]);
        assert_eq!(at_port.short_jogs(), 1, "a stair turning again right at the port");
    }

    #[test]
    fn collinear_corners_are_merged() {
        let points =
            [PlanPoint::new(0.0, 0.0), PlanPoint::new(1.0, 0.0), PlanPoint::new(2.0, 0.0), PlanPoint::new(2.0, 1.0)];
        assert_eq!(without_collinear(&points).len(), 3);
    }

    #[test]
    fn ends_sharing_a_face_get_ordered_lanes() {
        let footprint = Footprint::Rect { half_width: 0.5, half_depth: 0.5 };
        let ends = [
            FaceEnd { angle: 0.0, toward: 0.4 },
            FaceEnd { angle: 0.0, toward: -0.3 },
            FaceEnd { angle: 1.6, toward: 1.6 },
        ];
        let angles = share_faces(&footprint, &ends);
        assert!(angles[0] > angles[1], "{angles:?}");
        assert_eq!(angles[2], 1.6);
        assert!(angles[0].abs() < 0.6 && angles[1].abs() < 0.6);
    }
}
