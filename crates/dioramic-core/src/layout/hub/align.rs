//! Rail alignment of packed blocks (T-425). The packing ([`super::compact`]) minimizes the squared length of the
//! edges between blocks: a node linked to two nodes of another block lands half way between them, and the
//! blocks of two linked zones settle a fraction of a cell apart. Drawn as rails, every such edge is an L with a
//! leg too short to read (a hook or a step at the port). With elbow routing the blocks then slide, one at a time
//! and inside the room their separations leave, onto the positions that align their edges, whenever that lowers
//! the cost a reader pays for the edges between blocks ([`links_cost`]).

use crate::geometry::{PlanPoint, PlanRect, PlanVector};

use super::compact::{Axis, PackItem, PackLink};
use super::lattice::{snap_down, snap_up};
use super::score::{
    EdgeShape, PICTURE_COST, THROUGH_NODE_COST, closed_ls_cost, edge_length, near_alignment_cost, picture_area,
    through_node_share,
};

/// Sliding passes at most (each takes the best single slide of one block along one axis).
const MAX_PASSES: usize = 8;
/// A slide must lower the cost by more than this to be taken.
const MIN_GAIN: f64 = 1e-9;
/// Where a block may slide relative to the alignment of one of its links (tiles): onto it, or far enough off it
/// that the link becomes a clean L ([`super::score`]'s near misses end below 2.5 tiles).
const OFFSETS: [f64; 3] = [0.0, -2.5, 2.5];
/// Positions closer than this (tiles) are the same.
const SAME: f64 = 1e-6;

/// `before` must end before `after` starts along an axis: `t_after − t_before ≥ gap`.
pub(super) type Constraint = (usize, usize, f64);

/// Slides the blocks at translations `x`/`y` (one per item) onto alignments of their links while the cost of the
/// links drops, keeping every separation (`constraints`, along x and along y) on the lattice: a block that slides
/// into a neighbour pushes it along.
pub(super) fn align_blocks(
    items: &[PackItem],
    links: &[PackLink],
    constraints: (&[Constraint], &[Constraint]),
    shape: EdgeShape,
    x: &mut Vec<f64>,
    y: &mut Vec<f64>,
) {
    let mut current = links_cost(items, links, shape, x, y);
    for _ in 0..MAX_PASSES {
        let mut best: Option<(f64, Axis, Vec<f64>)> = None;
        for axis in [Axis::X, Axis::Y] {
            let (values, axis_constraints) = match axis {
                Axis::X => (&*x, constraints.0),
                Axis::Y => (&*y, constraints.1),
            };
            for item in 0..items.len() {
                for target in alignment_targets(links, item, axis, x, y) {
                    if (target - values[item]).abs() < SAME {
                        continue;
                    }
                    let slid = slide(values, item, target, axis_constraints);
                    let cost = match axis {
                        Axis::X => links_cost(items, links, shape, &slid, y),
                        Axis::Y => links_cost(items, links, shape, x, &slid),
                    };
                    if cost < current - MIN_GAIN
                        && best.as_ref().is_none_or(|(best_cost, ..)| cost < *best_cost - MIN_GAIN)
                    {
                        best = Some((cost, axis, slid));
                    }
                }
            }
        }
        let Some((cost, axis, slid)) = best else { break };
        match axis {
            Axis::X => *x = slid,
            Axis::Y => *y = slid,
        }
        current = cost;
    }
}

/// `values` with `item` at `target` and the blocks it runs into pushed along (forward when it moves forward,
/// backward when it moves back), until every separation holds again on the lattice.
fn slide(values: &[f64], item: usize, target: f64, constraints: &[Constraint]) -> Vec<f64> {
    let mut values = values.to_vec();
    let forward = target > values[item];
    values[item] = target;
    // Each pass settles at least one more block in the chain; a chain is never longer than the blocks.
    for _ in 0..=values.len() {
        let mut moved = false;
        for &(before, after, gap) in constraints {
            let floor = snap_up(values[before] + gap);
            if values[after] >= floor - SAME {
                continue;
            }
            if forward {
                values[after] = floor;
            } else {
                values[before] = snap_down(values[after] - gap);
            }
            moved = true;
        }
        if !moved {
            break;
        }
    }
    values
}

