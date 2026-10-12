//! Arrow head measurements (`docs/06-renderizacao.md#setas-e-portas`), shared by
//! [`crate::assert_arrows_aim_at_targets`] and the `cargo xtask gallery` report.
//!
//! Every measurement is recomputed from the public scene geometry (tips, bases, paths, node footprints),
//! so a regression in the scene builder cannot hide behind the builder's own bookkeeping.

use dioramic_core::geometry::{PlanVector, Projection, ScreenPoint, ScreenVector};
use dioramic_core::icons::Footprint;
use dioramic_core::scene::{ArrowHead, EdgePath, Scene, SceneEdge, SceneNode};

/// How deep (px) a tip must sit inside a node silhouette to count as hidden. Visible ports put the tip
/// slightly outside the footprint edge, and a `rails` head arriving at a face hidden behind its node stands
/// on the silhouette border, where the rail comes out of it (T-407); this slack absorbs
/// anti-aliasing-sized rounding only.
pub const HIDDEN_TIP_TOLERANCE_PX: f64 = 1.0;

/// Polygon resolution used to approximate circular footprints.
const CIRCLE_SILHOUETTE_SAMPLES: usize = 48;

/// Which end of the edge an arrow head sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadEnd {
    /// At `to` (the usual head).
    Target,
    /// At `from` (`arrows: both` / `start`).
    Source,
}

/// Angle errors and visibility of one arrow head.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrowMeasurement {
    /// Edge id.
    pub edge: String,
    /// End of the edge where the head sits.
    pub end: HeadEnd,
    /// Id of the node the head points at.
    pub target: String,
    /// Angle (degrees) between the head direction and `target_center − tip`.
    pub center_error_deg: f64,
    /// Angle (degrees) between the head direction and the direction the stroke arrives in.
    pub tangent_error_deg: f64,
    /// Node whose body (painted after edges) covers the tip, if any.
    pub hidden_by: Option<String>,
}

impl ArrowMeasurement {
    /// The larger of the two angle errors.
    pub fn max_error_deg(&self) -> f64 {
        self.center_error_deg.max(self.tangent_error_deg)
    }

    /// `true` when both angles are within `tolerance_deg` and the tip is visible.
    pub fn is_ok(&self, tolerance_deg: f64) -> bool {
        self.max_error_deg() <= tolerance_deg && self.hidden_by.is_none()
    }
}

/// Measures every arrow head of the scene, in edge order.
pub fn measure_arrows(scene: &Scene) -> Vec<ArrowMeasurement> {
    let silhouettes: Vec<NodeSilhouette> =
        scene.nodes.iter().map(|node| NodeSilhouette::of(node, scene.tile_px)).collect();
    scene
        .edges
        .iter()
        .flat_map(|edge| edge.heads.iter().map(move |head| (edge, head)))
        .map(|(edge, head)| measure_head(edge, head, &silhouettes))
        .collect()
}

fn measure_head(edge: &SceneEdge, head: &ArrowHead, silhouettes: &[NodeSilhouette]) -> ArrowMeasurement {
    let end = head_end(&edge.path, head);
    let target = match end {
        HeadEnd::Target => edge.to.as_str(),
        HeadEnd::Source => edge.from.as_str(),
    };
    let direction = head.direction();
    let arrival = arrival_direction(&edge.path, end);
    ArrowMeasurement {
        edge: edge.id.to_string(),
        end,
        target: target.to_owned(),
        center_error_deg: angle_between_degrees(direction, head.target_center - head.tip),
        tangent_error_deg: angle_between_degrees(direction, arrival),
        hidden_by: hiding_node(head.tip, target, !edge.midway.is_empty(), silhouettes),
    }
}

/// The head belongs to the path end its base is closest to (the stroke stops at the base).
fn head_end(path: &EdgePath, head: &ArrowHead) -> HeadEnd {
    let base_middle = head.base[0].midpoint(head.base[1]);
    let (start, end) = path_endpoints(path);
    if base_middle.distance(end) <= base_middle.distance(start) { HeadEnd::Target } else { HeadEnd::Source }
}

fn path_endpoints(path: &EdgePath) -> (ScreenPoint, ScreenPoint) {
    match path {
        EdgePath::Curve(curve) => (curve.points[0], curve.points[3]),
        EdgePath::Polyline(points) => {
            let first = points.first().copied().unwrap_or_default();
            (first, points.last().copied().unwrap_or(first))
        }
    }
}

