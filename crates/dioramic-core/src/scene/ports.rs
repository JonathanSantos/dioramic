//! Arrow ports on node footprints (`docs/06-renderizacao.md#setas-e-portas`, ADR-012).
//!
//! Ported from the prototype's `exitAt` / `inPort`, generalized with an angular parametrization of the
//! footprint border so several arrivals at the same node can be spread apart:
//!
//! - a port is the border point in direction `φ` (plan angle, `atan2(y, x)`) from the node center;
//! - on a **tall** node the arrival must be on a visible face (`+x` or `+y`), i.e. `φ` inside the visible
//!   arc that goes from the right corner `(+w, −d)` through the front corner to the left corner `(−w, +d)`;
//!   a natural border behind the node slides to the nearest side corner (the prototype's rule);
//! - the head always aims at the node center: `aim = center − port`.

use std::f64::consts::{PI, TAU};

use crate::geometry::{PlanPoint, PlanVector};
use crate::icons::Footprint;

use super::{ARROW_HALF_WIDTH, ARROW_LENGTH};

/// Gap (tiles) between the footprint border and the arrow tip, so the tip never touches the body outline.
pub const PORT_GAP: f64 = 0.08;
/// Minimum distance (tiles, along the border) between two arrival ports on the same node.
const MIN_PORT_SPACING: f64 = 0.34;
/// Numerical slack for "on the back face" tests.
const BACK_EPSILON: f64 = 1e-6;

/// Range of allowed arrival angles (radians, `low < high`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrivalArc {
    /// Lowest allowed angle.
    pub low: f64,
    /// Highest allowed angle.
    pub high: f64,
}

impl ArrivalArc {
    /// The whole border (low nodes accept any edge).
    pub const FULL: ArrivalArc = ArrivalArc { low: -PI, high: PI };

    /// Arc of the visible faces of a tall footprint: right corner → front → left corner.
    pub fn visible(footprint: &Footprint) -> Self {
        let (half_width, half_depth) = footprint.half_extents();
        Self { low: (-half_depth).atan2(half_width), high: half_depth.atan2(-half_width) }
    }

    /// Allowed arc for a node.
    pub fn for_node(footprint: &Footprint, tall: bool) -> Self {
        if tall { Self::visible(footprint) } else { Self::FULL }
    }

    /// Length of the arc (radians).
    pub fn span(&self) -> f64 {
        self.high - self.low
    }

    /// Moves `angle` into the arc: angles behind the node slide to the nearest end (side corner).
    pub fn clamp(&self, angle: f64) -> f64 {
        let wrapped = self.low + (angle - self.low).rem_euclid(TAU);
        if wrapped <= self.high {
            return wrapped;
        }
        let past_high = wrapped - self.high;
        let before_low = self.low + TAU - wrapped;
        if past_high <= before_low { self.high } else { self.low }
    }

    /// How far behind the arc `angle` lies, signed so that sorting by `(clamp, overflow)` keeps the fan order
    /// of ends clamped to the same corner (T-400): past [`ArrivalArc::high`] it is positive (the farther behind,
    /// the closer to the corner), before [`ArrivalArc::low`] negative (the farther behind, the closer to `low`);
    /// 0 inside the arc.
    pub fn overflow(&self, angle: f64) -> f64 {
        let wrapped = self.low + (angle - self.low).rem_euclid(TAU);
        if wrapped <= self.high {
            return 0.0;
        }
        let past_high = wrapped - self.high;
        let before_low = self.low + TAU - wrapped;
        if past_high <= before_low { past_high } else { -before_low }
    }

    /// True when `angle` (any turn) lies inside the arc.
    pub fn contains(&self, angle: f64) -> bool {
        self.low + (angle - self.low).rem_euclid(TAU) <= self.high + BACK_EPSILON
    }
}

/// Unit plan vector at angle `angle`.
pub fn direction_at(angle: f64) -> PlanVector {
    PlanVector::new(angle.cos(), angle.sin())
}

/// Plan angle of a vector.
pub fn angle_of(vector: PlanVector) -> f64 {
    vector.y.atan2(vector.x)
}

