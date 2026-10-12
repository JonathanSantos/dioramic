//! Legalization of a container on a lattice (the grid-like step of Yoghourdjian et al. and of HOLA): the
//! relaxed positions of its members are clustered into columns and rows, each member takes one cell (the
//! nearest free one on a clash), empty columns and rows vanish, and columns and rows are packed as tight as
//! their widest member allows, on multiples of [`LATTICE_STEP`] so every node center lands on the half-tile grid.

use std::collections::BTreeMap;

use dioramic_spec::Arrange;

use crate::geometry::{PlanPoint, PlanVector};

use super::super::block::Block;

/// Node centers land on multiples of this (tiles): the half-tile lattice.
pub(in crate::layout) const LATTICE_STEP: f64 = 0.5;
/// Positions closer than this fraction of a cell share a column (or a row).
const CLUSTER_TOLERANCE: f64 = 0.5;
/// Rounding slack: a value this close under a lattice multiple is that multiple.
const SNAP_SLACK: f64 = 1e-9;

/// Nearest lattice multiple.
pub(super) fn snap(value: f64) -> f64 {
    (value / LATTICE_STEP).round() * LATTICE_STEP
}

/// Largest lattice multiple at or below `value`.
pub(super) fn snap_down(value: f64) -> f64 {
    (value / LATTICE_STEP + SNAP_SLACK).floor() * LATTICE_STEP
}

/// Smallest lattice multiple at or above `value`.
pub(super) fn snap_up(value: f64) -> f64 {
    (value / LATTICE_STEP - SNAP_SLACK).ceil() * LATTICE_STEP
}

/// A member to legalize: its laid-out block (local coordinates, nodes on the lattice) and where the relaxation
/// wants its center.
#[derive(Debug, Clone)]
pub(super) struct LatticeItem {
    /// The member, laid out.
    pub block: Block,
    /// Target center.
    pub position: PlanPoint,
    /// A single node (centered exactly on its cell); a nested zone may sit up to half a step off center.
    pub is_node: bool,
}

/// Packs the items into an aligned grid that keeps their relative order. `arrange: row | column` force one row
/// or one column (in the order of the relaxed positions); `auto` and `grid` follow the relaxed positions.
pub(super) fn lattice(items: Vec<LatticeItem>, arrange: Arrange, cell: (f64, f64)) -> Block {
    if items.is_empty() {
        return Block::empty();
    }
    let slots = assign_cells(&items, arrange, cell);
    let columns = dense_ranks(slots.iter().map(|slot| slot.0));
    let rows = dense_ranks(slots.iter().map(|slot| slot.1));
    let column_centers = pack_axis(&items, &columns, |item| item.block.extent.width());
    let row_centers = pack_axis(&items, &rows, |item| item.block.extent.depth());
    let mut group = Block::empty();
    for ((mut item, column), row) in items.into_iter().zip(columns).zip(rows) {
        let target = PlanPoint::new(column_centers[column], row_centers[row]);
        let center = item.block.extent.center();
        item.block.translate(PlanVector::new(snap(target.x - center.x), snap(target.y - center.y)));
        group.absorb(item.block);
    }
    group
}

/// `(column, row)` of each item before compaction (may be negative or sparse).
fn assign_cells(items: &[LatticeItem], arrange: Arrange, cell: (f64, f64)) -> Vec<(i64, i64)> {
    let xs: Vec<f64> = items.iter().map(|item| item.position.x).collect();
    let ys: Vec<f64> = items.iter().map(|item| item.position.y).collect();
    match arrange {
        Arrange::Row => order_of(&xs).into_iter().map(|column| (column, 0)).collect(),
        Arrange::Column => order_of(&ys).into_iter().map(|row| (0, row)).collect(),
        Arrange::Auto | Arrange::Grid => {
            let columns = cluster(&xs, CLUSTER_TOLERANCE * cell.0);
            let rows = cluster(&ys, CLUSTER_TOLERANCE * cell.1);
            resolve_clashes(items, &columns, &rows, cell)
        }
    }
}

/// Position of each value in ascending order (ties: input order).
fn order_of(values: &[f64]) -> Vec<i64> {
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|&a, &b| values[a].total_cmp(&values[b]).then(a.cmp(&b)));
    let mut rank = vec![0; values.len()];
    for (position, index) in order.into_iter().enumerate() {
        rank[index] = position as i64;
    }
    rank
}

/// Groups sorted values into lines: a value joins the current line while it stays within `tolerance` of the
/// line's first value.
fn cluster(values: &[f64], tolerance: f64) -> Vec<i64> {
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|&a, &b| values[a].total_cmp(&values[b]).then(a.cmp(&b)));
    let mut line = vec![0; values.len()];
    let mut current = -1_i64;
    let mut first = f64::NEG_INFINITY;
    for index in order {
        if values[index] - first > tolerance {
            current += 1;
            first = values[index];
        }
        line[index] = current;
    }
    line
}

/// Mean position of the values on each line, extended one line before and after by `step`.
fn line_positions(lines: &[i64], values: &[f64], step: f64) -> BTreeMap<i64, f64> {
    let mut sums: BTreeMap<i64, (f64, f64)> = BTreeMap::new();
    for (&line, &value) in lines.iter().zip(values) {
        let entry = sums.entry(line).or_default();
        *entry = (entry.0 + value, entry.1 + 1.0);
    }
    let mut positions: BTreeMap<i64, f64> = sums.into_iter().map(|(line, (sum, count))| (line, sum / count)).collect();
    let first = positions.iter().next().map(|(line, value)| (*line, *value));
    let last = positions.iter().next_back().map(|(line, value)| (*line, *value));
    if let (Some((first_line, first_value)), Some((last_line, last_value))) = (first, last) {
        positions.insert(first_line - 1, first_value - step);
        positions.insert(last_line + 1, last_value + step);
    }
    positions
}