/// Direction in which the stroke arrives at `end` (for the source end: travelling backwards).
///
/// Computed here rather than with `EdgePath::end_direction` so the check also covers the start of
/// bidirectional edges and does not depend on the scene builder's own helper.
fn arrival_direction(path: &EdgePath, end: HeadEnd) -> ScreenVector {
    match (path, end) {
        (EdgePath::Curve(curve), HeadEnd::Target) => curve.end_direction(),
        (EdgePath::Curve(curve), HeadEnd::Source) => {
            let [start, control1, control2, finish] = curve.points;
            first_non_zero([start - control1, start - control2, start - finish])
        }
        (EdgePath::Polyline(points), HeadEnd::Target) => last_segment_direction(points.iter().rev()),
        (EdgePath::Polyline(points), HeadEnd::Source) => last_segment_direction(points.iter()),
    }
}

/// Direction of the first non-degenerate segment, pointing towards the first point of `points`.
fn last_segment_direction<'a>(mut points: impl Iterator<Item = &'a ScreenPoint>) -> ScreenVector {
    let Some(&anchor) = points.next() else { return ScreenVector::default() };
    first_non_zero(points.map(|&point| anchor - point))
}

fn first_non_zero(vectors: impl IntoIterator<Item = ScreenVector>) -> ScreenVector {
    vectors.into_iter().find(|vector| vector.length() > f64::EPSILON).unwrap_or_default()
}

/// Unsigned angle in degrees; a zero vector counts as 180° so a degenerate head never passes silently.
fn angle_between_degrees(a: ScreenVector, b: ScreenVector) -> f64 {
    let lengths = a.length() * b.length();
    if lengths <= f64::EPSILON {
        return 180.0;
    }
    let cosine = ((a.x * b.x + a.y * b.y) / lengths).clamp(-1.0, 1.0);
    cosine.acos().to_degrees()
}

/// The first node whose body covers `tip`. The target only counts when it is tall: low nodes
/// (switch, load balancer) accept any port, as in the prototype. Nor does it count when the head is drawn
/// midway (`schematic`): nothing is painted at the port, and the stroke may run on into the body it enters
/// (T-425: up to the outline of a cloud floating above its footprint).
fn hiding_node(tip: ScreenPoint, target: &str, drawn_midway: bool, silhouettes: &[NodeSilhouette]) -> Option<String> {
    silhouettes
        .iter()
        .filter(|silhouette| silhouette.id != target || (silhouette.is_tall && !drawn_midway))
        .find(|silhouette| silhouette.depth_inside(tip) > HIDDEN_TIP_TOLERANCE_PX)
        .map(|silhouette| silhouette.id.clone())
}

/// Screen-space convex silhouette of a node body: its footprint extruded from the floor to its height.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeSilhouette {
    /// Node id.
    pub id: String,
    /// Tall nodes hide what is behind them; low ones only cover a sliver.
    pub is_tall: bool,
    /// Convex hull, counter-clockwise in screen coordinates.
    pub hull: Vec<ScreenPoint>,
}

impl NodeSilhouette {
    /// Silhouette of a scene node.
    pub fn of(node: &SceneNode, tile_px: f64) -> Self {
        let projection = Projection::new(tile_px);
        let lift = ScreenVector::new(0.0, -node.shape.height * tile_px);
        let floor: Vec<ScreenPoint> = footprint_outline(&node.shape.footprint)
            .into_iter()
            .map(|offset| node.floor_center + projection.project_vector(offset))
            .collect();
        let top = floor.iter().map(|&point| point + lift);
        let points: Vec<ScreenPoint> = floor.iter().copied().chain(top).collect();
        Self { id: node.id.to_string(), is_tall: node.shape.is_tall(), hull: convex_hull(points) }
    }

    /// Distance (px) from `point` to the hull border when inside (positive), negative when outside.
    pub fn depth_inside(&self, point: ScreenPoint) -> f64 {
        if self.hull.len() < 3 {
            return f64::NEG_INFINITY;
        }
        let count = self.hull.len();
        (0..count)
            .map(|index| signed_distance_left_of(self.hull[index], self.hull[(index + 1) % count], point))
            .fold(f64::INFINITY, f64::min)
    }
}

fn footprint_outline(footprint: &Footprint) -> Vec<PlanVector> {
    match *footprint {
        Footprint::Rect { half_width, half_depth } => vec![
            PlanVector::new(-half_width, -half_depth),
            PlanVector::new(half_width, -half_depth),
            PlanVector::new(half_width, half_depth),
            PlanVector::new(-half_width, half_depth),
        ],
        Footprint::Circle { radius } => (0..CIRCLE_SILHOUETTE_SAMPLES)
            .map(|index| {
                let angle = std::f64::consts::TAU * index as f64 / CIRCLE_SILHOUETTE_SAMPLES as f64;
                PlanVector::new(radius * angle.cos(), radius * angle.sin())
            })
            .collect(),
    }
}

