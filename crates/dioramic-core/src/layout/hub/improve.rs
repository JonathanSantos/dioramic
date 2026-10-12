//! Local improvement of the legalized layout (the node moves and swaps of Freivalds & Glagolevs' compact
//! orthogonal layout): inside each zone, two nodes trade cells, or a node moves to a free cell of the zone's
//! grid, whenever that lowers the cost of the drawing. The cost is what a reader pays: the Manhattan length of
//! the edges (aligned edges are the cheapest), each crossing between edges, and each edge running over a node.
//! Greedy, first improvement, fixed visiting order: deterministic.

use crate::geometry::PlanPoint;

use super::score::{
    CROSSING_COST, DIAGONAL_COST, EdgeShape, THROUGH_NODE_COST, closed_ls_cost, edge_length, is_diagonal, may_block,
    near_alignment_cost, segments_cross, share_a_node, through_node_share,
};

/// Cost of one tile² of zone floor, in tiles of edge length: a zone shaped as a line of three instead of an L
/// with a hole saves about 6 tiles², worth 1.5 tiles of edges.
const AREA_COST: f64 = 0.25;
/// Improvement passes at most (each pass tries every swap and move once).
const MAX_PASSES: usize = 3;
/// Two nodes trade places only when at most this many cells apart along each axis (farther swaps rarely pay and
/// cost time).
const MAX_SWAP_CELLS: f64 = 1.0;
/// A change must lower the cost by more than this to be taken.
const MIN_GAIN: f64 = 1e-9;

/// Nodes that may trade places, and the free cells they may move to (plan coordinates).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SwapGroup {
    /// Nodes of the group.
    pub nodes: Vec<usize>,
    /// Empty cells of the group's grid and of the ring around it.
    pub free_cells: Vec<PlanPoint>,
    /// Room the zone keeps around its nodes (its floor is the nodes' box grown by this).
    pub padding: f64,
}

/// A candidate change.
#[derive(Debug, Clone, Copy)]
enum Change {
    Swap(usize, usize),
    Move(usize, usize),
}

/// Improves `centers` in place by swaps and moves inside each group.
pub(super) fn improve(
    edges: &[(usize, usize)],
    cell: (f64, f64),
    shape: EdgeShape,
    centers: &mut [PlanPoint],
    groups: &mut [SwapGroup],
) {
    let drawing = Drawing { edges, incident: incident_edges(edges, centers.len()), cell, shape };
    for _ in 0..MAX_PASSES {
        let mut improved = false;
        for group in groups.iter_mut() {
            // Refreshed once per group, not after every accepted change: a stale bound may only skip a change
            // that would have paid, never accept one that does not (the cost itself is always recounted).
            let penalties = drawing.penalties(centers);
            for change in candidates(group) {
                if try_change(&drawing, &penalties, centers, group, change) {
                    improved = true;
                }
            }
        }
        if !improved {
            break;
        }
    }
}

fn candidates(group: &SwapGroup) -> Vec<Change> {
    let mut changes = Vec::new();
    for (index, &first) in group.nodes.iter().enumerate() {
        for &second in &group.nodes[index + 1..] {
            changes.push(Change::Swap(first, second));
        }
    }
    for &node in &group.nodes {
        for cell in 0..group.free_cells.len() {
            changes.push(Change::Move(node, cell));
        }
    }
    changes
}

/// Applies `change` when it lowers the cost; returns whether it did.
/// Applies `change` when it lowers the cost; returns whether it did. `penalties` (the crossing, node and diagonal
/// costs touching each node) bound what a change can save beyond length and area, so most hopeless changes are
/// rejected without counting crossings; the others are counted exactly, the new state only until it is known not
/// to pay.
fn try_change(
    drawing: &Drawing<'_>,
    penalties: &[f64],
    centers: &mut [PlanPoint],
    group: &mut SwapGroup,
    change: Change,
) -> bool {
    let (moved_pair, worth_trying) = match change {
        Change::Swap(a, b) => ([a, b], drawing.within(centers[a], centers[b], MAX_SWAP_CELLS)),
        Change::Move(node, cell) => ([node, node], drawing.move_is_local(centers, node, group.free_cells[cell])),
    };
    if !worth_trying {
        return false;
    }
    let moved: &[usize] = if moved_pair[0] == moved_pair[1] { &moved_pair[..1] } else { &moved_pair };
    // Length of the edges at the moved nodes (an edge between two swapped nodes counts twice on both sides,
    // which leaves the comparison intact) plus the floor of the group.
    let cheap = |centers: &[PlanPoint], group: &SwapGroup| {
        moved.iter().map(|&node| drawing.node_length(centers, node)).sum::<f64>()
            + AREA_COST * floor_area(group, centers)
    };
    let saving_bound = cheap(centers, group) + moved.iter().map(|&node| penalties[node]).sum::<f64>();
    apply(centers, group, change);
    let hopeless = cheap(centers, group) >= saving_bound - MIN_GAIN;
    apply(centers, group, change); // both changes are their own inverse
    if hopeless {
        return false;
    }
    // Edges of other nodes whose L's a moved node may close or open, before or after the change.
    let mut watched = drawing.watched(centers, moved);
    apply(centers, group, change);
    watched.extend(drawing.watched(centers, moved));
    apply(centers, group, change);
    watched.sort_unstable();
    watched.dedup();
    let before = drawing.local_cost(centers, moved, f64::INFINITY, &watched) + AREA_COST * floor_area(group, centers);
    apply(centers, group, change);
    let target = before - AREA_COST * floor_area(group, centers) - MIN_GAIN;
    if drawing.local_cost(centers, moved, target, &watched) < target {
        return true;
    }
    apply(centers, group, change);
    false
}

