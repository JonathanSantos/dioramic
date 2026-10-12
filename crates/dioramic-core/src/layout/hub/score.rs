//! The cost of a drawing, as a reader pays it: long edges (Manhattan: aligned edges are the cheapest), edges
//! crossing, edges running over nodes, nodes standing diagonally next to each other (their labels collide in
//! half the camera rotations) and the size of the picture. The local improvement uses its parts; the hub layout
//! uses the whole to pick the best of its candidates.

use crate::geometry::{COS_30, PlanPoint, PlanRect, SIN_30};

/// Cost of one crossing between two edges, in tiles of edge length.
pub(super) const CROSSING_COST: f64 = 4.0;
/// Cost of an edge running over a node that is not one of its ends, in tiles of edge length.
pub(super) const THROUGH_NODE_COST: f64 = 6.0;
/// Cost of two nodes one cell apart diagonally, in tiles of edge length.
pub(super) const DIAGONAL_COST: f64 = 2.0;
/// Cost of one tile² of picture (the screen box of the floor), in tiles of edge length.
pub(super) const PICTURE_COST: f64 = 0.15;
/// Half side of the square a node blocks for the edges of others (tiles).
const NODE_HALF: f64 = 0.6;
/// Offsets within this of one cell count as one cell (tiles).
const DIAGONAL_TOLERANCE: f64 = 1e-6;
/// Share of a node-crossing an aligned elbow edge pays for a node standing on it (T-425): the rail cannot run
/// straight and takes a U around the node (three bends, often under its label), worse than the Z an unaligned
/// edge takes round a node on each of its L's.
const ALIGNED_ELBOW_BLOCK_SHARE: f64 = 7.0;
/// Share of a node-crossing an unaligned elbow edge pays for each of its two L's a node stands on: the router
/// takes the free one.
const ELBOW_L_BLOCK_SHARE: f64 = 0.5;
/// Cost (tiles of edge length) of an unaligned elbow edge whose two L's both run over nodes, on top of their
/// shares: only a Z through the corridor beside them is left, which often turns back into the target at the end
/// (T-425: `orders -> db` of the web-platform).
const CLOSED_LS_COST: f64 = 10.0;
/// Two elbow ends whose centers stand closer than this across (tiles) without being aligned cannot be joined
/// straight, and the leg of their L is too short to read: the rail ends in a hook or a step at the port (T-425:
/// the web-platform clients half a cell off the cloud).
const NEAR_ALIGNED_MAX: f64 = 2.25;
/// Factor on the cost of a near miss whose bends will be sharp on screen ([`EdgeShape::sharpness`]): the rail
/// then zigzags or hooks back into its node instead of stepping down like a stair.
const SHARP_FACTOR: f64 = 1.5;
/// Cost of such an edge, in tiles of edge length.
const NEAR_ALIGNED_COST: f64 = 12.0;

/// How the edges will be drawn, as far as the layout cares: which floor they run over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EdgeShape {
    /// Close to the segment between the two centers (`curved`, `straight`).
    Straight,
    /// Axis legs (`rails`, `orthogonal`): an edge between two unaligned nodes runs as one of its two L's, so a
    /// node standing on the L's corner is in its way even when it is far from the diagonal (T-413). Which of its
    /// bends will be sharp on screen depends on the mirror the drawing ends up in (T-425).
    Elbow(SharpBends),
}

/// Which elbow edges turn sharply on screen at rotation 0, the rail running back against itself (up-right then
/// up-left, a 120° turn instead of 60°): those whose plan offsets have the same sign, once the drawing is
/// mirrored for its reading ([`sharp_misses`], T-425).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SharpBends {
    /// The mirror is not chosen yet: no edge counts as sharp.
    Unknown,
    /// The drawing keeps the sign of `x · y` (identity, or both axes flipped): edges whose offsets agree.
    WhenOffsetsAgree,
    /// The drawing flips the sign of `x · y` (one axis flipped): edges whose offsets differ.
    WhenOffsetsDiffer,
}

impl EdgeShape {
    /// True for elbow edges.
    pub(super) fn is_elbow(self) -> bool {
        matches!(self, EdgeShape::Elbow(_))
    }

    /// Factor on the near misses of the edge from `from` to `to`: [`SHARP_FACTOR`] when its bends will be sharp.
    fn sharpness(self, from: PlanPoint, to: PlanPoint) -> f64 {
        let product = (to.x - from.x) * (to.y - from.y);
        let sharp = match self {
            EdgeShape::Elbow(SharpBends::WhenOffsetsAgree) => product > 0.0,
            EdgeShape::Elbow(SharpBends::WhenOffsetsDiffer) => product < 0.0,
            EdgeShape::Elbow(SharpBends::Unknown) | EdgeShape::Straight => false,
        };
        if sharp { SHARP_FACTOR } else { 1.0 }
    }
}