/// One item per cell: an item landing on a taken cell moves to the free cell (within one line around the grid)
/// nearest to its relaxed position.
fn resolve_clashes(items: &[LatticeItem], columns: &[i64], rows: &[i64], cell: (f64, f64)) -> Vec<(i64, i64)> {
    let xs: Vec<f64> = items.iter().map(|item| item.position.x).collect();
    let ys: Vec<f64> = items.iter().map(|item| item.position.y).collect();
    let column_x = line_positions(columns, &xs, cell.0);
    let row_y = line_positions(rows, &ys, cell.1);
    let mut taken: BTreeMap<(i64, i64), usize> = BTreeMap::new();
    let mut result = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let wanted = (columns[index], rows[index]);
        let chosen = if taken.contains_key(&wanted) {
            nearest_free(item.position, &column_x, &row_y, &taken).unwrap_or(wanted)
        } else {
            wanted
        };
        taken.insert(chosen, index);
        result.push(chosen);
    }
    result
}

fn nearest_free(
    position: PlanPoint,
    column_x: &BTreeMap<i64, f64>,
    row_y: &BTreeMap<i64, f64>,
    taken: &BTreeMap<(i64, i64), usize>,
) -> Option<(i64, i64)> {
    let mut best: Option<(f64, (i64, i64))> = None;
    for (&column, &x) in column_x {
        for (&row, &y) in row_y {
            if taken.contains_key(&(column, row)) {
                continue;
            }
            let distance = position.distance(PlanPoint::new(x, y));
            if best.is_none_or(|(best_distance, _)| distance < best_distance) {
                best = Some((distance, (column, row)));
            }
        }
    }
    best.map(|(_, cell)| cell)
}

/// Renumbers sparse line ids to `0..n` keeping their order (empty lines vanish).
fn dense_ranks(lines: impl Iterator<Item = i64>) -> Vec<usize> {
    let lines: Vec<i64> = lines.collect();
    let mut distinct = lines.clone();
    distinct.sort_unstable();
    distinct.dedup();
    lines.iter().map(|line| distinct.partition_point(|other| other < line)).collect()
}

/// Lattice centers of consecutive lines, each as close to the previous one as their widest items allow.
fn pack_axis(items: &[LatticeItem], lines: &[usize], size: impl Fn(&LatticeItem) -> f64) -> Vec<f64> {
    let count = lines.iter().copied().max().map_or(0, |last| last + 1);
    let mut half = vec![0.0_f64; count];
    for (item, &line) in items.iter().zip(lines) {
        let slack = if item.is_node { 0.0 } else { LATTICE_STEP / 2.0 };
        half[line] = half[line].max(size(item) / 2.0 + slack);
    }
    let mut centers = Vec::with_capacity(count);
    let mut previous: Option<(f64, f64)> = None;
    for &line_half in &half {
        let center = match previous {
            None => 0.0,
            Some((center, previous_half)) => center + snap_up(previous_half + line_half),
        };
        centers.push(center);
        previous = Some((center, line_half));
    }
    centers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(index: usize, x: f64, y: f64) -> LatticeItem {
        LatticeItem {
            block: Block::node_in_cell(index, PlanPoint::default(), (2.5, 2.5), false),
            position: PlanPoint::new(x, y),
            is_node: true,
        }
    }

    fn centers(block: &Block) -> Vec<(usize, f64, f64)> {
        let mut result: Vec<(usize, f64, f64)> = block.nodes.iter().map(|(index, at)| (*index, at.x, at.y)).collect();
        result.sort_by_key(|entry| entry.0);
        result
    }

    #[test]
    fn snapping_rounds_to_half_tiles() {
        assert_eq!((snap(1.24), snap(1.26), snap_up(2.4), snap_up(2.5)), (1.0, 1.5, 2.5, 2.5));
    }

    #[test]
    fn nearby_positions_share_lines_and_lines_pack_at_one_cell() {
        let items = vec![node(0, 0.1, 0.0), node(1, 3.9, 0.3), node(2, 4.2, 2.9)];
        let block = lattice(items, Arrange::Auto, (2.5, 2.5));
        assert_eq!(centers(&block), vec![(0, 0.0, 0.0), (1, 2.5, 0.0), (2, 2.5, 2.5)]);
    }

    #[test]
    fn a_clash_moves_to_the_nearest_free_cell() {
        let items = vec![node(0, 0.0, 0.0), node(1, 0.2, 0.1), node(2, 2.5, 0.0)];
        let block = lattice(items, Arrange::Auto, (2.5, 2.5));
        let placed = centers(&block);
        let unique: std::collections::BTreeSet<(i64, i64)> =
            placed.iter().map(|(_, x, y)| ((x * 2.0) as i64, (y * 2.0) as i64)).collect();
        assert_eq!(unique.len(), 3, "{placed:?}");
    }

    #[test]
    fn row_and_column_follow_the_relaxed_order() {
        let row = lattice(vec![node(0, 5.0, 3.0), node(1, 1.0, 0.0)], Arrange::Row, (2.5, 2.5));
        assert_eq!(centers(&row), vec![(0, 2.5, 0.0), (1, 0.0, 0.0)]);
        let column = lattice(vec![node(0, 5.0, 3.0), node(1, 1.0, 0.0)], Arrange::Column, (2.5, 2.5));
        assert_eq!(centers(&column), vec![(0, 0.0, 2.5), (1, 0.0, 0.0)]);
    }
}