/// Border point of the footprint in `direction` (unit) from `center`, and whether it is on a back face
/// (`−x` or `−y`, hidden behind the body). Prototype `exitAt`.
pub fn border_point(center: PlanPoint, footprint: &Footprint, direction: PlanVector) -> (PlanPoint, bool) {
    match *footprint {
        Footprint::Circle { radius } => (center + direction * radius, direction.x + direction.y < -BACK_EPSILON),
        Footprint::Rect { half_width, half_depth } => {
            let along_x = half_width / direction.x.abs().max(f64::EPSILON);
            let along_y = half_depth / direction.y.abs().max(f64::EPSILON);
            let distance = along_x.min(along_y);
            let on_x_face = direction.x.abs() * half_depth >= direction.y.abs() * half_width;
            let back = if on_x_face { direction.x < -BACK_EPSILON } else { direction.y < -BACK_EPSILON };
            (center + direction * distance, back)
        }
    }
}

/// Outward normal of the footprint face crossed in `direction` from the center: a plan axis. Rails leave
/// a port square to its face (T-403); a circle uses the axis nearest to `direction`.
pub fn face_normal(footprint: &Footprint, direction: PlanVector) -> PlanVector {
    let on_x_face = match *footprint {
        Footprint::Circle { .. } => direction.x.abs() >= direction.y.abs(),
        Footprint::Rect { half_width, half_depth } => direction.x.abs() * half_depth >= direction.y.abs() * half_width,
    };
    if on_x_face { PlanVector::new(direction.x.signum(), 0.0) } else { PlanVector::new(0.0, direction.y.signum()) }
}

/// An arrival port: where the arrow tip is and where it aims.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrivalPort {
    /// Arrow tip (border point pushed out by [`PORT_GAP`]).
    pub tip: PlanPoint,
    /// Unit direction from the tip to the node center.
    pub aim: PlanVector,
}

impl ArrivalPort {
    /// Port at border angle `angle` of a node.
    pub fn at_angle(center: PlanPoint, footprint: &Footprint, angle: f64) -> Self {
        let (border, _) = border_point(center, footprint, direction_at(angle));
        let aim = border.to(center).normalized();
        let aim = if aim.length() > 0.0 { aim } else { direction_at(angle) * -1.0 };
        Self { tip: border - aim * PORT_GAP, aim }
    }

    /// Port at border angle `angle` whose head points square into the face it sits on (rails, T-406): the
    /// last leg of an elbow runs along a plan axis, so the head does too.
    pub fn square_to_face(center: PlanPoint, footprint: &Footprint, angle: f64) -> Self {
        let direction = direction_at(angle);
        let normal = face_normal(footprint, direction);
        let border = match *footprint {
            // A round body: the head stands on the tangent line square to the axis.
            Footprint::Circle { radius } => {
                let tangent = PlanVector::new(-normal.y, normal.x);
                center + normal * radius + tangent * (direction.dot(tangent) * radius)
            }
            Footprint::Rect { .. } => border_point(center, footprint, direction).0,
        };
        Self { tip: border + normal * PORT_GAP, aim: normal * -1.0 }
    }

    /// Base midpoint of the head (where the stroke ends).
    pub fn base(&self) -> PlanPoint {
        self.tip - self.aim * ARROW_LENGTH
    }

    /// Head triangle on the floor: tip, base corners.
    pub fn triangle(&self) -> [PlanPoint; 3] {
        let base = self.base();
        let side = self.aim.perpendicular() * ARROW_HALF_WIDTH;
        [self.tip, base + side, base - side]
    }
}

/// Mean radius of a footprint (tiles), to turn border distances into angles.
fn mean_radius(footprint: &Footprint) -> f64 {
    let (half_width, half_depth) = footprint.half_extents();
    ((half_width + half_depth) / 2.0).max(0.05)
}

/// Assigns border angles to the arrivals at one node: each desired angle is clamped into the allowed arc,
/// then neighbors closer than [`MIN_PORT_SPACING`] are pushed apart symmetrically (pool-adjacent-violators),
/// staying inside the arc. Returns one angle per request, in request order. Deterministic.
pub fn spread_arrivals(footprint: &Footprint, arc: ArrivalArc, desired: &[f64]) -> Vec<f64> {
    let count = desired.len();
    if count == 0 {
        return Vec::new();
    }
    let clamped: Vec<f64> = desired.iter().map(|&angle| arc.clamp(angle)).collect();
    let overflow: Vec<f64> = desired.iter().map(|&angle| arc.overflow(angle)).collect();
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by(|&a, &b| clamped[a].total_cmp(&clamped[b]).then(overflow[a].total_cmp(&overflow[b])).then(a.cmp(&b)));
    let wanted_gap = MIN_PORT_SPACING / mean_radius(footprint);
    let gap = if count > 1 { wanted_gap.min(arc.span() / (count - 1) as f64) } else { 0.0 };
    let sorted: Vec<f64> = order.iter().map(|&index| clamped[index]).collect();
    let placed = spread_sorted(&sorted, gap, arc);
    let mut result = vec![0.0; count];
    for (rank, &index) in order.iter().enumerate() {
        result[index] = placed[rank];
    }
    result
}