/// How much of a node-crossing an edge from `from` to `to` pays for `node` (0 to 1). A straight edge pays when
/// its segment passes over the node; an elbow edge between unaligned nodes pays half for each of its two L's
/// the node stands on (the router takes the free one; a node on both, or nodes on each, cost a whole crossing).
pub(super) fn through_node_share(from: PlanPoint, to: PlanPoint, node: PlanPoint, shape: EdgeShape) -> f64 {
    if !shape.is_elbow() || is_aligned(from, to) {
        let share = if shape.is_elbow() { ALIGNED_ELBOW_BLOCK_SHARE } else { 1.0 };
        return if segment_hits(from, to, node) { share } else { 0.0 };
    }
    let on_l = |corner: PlanPoint| segment_hits(from, corner, node) || segment_hits(corner, to, node);
    let hits = [PlanPoint::new(to.x, from.y), PlanPoint::new(from.x, to.y)].into_iter().filter(|&c| on_l(c)).count();
    hits as f64 * ELBOW_L_BLOCK_SHARE
}

/// Manhattan length of the edge from `from` to `to` (aligned edges are the cheapest).
pub(super) fn edge_length(from: PlanPoint, to: PlanPoint) -> f64 {
    (from.x - to.x).abs() + (from.y - to.y).abs()
}

/// The extra cost of edge `edge` (positions in `centers`) when it is an unaligned elbow edge whose two L's are
/// both blocked by other nodes ([`CLOSED_LS_COST`]); 0 otherwise.
pub(super) fn closed_ls_cost(edge: (usize, usize), centers: &[PlanPoint], shape: EdgeShape) -> f64 {
    if shape.is_elbow() && ls_closed(edge, centers) {
        CLOSED_LS_COST * shape.sharpness(centers[edge.0], centers[edge.1])
    } else {
        0.0
    }
}

/// True when edge `edge` (positions in `centers`) joins two unaligned nodes and other nodes stand on both of its
/// L's.
fn ls_closed(edge: (usize, usize), centers: &[PlanPoint]) -> bool {
    let (from, to) = (centers[edge.0], centers[edge.1]);
    if is_aligned(from, to) {
        return false;
    }
    let others =
        || centers.iter().enumerate().filter(|(node, _)| *node != edge.0 && *node != edge.1).map(|(_, at)| *at);
    let closed =
        |corner: PlanPoint| others().any(|node| segment_hits(from, corner, node) || segment_hits(corner, to, node));
    closed(PlanPoint::new(to.x, from.y)) && closed(PlanPoint::new(from.x, to.y))
}

/// True when the two points share a column or a row.
fn is_aligned(from: PlanPoint, to: PlanPoint) -> bool {
    (from.x - to.x).abs() < DIAGONAL_TOLERANCE || (from.y - to.y).abs() < DIAGONAL_TOLERANCE
}

/// True when the two points stand nearly but not quite in line ([`NEAR_ALIGNED_MAX`]).
fn is_near_aligned(from: PlanPoint, to: PlanPoint) -> bool {
    let across = (from.x - to.x).abs().min((from.y - to.y).abs());
    across > DIAGONAL_TOLERANCE && across < NEAR_ALIGNED_MAX
}

/// Elbow edges whose rail will read badly at rotation 0 (T-425): edges nearly aligned (a hook or a step at a port)
/// or with both L's closed (a Z through the corridor), when their bends are sharp on screen. A bend is sharp, the
/// rail running back against itself, when the edge runs down-right and down-left at once (or up-left and
/// up-right): its plan offsets have the same sign. A mirror of the drawing turns them into gentle stair steps.
pub(super) fn sharp_misses(edges: &[(usize, usize)], centers: &[PlanPoint]) -> usize {
    edges
        .iter()
        .filter(|&&(from, to)| {
            let (a, b) = (centers[from], centers[to]);
            let sharp = (b.x - a.x) * (b.y - a.y) > 0.0;
            sharp && (is_near_aligned(a, b) || ls_closed((from, to), centers))
        })
        .count()
}

/// True when `node` stands where it may block an L of the edge from `from` to `to` (inside the box they span,
/// grown by a node's half side).
pub(super) fn may_block(from: PlanPoint, to: PlanPoint, node: PlanPoint) -> bool {
    node.x >= from.x.min(to.x) - NODE_HALF
        && node.x <= from.x.max(to.x) + NODE_HALF
        && node.y >= from.y.min(to.y) - NODE_HALF
        && node.y <= from.y.max(to.y) + NODE_HALF
}