/// Translations of `item` along `axis` that line one of its links up with the node at the other end.
fn alignment_targets(links: &[PackLink], item: usize, axis: Axis, x: &[f64], y: &[f64]) -> Vec<f64> {
    let translation = |item: usize| match axis {
        Axis::X => x[item],
        Axis::Y => y[item],
    };
    let mut targets: Vec<f64> = links
        .iter()
        .filter_map(|link| {
            let (mine, other, other_point) = if link.from == item {
                (link.from_point, link.to, link.to_point)
            } else if link.to == item {
                (link.to_point, link.from, link.from_point)
            } else {
                return None;
            };
            (other != item).then(|| axis.of_point(other_point) + translation(other) - axis.of_point(mine))
        })
        .flat_map(|aligned| OFFSETS.iter().map(move |offset| aligned + offset))
        .collect();
    targets.sort_by(f64::total_cmp);
    targets.dedup_by(|a, b| (*a - *b).abs() < SAME);
    targets
}

/// What a reader pays for the edges between blocks at translations `x`/`y`: their Manhattan length, the nodes in
/// their way and the near misses of their alignment ([`near_alignment_cost`]), plus the size of the picture the
/// blocks span (a slide outwards that only moves the problem is not worth a larger drawing).
fn links_cost(items: &[PackItem], links: &[PackLink], shape: EdgeShape, x: &[f64], y: &[f64]) -> f64 {
    let shift = |item: usize| PlanVector::new(x[item], y[item]);
    let nodes: Vec<PlanPoint> = items
        .iter()
        .enumerate()
        .flat_map(|(item, pack)| pack.block.nodes.iter().map(move |(_, at)| *at + shift(item)))
        .collect();
    let extents: Vec<PlanRect> = items
        .iter()
        .enumerate()
        .map(|(item, pack)| {
            let by = shift(item);
            PlanRect::new(
                pack.block.extent.x0 + by.x,
                pack.block.extent.y0 + by.y,
                pack.block.extent.x1 + by.x,
                pack.block.extent.y1 + by.y,
            )
        })
        .collect();
    let mut cost = PICTURE_COST * picture_area(&extents);
    for link in links {
        let (from, to) = (link.from_point + shift(link.from), link.to_point + shift(link.to));
        cost += closed_ls_cost_among(from, to, &nodes, shape);
        cost += edge_length(from, to) + near_alignment_cost(from, to, shape);
        for &node in &nodes {
            if node.distance(from) > SAME && node.distance(to) > SAME {
                cost += THROUGH_NODE_COST * through_node_share(from, to, node, shape);
            }
        }
    }
    cost
}

/// [`closed_ls_cost`] of the edge from `from` to `to` among `nodes` (its ends are the nodes standing there).
fn closed_ls_cost_among(from: PlanPoint, to: PlanPoint, nodes: &[PlanPoint], shape: EdgeShape) -> f64 {
    let mut centers = vec![from, to];
    centers.extend(nodes.iter().copied().filter(|node| node.distance(from) > SAME && node.distance(to) > SAME));
    closed_ls_cost((0, 1), &centers, shape)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::block::Block;
    use crate::layout::hub::score::SharpBends;

    const ELBOW: EdgeShape = EdgeShape::Elbow(SharpBends::Unknown);

    fn item(index: usize, center: (f64, f64)) -> PackItem {
        PackItem {
            block: Block::node_in_cell(index, PlanPoint::new(center.0, center.1), (2.0, 2.0), false),
            anchor: PlanVector::new(0.0, 0.0),
        }
    }

    #[test]
    fn a_block_half_a_cell_off_slides_onto_its_neighbour() {
        // Two nodes linked across the flow, 1.5 tiles apart along it: the rail would be an L with a stub.
        let items = [item(0, (0.0, 0.0)), item(1, (0.0, 0.0))];
        let links = [PackLink { from: 0, from_point: PlanPoint::default(), to: 1, to_point: PlanPoint::default() }];
        let (mut x, mut y) = (vec![0.0, 1.5], vec![0.0, 4.0]);
        let separations: Vec<Constraint> = vec![(0, 1, 2.0)];
        align_blocks(&items, &links, (&[], &separations), ELBOW, &mut x, &mut y);
        assert!((x[0] - x[1]).abs() < 1e-9, "aligned: {x:?}");
        assert!(y[1] - y[0] >= 2.0 - 1e-9, "the separation across holds: {y:?}");
    }

    #[test]
    fn a_block_pushes_the_block_it_runs_into() {
        let constraints: Vec<Constraint> = vec![(0, 1, 2.0), (1, 2, 2.0)];
        let slid = slide(&[0.0, 2.0, 4.0], 0, 1.0, &constraints);
        assert_eq!(slid, vec![1.0, 3.0, 5.0]);
        let back = slide(&[0.0, 2.0, 4.0], 2, 3.0, &constraints);
        assert_eq!(back, vec![-1.0, 1.0, 3.0]);
        assert_eq!(slide(&[0.0, 5.0], 0, 1.0, &[(0, 1, 2.0)]), vec![1.0, 5.0], "room enough: nothing pushed");
    }
}