/// Positions `x_i ≥ x_{i−1} + gap` closest (least squares) to the sorted `values`, clamped into the arc.
///
/// With `b_i = value_i − i·gap` the constraint becomes "`y` non-decreasing", i.e. isotonic regression,
/// solved by pool-adjacent-violators; then `x_i = y_i + i·gap`.
fn spread_sorted(values: &[f64], gap: f64, arc: ArrivalArc) -> Vec<f64> {
    let mut blocks: Vec<(f64, usize)> = Vec::new();
    for (index, value) in values.iter().enumerate() {
        blocks.push((value - index as f64 * gap, 1));
        while let [.., (previous_sum, previous_len), (last_sum, last_len)] = blocks[..] {
            if previous_sum / previous_len as f64 <= last_sum / last_len as f64 {
                break;
            }
            blocks.truncate(blocks.len() - 2);
            blocks.push((previous_sum + last_sum, previous_len + last_len));
        }
    }
    let mut positions = Vec::with_capacity(values.len());
    for (sum, len) in blocks {
        let mean = sum / len as f64;
        for _ in 0..len {
            positions.push(mean + positions.len() as f64 * gap);
        }
    }
    clamp_into_arc(&mut positions, gap, arc);
    positions
}

/// Shifts positions back inside the arc while keeping the gap (forward then backward pass).
fn clamp_into_arc(positions: &mut [f64], gap: f64, arc: ArrivalArc) {
    let count = positions.len();
    for index in 0..count {
        let lowest = if index == 0 { arc.low } else { positions[index - 1] + gap };
        positions[index] = positions[index].max(lowest);
    }
    for index in (0..count).rev() {
        let highest = if index + 1 == count { arc.high } else { positions[index + 1] - gap };
        positions[index] = positions[index].min(highest);
    }
}

/// Do two floor triangles overlap? (separating axis test, used to keep heads apart).
pub fn triangles_overlap(a: &[PlanPoint; 3], b: &[PlanPoint; 3]) -> bool {
    let separated_by = |shape: &[PlanPoint; 3], other: &[PlanPoint; 3]| {
        (0..3).any(|index| {
            let edge = shape[index].to(shape[(index + 1) % 3]);
            let normal = edge.perpendicular();
            let project = |point: &PlanPoint| normal.dot(PlanVector::new(point.x, point.y));
            let (min_a, max_a) = min_max(shape.iter().map(project));
            let (min_b, max_b) = min_max(other.iter().map(project));
            max_a < min_b || max_b < min_a
        })
    };
    !(separated_by(a, b) || separated_by(b, a))
}