fn apply(centers: &mut [PlanPoint], group: &mut SwapGroup, change: Change) {
    match change {
        Change::Swap(a, b) => centers.swap(a, b),
        Change::Move(node, cell) => std::mem::swap(&mut centers[node], &mut group.free_cells[cell]),
    }
}

/// Floor of the group: the box of its nodes grown by its padding.
fn floor_area(group: &SwapGroup, centers: &[PlanPoint]) -> f64 {
    let Some(first) = group.nodes.first().map(|&node| centers[node]) else { return 0.0 };
    let (mut x0, mut x1, mut y0, mut y1) = (first.x, first.x, first.y, first.y);
    for &node in &group.nodes {
        let center = centers[node];
        (x0, x1, y0, y1) = (x0.min(center.x), x1.max(center.x), y0.min(center.y), y1.max(center.y));
    }
    (x1 - x0 + 2.0 * group.padding) * (y1 - y0 + 2.0 * group.padding)
}

fn incident_edges(edges: &[(usize, usize)], count: usize) -> Vec<Vec<usize>> {
    let mut incident = vec![Vec::new(); count];
    for (index, &(from, to)) in edges.iter().enumerate() {
        incident[from].push(index);
        if to != from {
            incident[to].push(index);
        }
    }
    incident
}

/// The edges and what the cost needs about them.
struct Drawing<'a> {
    edges: &'a [(usize, usize)],
    incident: Vec<Vec<usize>>,
    /// Node cell `(along, across)`.
    cell: (f64, f64),
    /// How the edges will be drawn.
    shape: EdgeShape,
}

