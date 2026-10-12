//! Orthogonal compaction of the top level (as in the constrained quadratic step of Freivalds & Glagolevs and
//! the separation-constraint QP of Dwyer, Koren, Marriott): every pair of top-level blocks keeps the side it had
//! after relaxation (left/right or front/back, whichever they were most apart on), and, axis by axis, the blocks
//! slide to minimize the squared length of the edges between them (short, straight edges) with a light pull to
//! their relaxed place, subject to those separations. The result is snapped to the lattice.

use crate::geometry::{PlanPoint, PlanRect, PlanVector};

use super::super::block::Block;
use super::align::{Constraint, align_blocks};
use super::lattice::{snap, snap_up};
use super::qp::{QuadraticProgram, Separation};
use super::score::EdgeShape;
use super::seed::SeedCell;

/// Pull of each block towards its relaxed place, relative to one edge.
const ANCHOR_WEIGHT: f64 = 0.05;

/// A top-level block and the translation that puts it at its relaxed place.
#[derive(Debug, Clone)]
pub(super) struct PackItem {
    /// The block, in local coordinates (nodes on the lattice).
    pub block: Block,
    /// Translation to its relaxed place.
    pub anchor: PlanVector,
}

/// An edge between two blocks: its end points in their blocks' local coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct PackLink {
    /// First block.
    pub from: usize,
    /// End point inside the first block.
    pub from_point: PlanPoint,
    /// Second block.
    pub to: usize,
    /// End point inside the second block.
    pub to_point: PlanPoint,
}

/// Which plan axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Axis {
    X,
    Y,
}

impl Axis {
    pub(super) fn of_point(self, point: PlanPoint) -> f64 {
        match self {
            Axis::X => point.x,
            Axis::Y => point.y,
        }
    }

    fn of_vector(self, vector: PlanVector) -> f64 {
        match self {
            Axis::X => vector.x,
            Axis::Y => vector.y,
        }
    }

    fn span(self, rect: PlanRect) -> (f64, f64) {
        match self {
            Axis::X => (rect.x0, rect.x1),
            Axis::Y => (rect.y0, rect.y1),
        }
    }
}

/// Packs the blocks and merges them into one. With elbow edges the packed blocks then slide onto the alignments
/// of their links ([`align_blocks`], T-425).
pub(super) fn pack(items: Vec<PackItem>, links: &[PackLink], seed: Option<&[SeedCell]>, shape: EdgeShape) -> Block {
    let placed: Vec<PlanRect> = items.iter().map(|item| shifted(item.block.extent, item.anchor)).collect();
    let separations = separations(&placed, seed);
    let (x_constraints, y_constraints) =
        (axis_constraints(&items, &separations, Axis::X), axis_constraints(&items, &separations, Axis::Y));
    let mut x = solve_axis(&items, links, &x_constraints, Axis::X);
    let mut y = solve_axis(&items, links, &y_constraints, Axis::Y);
    if shape.is_elbow() {
        align_blocks(&items, links, (&x_constraints, &y_constraints), shape, &mut x, &mut y);
    }
    let mut group = Block::empty();
    for ((mut item, x), y) in items.into_iter().zip(x).zip(y) {
        item.block.translate(PlanVector::new(x, y));
        group.absorb(item.block);
    }
    group
}

fn shifted(rect: PlanRect, by: PlanVector) -> PlanRect {
    PlanRect::new(rect.x0 + by.x, rect.y0 + by.y, rect.x1 + by.x, rect.y1 + by.y)
}

/// `(before, after, axis)` for each pair of blocks. With a seed, the pair keeps the side the seed gave it: the axis
/// of the larger cell offset, in cell order. Diagonal seed neighbours (and every pair without a seed) take the
/// axis along which their relaxed boxes are farthest apart relative to their sizes, still in seed order when
/// there is one.
fn separations(placed: &[PlanRect], seed: Option<&[SeedCell]>) -> Vec<(usize, usize, Axis)> {
    let mut pairs = Vec::new();
    for (a, first) in placed.iter().enumerate() {
        for (b, second) in placed.iter().enumerate().skip(a + 1) {
            let cells = seed.and_then(|cells| Some((*cells.get(a)?, *cells.get(b)?)));
            let offset = cells.map(|(cell_a, cell_b)| (cell_b.0 - cell_a.0, cell_b.1 - cell_a.1));
            let axis = match offset {
                Some((dx, dy)) if dx.abs() > dy.abs() => Axis::X,
                Some((dx, dy)) if dy.abs() > dx.abs() => Axis::Y,
                _ => relaxed_axis(*first, *second),
            };
            let a_first = match (axis, offset) {
                (Axis::X, Some((dx, _))) if dx != 0 => dx > 0,
                (Axis::Y, Some((_, dy))) if dy != 0 => dy > 0,
                (Axis::X, _) => (first.center().x, a) <= (second.center().x, b),
                (Axis::Y, _) => (first.center().y, a) <= (second.center().y, b),
            };
            pairs.push(if a_first { (a, b, axis) } else { (b, a, axis) });
        }
    }
    pairs
}

/// The axis along which two boxes are farthest apart relative to their sizes (ties: along the flow).
fn relaxed_axis(first: PlanRect, second: PlanRect) -> Axis {
    let (center_a, center_b) = (first.center(), second.center());
    let along = (center_a.x - center_b.x).abs() / ((first.width() + second.width()) / 2.0).max(f64::EPSILON);
    let across = (center_a.y - center_b.y).abs() / ((first.depth() + second.depth()) / 2.0).max(f64::EPSILON);
    if along >= across { Axis::X } else { Axis::Y }
}