/// What an elbow edge from `from` to `to` costs beyond its length and the nodes in its way (T-425): nodes nearly
/// but not quite aligned ([`NEAR_ALIGNED_MAX`]) give a hook or a step at a port. 0 for straight edges.
pub(super) fn near_alignment_cost(from: PlanPoint, to: PlanPoint, shape: EdgeShape) -> f64 {
    if shape.is_elbow() && is_near_aligned(from, to) { NEAR_ALIGNED_COST * shape.sharpness(from, to) } else { 0.0 }
}

/// Total cost of a drawing: node centers, edges and the floor rectangles (zones, node cells).
pub(super) fn drawing_cost(
    edges: &[(usize, usize)],
    centers: &[PlanPoint],
    floor: &[PlanRect],
    cell: (f64, f64),
    shape: EdgeShape,
) -> f64 {
    let mut cost = PICTURE_COST * picture_area(floor);
    for (index, &(from, to)) in edges.iter().enumerate() {
        let (a, b) = (centers[from], centers[to]);
        cost += edge_length(a, b) + near_alignment_cost(a, b, shape) + closed_ls_cost((from, to), centers, shape);
        for &other in &edges[index + 1..] {
            if !share_a_node((from, to), other) && segments_cross(a, b, centers[other.0], centers[other.1]) {
                cost += CROSSING_COST;
            }
        }
        for (node, &center) in centers.iter().enumerate() {
            if node != from && node != to {
                cost += THROUGH_NODE_COST * through_node_share(a, b, center, shape);
            }
        }
    }
    for (index, &first) in centers.iter().enumerate() {
        for &second in &centers[index + 1..] {
            if is_diagonal(first, second, cell) {
                cost += DIAGONAL_COST;
            }
        }
    }
    cost
}

/// Area of the screen box of the floor rectangles, in tiles² (an isometric plan point `(x, y)` lands at
/// `((x − y)·cos 30°, (x + y)·sin 30°)`).
pub(super) fn picture_area(floor: &[PlanRect]) -> f64 {
    let (mut across, mut down) = ((f64::INFINITY, f64::NEG_INFINITY), (f64::INFINITY, f64::NEG_INFINITY));
    for corner in floor.iter().flat_map(PlanRect::corners) {
        across = (across.0.min(corner.x - corner.y), across.1.max(corner.x - corner.y));
        down = (down.0.min(corner.x + corner.y), down.1.max(corner.x + corner.y));
    }
    if across.1 < across.0 {
        return 0.0;
    }
    (across.1 - across.0) * COS_30 * (down.1 - down.0) * SIN_30
}

/// Two nodes exactly one cell apart along both axes.
pub(super) fn is_diagonal(a: PlanPoint, b: PlanPoint, cell: (f64, f64)) -> bool {
    let (dx, dy) = ((a.x - b.x).abs(), (a.y - b.y).abs());
    (dx - cell.0).abs() < DIAGONAL_TOLERANCE && (dy - cell.1).abs() < DIAGONAL_TOLERANCE
}

pub(super) fn share_a_node(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 == b.0 || a.0 == b.1 || a.1 == b.0 || a.1 == b.1
}

fn orientation(a: PlanPoint, b: PlanPoint, c: PlanPoint) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

/// Proper crossing (touching or collinear segments do not count).
pub(super) fn segments_cross(a: PlanPoint, b: PlanPoint, c: PlanPoint, d: PlanPoint) -> bool {
    let apart = a.x.max(b.x) < c.x.min(d.x)
        || c.x.max(d.x) < a.x.min(b.x)
        || a.y.max(b.y) < c.y.min(d.y)
        || c.y.max(d.y) < a.y.min(b.y);
    if apart {
        return false;
    }
    let (first, second) = (orientation(a, b, c), orientation(a, b, d));
    let (third, fourth) = (orientation(c, d, a), orientation(c, d, b));
    first * second < 0.0 && third * fourth < 0.0
}