impl Drawing<'_> {
    /// Whether `a` and `b` are at most `cells` cells apart along each axis.
    fn within(&self, a: PlanPoint, b: PlanPoint, cells: f64) -> bool {
        let slack = 1e-6;
        (a.x - b.x).abs() <= cells * self.cell.0 + slack && (a.y - b.y).abs() <= cells * self.cell.1 + slack
    }

    /// A move is worth counting when it lands next to where the node is or next to one of its neighbours: the
    /// places where its edges can get shorter.
    fn move_is_local(&self, centers: &[PlanPoint], node: usize, target: PlanPoint) -> bool {
        self.within(centers[node], target, 1.0)
            || self.incident[node].iter().any(|&edge| {
                let (from, to) = self.edges[edge];
                let neighbour = if from == node { to } else { from };
                self.within(centers[neighbour], target, 1.0)
            })
    }

    /// The crossing, node and diagonal costs touching each node (its [`Drawing::local_cost`] without length).
    fn penalties(&self, centers: &[PlanPoint]) -> Vec<f64> {
        (0..centers.len())
            .map(|node| {
                let watched = self.watched(centers, &[node]);
                self.local_cost(centers, &[node], f64::INFINITY, &watched) - self.length_cost(centers, &[node])
            })
            .collect()
    }

    /// Elbow edges not touching `moved` whose L's one of the `moved` nodes may block where it stands
    /// ([`closed_ls_cost`] depends on every node, not only on the ends).
    fn watched(&self, centers: &[PlanPoint], moved: &[usize]) -> Vec<usize> {
        if !self.shape.is_elbow() {
            return Vec::new();
        }
        let affected = self.affected_edges(moved);
        (0..self.edges.len())
            .filter(|edge| affected.binary_search(edge).is_err())
            .filter(|&edge| {
                let (from, to) = (centers[self.edges[edge].0], centers[self.edges[edge].1]);
                moved.iter().any(|&node| may_block(from, to, centers[node]))
            })
            .collect()
    }

    /// Manhattan length of the edges at `node`.
    fn node_length(&self, centers: &[PlanPoint], node: usize) -> f64 {
        self.incident[node]
            .iter()
            .map(|&edge| {
                let (from, to) = (centers[self.edges[edge].0], centers[self.edges[edge].1]);
                edge_length(from, to)
            })
            .sum()
    }

    /// Edges touching `moved`, each once.
    fn affected_edges(&self, moved: &[usize]) -> Vec<usize> {
        let mut affected: Vec<usize> = moved.iter().flat_map(|&node| self.incident[node].iter().copied()).collect();
        affected.sort_unstable();
        affected.dedup();
        affected
    }

    /// Manhattan length of the edges touching `moved`: the part of [`Drawing::local_cost`] that is cheap.
    fn length_cost(&self, centers: &[PlanPoint], moved: &[usize]) -> f64 {
        self.affected_edges(moved)
            .into_iter()
            .map(|edge| {
                let (from, to) = (centers[self.edges[edge].0], centers[self.edges[edge].1]);
                edge_length(from, to)
            })
            .sum()
    }

    /// The part of the cost that depends on the positions of `moved`. Counting stops once it reaches `cap` (the
    /// result is then some value ≥ `cap`): a change is rejected as soon as it is known not to pay.
    /// `watched` are the other edges whose L's the moved nodes may block ([`Drawing::watched`]).
    fn local_cost(&self, centers: &[PlanPoint], moved: &[usize], cap: f64, watched: &[usize]) -> f64 {
        let edges = self.edges;
        let affected = self.affected_edges(moved);
        let segment = |edge: usize| (centers[edges[edge].0], centers[edges[edge].1]);
        let mut cost = self.length_cost(centers, moved);
        for &edge in &affected {
            if cost >= cap {
                return cost;
            }
            let (from, to) = segment(edge);
            cost += near_alignment_cost(from, to, self.shape) + closed_ls_cost(edges[edge], centers, self.shape);
            for other in 0..edges.len() {
                let counted_already = other <= edge && affected.binary_search(&other).is_ok();
                if other == edge || counted_already || share_a_node(edges[edge], edges[other]) {
                    continue;
                }
                let (other_from, other_to) = segment(other);
                if segments_cross(from, to, other_from, other_to) {
                    cost += CROSSING_COST;
                }
            }
            for (node, &center) in centers.iter().enumerate() {
                if node != edges[edge].0 && node != edges[edge].1 {
                    cost += THROUGH_NODE_COST * through_node_share(from, to, center, self.shape);
                }
            }
        }
        for (edge, &(from_node, to_node)) in edges.iter().enumerate() {
            if affected.binary_search(&edge).is_ok() {
                continue;
            }
            let (from, to) = (centers[from_node], centers[to_node]);
            for &node in moved {
                cost += THROUGH_NODE_COST * through_node_share(from, to, centers[node], self.shape);
            }
        }
        let closed: f64 = watched.iter().map(|&edge| closed_ls_cost(edges[edge], centers, self.shape)).sum();
        cost + closed + self.diagonal_cost(centers, moved)
    }

    /// Pairs of nodes one cell apart diagonally: in two of the four camera rotations they stand one above the
    /// other on screen, and the label tower of the front one runs into the icon behind it.
    fn diagonal_cost(&self, centers: &[PlanPoint], moved: &[usize]) -> f64 {
        let mut cost = 0.0;
        for (position, &node) in moved.iter().enumerate() {
            for (other, &center) in centers.iter().enumerate() {
                let counted_already = moved[..position].contains(&other);
                if other == node || counted_already {
                    continue;
                }
                if is_diagonal(centers[node], center, self.cell) {
                    cost += DIAGONAL_COST;
                }
            }
        }
        cost
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_swap_untangles_a_crossing() {
        // a(0,0) → c(2,2) and b(0,2) → d(2,0) cross; swapping c and d untangles them.
        let edges = [(0, 2), (1, 3)];
        let mut centers = vec![
            PlanPoint::new(0.0, 0.0),
            PlanPoint::new(0.0, 2.0),
            PlanPoint::new(2.0, 2.0),
            PlanPoint::new(2.0, 0.0),
        ];
        let mut groups = [SwapGroup { nodes: vec![2, 3], free_cells: Vec::new(), padding: 1.0 }];
        improve(&edges, (2.0, 2.0), EdgeShape::Straight, &mut centers, &mut groups);
        assert_eq!(centers[2], PlanPoint::new(2.0, 0.0));
        assert_eq!(centers[3], PlanPoint::new(2.0, 2.0));
    }

    #[test]
    fn a_node_moves_off_an_edge_into_a_free_cell() {
        // b sits on the edge a → c; the free cell beside it is cheaper.
        let edges = [(0, 2), (1, 2)];
        let mut centers = vec![PlanPoint::new(0.0, 0.0), PlanPoint::new(2.0, 0.0), PlanPoint::new(4.0, 0.0)];
        let mut groups = [SwapGroup { nodes: vec![1], free_cells: vec![PlanPoint::new(4.0, 2.0)], padding: 0.0 }];
        improve(&edges, (2.0, 2.0), EdgeShape::Straight, &mut centers, &mut groups);
        assert_eq!(centers[1], PlanPoint::new(4.0, 2.0));
    }
}