/// The separations along `axis` as translation constraints: `before` must end before `after` starts,
/// `t_after − t_before ≥ gap`.
fn axis_constraints(items: &[PackItem], separations: &[(usize, usize, Axis)], axis: Axis) -> Vec<Constraint> {
    let spans: Vec<(f64, f64)> = items.iter().map(|item| axis.span(item.block.extent)).collect();
    separations
        .iter()
        .filter(|(_, _, pair_axis)| *pair_axis == axis)
        .map(|&(before, after, _)| (before, after, spans[before].1 - spans[after].0))
        .collect()
}

/// Translations along one axis: least squares of the links plus the anchors, subject to the separations.
fn solve_axis(items: &[PackItem], links: &[PackLink], constraints: &[Constraint], axis: Axis) -> Vec<f64> {
    let anchors: Vec<f64> = items.iter().map(|item| axis.of_vector(item.anchor)).collect();
    let spans: Vec<(f64, f64)> = items.iter().map(|item| axis.span(item.block.extent)).collect();
    let key = |item: usize| anchors[item] + (spans[item].0 + spans[item].1) / 2.0;
    let mut program = QuadraticProgram::new(items.len());
    for (item, &anchor) in anchors.iter().enumerate() {
        program.add_anchor(item, anchor, ANCHOR_WEIGHT);
    }
    for link in links {
        // (t_from + from_point) − (t_to + to_point) → 0
        let offset = axis.of_point(link.to_point) - axis.of_point(link.from_point);
        program.add_difference(link.from, link.to, offset, 1.0);
    }
    let separations: Vec<Separation> =
        constraints.iter().map(|&(before, after, gap)| Separation { before, after, gap }).collect();
    legalize(program.solve(&separations), constraints, key)
}

/// Snaps to the lattice and makes every separation exact, pushing forward in a topological order of the
/// separations (ties: by `key`, then index).
fn legalize(values: Vec<f64>, constraints: &[(usize, usize, f64)], key: impl Fn(usize) -> f64) -> Vec<f64> {
    let mut values: Vec<f64> = values.into_iter().map(snap).collect();
    for after in topological_order(values.len(), constraints, key) {
        let floor = constraints
            .iter()
            .filter(|(_, other, _)| *other == after)
            .map(|&(before, _, gap)| snap_up(values[before] + gap))
            .fold(f64::NEG_INFINITY, f64::max);
        values[after] = values[after].max(floor);
    }
    values
}

/// Kahn's algorithm over the separations (they never form a cycle: each one points along increasing seed or
/// relaxed coordinates).
fn topological_order(count: usize, constraints: &[(usize, usize, f64)], key: impl Fn(usize) -> f64) -> Vec<usize> {
    let mut pending: Vec<usize> = vec![0; count];
    for &(_, after, _) in constraints {
        pending[after] += 1;
    }
    let mut order = Vec::with_capacity(count);
    let mut done = vec![false; count];
    while order.len() < count {
        let next = (0..count)
            .filter(|&item| !done[item] && pending[item] == 0)
            .min_by(|&a, &b| key(a).total_cmp(&key(b)).then(a.cmp(&b)))
            // Only reachable with a cycle: fall back to the remaining item with the smallest key.
            .or_else(|| (0..count).filter(|&item| !done[item]).min_by(|&a, &b| key(a).total_cmp(&key(b))));
        let Some(next) = next else { break };
        done[next] = true;
        order.push(next);
        for &(before, after, _) in constraints {
            if before == next {
                pending[after] = pending[after].saturating_sub(1);
            }
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(index: usize, side: f64, anchor: (f64, f64)) -> PackItem {
        PackItem {
            block: Block::node_in_cell(index, PlanPoint::default(), (side, side), false),
            anchor: PlanVector::new(anchor.0, anchor.1),
        }
    }

    fn link(from: usize, to: usize) -> PackLink {
        PackLink { from, from_point: PlanPoint::default(), to, to_point: PlanPoint::default() }
    }

    #[test]
    fn linked_blocks_close_the_gap_but_never_overlap() {
        let block = pack(
            vec![square(0, 2.0, (0.0, 0.0)), square(1, 2.0, (9.0, 0.4))],
            &[link(0, 1)],
            None,
            EdgeShape::Straight,
        );
        let (a, b) = (block.nodes[0].1, block.nodes[1].1);
        assert!((b.x - a.x - 2.0).abs() < 1e-9, "packed side by side: {a:?} {b:?}");
        assert!((b.y - a.y).abs() < 1e-9, "the edge between them is straight: {a:?} {b:?}");
    }

    #[test]
    fn blocks_keep_the_side_they_had() {
        let block = pack(
            vec![square(0, 2.0, (0.0, 0.0)), square(1, 2.0, (0.5, 6.0)), square(2, 2.0, (6.0, 0.0))],
            &[link(0, 1), link(0, 2), link(1, 2)],
            None,
            EdgeShape::Straight,
        );
        let at =
            |index: usize| block.nodes.iter().find(|(node, _)| *node == index).map(|(_, at)| *at).unwrap_or_default();
        assert!(at(1).y - at(0).y >= 2.0 - 1e-9 && at(2).x - at(0).x >= 2.0 - 1e-9);
        for point in [at(0), at(1), at(2)] {
            assert_eq!((point.x * 2.0).fract(), 0.0, "on the half-tile lattice: {point:?}");
        }
    }
}