fn min_max(values: impl Iterator<Item = f64>) -> (f64, f64) {
    values.fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), value| (low.min(value), high.max(value)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQUARE: Footprint = Footprint::Rect { half_width: 0.42, half_depth: 0.42 };
    const WIDE: Footprint = Footprint::Rect { half_width: 0.55, half_depth: 0.4 };
    const ROUND: Footprint = Footprint::Circle { radius: 0.48 };
    const CENTER: PlanPoint = PlanPoint::new(5.0, 2.0);

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn rect_border_and_back_faces_match_the_prototype() {
        let (point, back) = border_point(CENTER, &WIDE, PlanVector::new(1.0, 0.0));
        assert!(close(point.x, 5.55) && close(point.y, 2.0) && !back);
        let (point, back) = border_point(CENTER, &WIDE, PlanVector::new(0.0, -1.0));
        assert!(close(point.y, 1.6) && back);
        let (_, back) = border_point(CENTER, &WIDE, PlanVector::new(-1.0, 0.2).normalized());
        assert!(back);
        let (point, back) = border_point(CENTER, &SQUARE, PlanVector::new(1.0, 1.0).normalized());
        assert!(close(point.x, 5.42) && close(point.y, 2.42) && !back);
    }

    #[test]
    fn circle_border_and_back_faces() {
        let (point, back) = border_point(CENTER, &ROUND, PlanVector::new(0.0, 1.0));
        assert!(close(point.y, 2.48) && !back);
        let (_, back) = border_point(CENTER, &ROUND, PlanVector::new(-0.6, -0.8));
        assert!(back);
    }

    #[test]
    fn visible_arc_spans_right_corner_to_left_corner() {
        let arc = ArrivalArc::visible(&SQUARE);
        assert!(close(arc.low, -PI / 4.0) && close(arc.high, 3.0 * PI / 4.0));
        assert!(arc.contains(0.0) && arc.contains(PI / 2.0) && !arc.contains(PI));
        // From behind, slightly to the screen left (x − y < 0) → left corner; to the right → right corner.
        assert!(close(arc.clamp(angle_of(PlanVector::new(-1.0, -0.8))), arc.high));
        assert!(close(arc.clamp(angle_of(PlanVector::new(-0.8, -1.0))), arc.low));
        assert!(close(arc.clamp(0.3), 0.3));
    }

    #[test]
    fn tall_node_port_slides_to_the_visible_corner_and_aims_at_center() {
        let arc = ArrivalArc::visible(&SQUARE);
        let angle = arc.clamp(angle_of(PlanVector::new(-1.0, -0.3)));
        let port = ArrivalPort::at_angle(CENTER, &SQUARE, angle);
        let corner = PlanPoint::new(CENTER.x - 0.42, CENTER.y + 0.42);
        assert!(port.tip.distance(corner) < PORT_GAP + 1e-9);
        let to_center = port.tip.to(CENTER).normalized();
        assert!(close(to_center.x, port.aim.x) && close(to_center.y, port.aim.y));
        let circle_port = ArrivalPort::at_angle(CENTER, &ROUND, ArrivalArc::visible(&ROUND).high);
        assert!(close(circle_port.tip.distance(CENTER), 0.48 + PORT_GAP));
    }

    #[test]
    fn ends_clamped_to_the_same_corner_keep_the_fan_order() {
        let arc = ArrivalArc::visible(&SQUARE);
        // Both come from behind the left corner (`high`): the one farther behind takes the corner itself.
        let angles = spread_arrivals(&SQUARE, arc, &[arc.high + 1.0, arc.high + 0.2]);
        assert!(close(angles[0], arc.high) && angles[1] < angles[0]);
        // Behind the right corner (`low`): the one farther behind takes `low`.
        let angles = spread_arrivals(&SQUARE, arc, &[arc.low - 0.2, arc.low - 1.0]);
        assert!(close(angles[1], arc.low) && angles[0] > angles[1]);
        assert_eq!(arc.overflow(0.3), 0.0);
    }

    #[test]
    fn face_normals_are_plan_axes() {
        assert_eq!(face_normal(&WIDE, PlanVector::new(1.0, 0.3)), PlanVector::new(1.0, 0.0));
        assert_eq!(face_normal(&WIDE, PlanVector::new(0.2, -1.0)), PlanVector::new(0.0, -1.0));
        assert_eq!(face_normal(&ROUND, PlanVector::new(-0.8, 0.5)), PlanVector::new(-1.0, 0.0));
    }

    #[test]
    fn low_nodes_accept_the_back_border() {
        let angle = ArrivalArc::FULL.clamp(PI * 0.9);
        assert!(close(angle, PI * 0.9));
    }

    #[test]
    fn arrivals_from_the_same_side_are_spread_symmetrically() {
        let angles = spread_arrivals(&SQUARE, ArrivalArc::FULL, &[0.2, 0.2, -2.0]);
        let gap = MIN_PORT_SPACING / 0.42;
        assert!(close(angles[1] - angles[0], gap) || close(angles[0] - angles[1], gap));
        assert!(close((angles[0] + angles[1]) / 2.0, 0.2));
        assert!(close(angles[2], -2.0));
    }

    #[test]
    fn spreading_stays_inside_the_visible_arc() {
        let arc = ArrivalArc::visible(&SQUARE);
        let angles = spread_arrivals(&SQUARE, arc, &[PI, PI, PI, PI]);
        assert!(angles.iter().all(|&angle| angle >= arc.low - 1e-9 && angle <= arc.high + 1e-9));
        let mut sorted = angles.clone();
        sorted.sort_by(f64::total_cmp);
        assert!(sorted.windows(2).all(|pair| pair[1] - pair[0] > 0.3));
    }

    #[test]
    fn spread_heads_do_not_overlap() {
        let arc = ArrivalArc::visible(&SQUARE);
        let angles = spread_arrivals(&SQUARE, arc, &[0.0, 0.0, 0.05]);
        let heads: Vec<[PlanPoint; 3]> =
            angles.iter().map(|&angle| ArrivalPort::at_angle(CENTER, &SQUARE, angle).triangle()).collect();
        for (index, head) in heads.iter().enumerate() {
            for other in &heads[index + 1..] {
                assert!(!triangles_overlap(head, other));
            }
        }
        assert!(triangles_overlap(&heads[0], &heads[0]));
    }
}