/// Whether the segment passes over the square a node blocks (Liang–Barsky clipping).
pub(super) fn segment_hits(from: PlanPoint, to: PlanPoint, node: PlanPoint) -> bool {
    let apart = node.x + NODE_HALF < from.x.min(to.x)
        || node.x - NODE_HALF > from.x.max(to.x)
        || node.y + NODE_HALF < from.y.min(to.y)
        || node.y - NODE_HALF > from.y.max(to.y);
    if apart {
        return false;
    }
    let square = PlanRect::around(node, NODE_HALF, NODE_HALF);
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let mut range = (0.0_f64, 1.0_f64);
    for (p, q) in
        [(-dx, from.x - square.x0), (dx, square.x1 - from.x), (-dy, from.y - square.y0), (dy, square.y1 - from.y)]
    {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
            continue;
        }
        let t = q / p;
        if p < 0.0 {
            range.0 = range.0.max(t);
        } else {
            range.1 = range.1.min(t);
        }
        if range.0 > range.1 {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossing_and_hit_tests() {
        let p = PlanPoint::new;
        assert!(segments_cross(p(0.0, 0.0), p(2.0, 2.0), p(0.0, 2.0), p(2.0, 0.0)));
        assert!(!segments_cross(p(0.0, 0.0), p(2.0, 0.0), p(2.0, 0.0), p(2.0, 2.0)));
        assert!(segment_hits(p(0.0, 0.0), p(4.0, 0.0), p(2.0, 0.3)));
        assert!(!segment_hits(p(0.0, 0.0), p(4.0, 0.0), p(2.0, 1.0)));
        assert!(is_diagonal(p(0.0, 0.0), p(2.5, -2.5), (2.5, 2.5)));
    }

    const ELBOW: EdgeShape = EdgeShape::Elbow(SharpBends::Unknown);

    #[test]
    fn a_near_miss_costs_and_an_alignment_or_a_full_cell_does_not() {
        let p = PlanPoint::new;
        assert_eq!(near_alignment_cost(p(0.0, 0.0), p(6.0, 0.0), ELBOW), 0.0, "aligned");
        assert_eq!(near_alignment_cost(p(0.0, 0.0), p(6.0, 1.5), ELBOW), NEAR_ALIGNED_COST, "half a cell off");
        assert_eq!(near_alignment_cost(p(0.0, 0.0), p(6.0, 3.0), ELBOW), 0.0, "a clean L");
        assert_eq!(near_alignment_cost(p(0.0, 0.0), p(6.0, 1.5), EdgeShape::Straight), 0.0, "curves do not care");
    }

    #[test]
    fn an_edge_with_both_ls_closed_costs_and_one_with_a_free_l_does_not() {
        let p = PlanPoint::new;
        // 0 → 1 runs from (0, 0) to (3, 3); nodes stand on (3, 0) and (0, 3), the corners of both L's.
        let closed = [p(0.0, 0.0), p(3.0, 3.0), p(3.0, 0.0), p(0.0, 3.0)];
        assert_eq!(closed_ls_cost((0, 1), &closed, ELBOW), CLOSED_LS_COST);
        let open = [p(0.0, 0.0), p(3.0, 3.0), p(3.0, 0.0), p(-3.0, 6.0)];
        assert_eq!(closed_ls_cost((0, 1), &open, ELBOW), 0.0);
        assert!(may_block(p(0.0, 0.0), p(3.0, 3.0), p(3.0, 0.0)));
        assert!(!may_block(p(0.0, 0.0), p(3.0, 3.0), p(5.0, 0.0)));
    }

    #[test]
    fn a_node_on_an_aligned_elbow_edge_costs_more_than_on_a_curve() {
        let p = PlanPoint::new;
        let (from, to, node) = (p(0.0, 0.0), p(6.0, 0.0), p(3.0, 0.0));
        assert!(through_node_share(from, to, node, ELBOW) > through_node_share(from, to, node, EdgeShape::Straight));
    }

    #[test]
    fn sharp_misses_follow_the_signs_of_the_offsets() {
        let p = PlanPoint::new;
        // Down-right and down-left at once (offsets of one sign): a sharp bend on screen.
        let sharp = [p(0.0, 0.0), p(6.0, 1.5)];
        assert_eq!(sharp_misses(&[(0, 1)], &sharp), 1);
        let gentle = [p(0.0, 0.0), p(6.0, -1.5)];
        assert_eq!(sharp_misses(&[(0, 1)], &gentle), 0);
        let mirrored = EdgeShape::Elbow(SharpBends::WhenOffsetsDiffer);
        assert!(mirrored.sharpness(gentle[0], gentle[1]) > mirrored.sharpness(sharp[0], sharp[1]));
    }

    #[test]
    fn a_crossing_costs_more_than_the_untangled_drawing() {
        let p = PlanPoint::new;
        let edges = [(0, 2), (1, 3)];
        let floor = [PlanRect::new(0.0, 0.0, 4.0, 4.0)];
        let crossed = [p(0.0, 0.0), p(0.0, 4.0), p(4.0, 4.0), p(4.0, 0.0)];
        let untangled = [p(0.0, 0.0), p(0.0, 4.0), p(4.0, 0.0), p(4.0, 4.0)];
        assert!(
            drawing_cost(&edges, &untangled, &floor, (2.5, 2.5), EdgeShape::Straight)
                < drawing_cost(&edges, &crossed, &floor, (2.5, 2.5), EdgeShape::Straight)
        );
    }
}