/// Signed distance of `point` to the line `from → to`; positive on the inner side of a hull built by
/// [`convex_hull`] (screen y grows downwards, so "counter-clockwise" uses the flipped cross product).
fn signed_distance_left_of(from: ScreenPoint, to: ScreenPoint, point: ScreenPoint) -> f64 {
    let edge = to - from;
    let length = edge.length();
    if length <= f64::EPSILON {
        return f64::INFINITY;
    }
    let relative = point - from;
    (edge.x * relative.y - edge.y * relative.x) / length
}

/// Andrew's monotone chain. Output order makes [`signed_distance_left_of`] positive inside.
fn convex_hull(mut points: Vec<ScreenPoint>) -> Vec<ScreenPoint> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let cross = |o: ScreenPoint, a: ScreenPoint, b: ScreenPoint| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut hull: Vec<ScreenPoint> = Vec::with_capacity(points.len() * 2);
    for pass in [points.clone(), points.into_iter().rev().collect()] {
        let floor = hull.len();
        for point in pass {
            while hull.len() >= floor + 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], point) <= 0.0 {
                hull.pop();
            }
            hull.push(point);
        }
        hull.pop();
    }
    hull
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> NodeSilhouette {
        let hull = convex_hull(vec![
            ScreenPoint::new(0.0, 0.0),
            ScreenPoint::new(10.0, 0.0),
            ScreenPoint::new(10.0, 10.0),
            ScreenPoint::new(0.0, 10.0),
            ScreenPoint::new(5.0, 5.0),
        ]);
        NodeSilhouette { id: "square".into(), is_tall: true, hull }
    }

    #[test]
    fn hull_drops_interior_points() {
        assert_eq!(square().hull.len(), 4);
    }

    #[test]
    fn depth_is_positive_inside_and_negative_outside() {
        let silhouette = square();
        assert!((silhouette.depth_inside(ScreenPoint::new(5.0, 5.0)) - 5.0).abs() < 1e-9);
        assert!((silhouette.depth_inside(ScreenPoint::new(1.0, 5.0)) - 1.0).abs() < 1e-9);
        assert!(silhouette.depth_inside(ScreenPoint::new(-2.0, 5.0)) < 0.0);
    }

    #[test]
    fn a_tip_on_the_silhouette_border_is_visible() {
        let silhouettes = [square()];
        assert_eq!(hiding_node(ScreenPoint::new(0.0, 5.0), "other", false, &silhouettes), None, "on the border");
        assert_eq!(hiding_node(ScreenPoint::new(-1.0, 5.0), "other", false, &silhouettes), None, "just outside");
        assert_eq!(hiding_node(ScreenPoint::new(3.0, 5.0), "other", false, &silhouettes), Some("square".to_owned()));
        assert_eq!(hiding_node(ScreenPoint::new(3.0, 5.0), "square", false, &silhouettes), Some("square".to_owned()));
        assert_eq!(
            hiding_node(ScreenPoint::new(3.0, 5.0), "square", true, &silhouettes),
            None,
            "a head drawn midway is not painted at the port: its target may cover the tip"
        );
        assert_eq!(hiding_node(ScreenPoint::new(3.0, 5.0), "other", true, &silhouettes), Some("square".to_owned()));
    }

    #[test]
    fn angle_between_handles_degenerate_vectors() {
        assert_eq!(angle_between_degrees(ScreenVector::default(), ScreenVector::new(1.0, 0.0)), 180.0);
        let right_angle = angle_between_degrees(ScreenVector::new(1.0, 0.0), ScreenVector::new(0.0, 2.0));
        assert!((right_angle - 90.0).abs() < 1e-9);
    }

    #[test]
    fn polyline_arrival_uses_last_non_degenerate_segment() {
        let path = EdgePath::Polyline(vec![
            ScreenPoint::new(0.0, 0.0),
            ScreenPoint::new(10.0, 0.0),
            ScreenPoint::new(10.0, 10.0),
            ScreenPoint::new(10.0, 10.0),
        ]);
        assert_eq!(arrival_direction(&path, HeadEnd::Target), ScreenVector::new(0.0, 10.0));
        assert_eq!(arrival_direction(&path, HeadEnd::Source), ScreenVector::new(-10.0, 0.0));
    }
}
