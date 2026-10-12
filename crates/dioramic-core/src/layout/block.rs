//! Blocks: laid-out pieces of the plan (a node, a zone with its content, a group) and the primitives that
//! compose them (lanes, grids, placement beside pinned items).
//!
//! Everything here works in the *flow frame*: the flow runs along plan `x` and siblings stack along `y`.
//! The caller transposes when `layout.flow` is `y`.

use crate::geometry::{PlanPoint, PlanRect, PlanVector};

/// A laid-out piece of the plan.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Block {
    /// Room reserved against siblings: a node's pitch cell, a zone's rectangle plus half the zone gap.
    pub extent: PlanRect,
    /// What a parent zone wraps with its padding: node centers and zone rectangles.
    pub core: PlanRect,
    /// Placed nodes: `(model index, center)`.
    pub nodes: Vec<(usize, PlanPoint)>,
    /// Placed zones: `(model index, rectangle)`. When `own_zone` is set, the first one is this block's zone.
    pub zones: Vec<(usize, PlanRect)>,
    /// The first entry of `zones` is the zone this block represents (it grows when the block is stretched).
    pub own_zone: bool,
    /// The block may grow to fill a grid cell (an auto-sized, unpinned zone).
    pub stretchable: bool,
    /// Its coordinates are absolute because of an `at` pin somewhere inside.
    pub anchored: bool,
}

/// Where a floating block may go relative to pinned items, in the flow frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Side {
    /// After them along the flow.
    After,
    /// Past them across the flow.
    Below,
    /// Before them along the flow.
    Before,
    /// Before them across the flow.
    Above,
}

impl Side {
    /// All sides, in order of preference.
    pub const ALL: [Side; 4] = [Side::After, Side::Below, Side::Before, Side::Above];
    /// Sides that keep the block right/below of a pinned top-left corner.
    pub const FORWARD: [Side; 2] = [Side::After, Side::Below];
}

/// A zone grows to line up with its neighbours only while it stays close to its natural size; past this
/// ratio it would become a large empty floor around a few nodes, so it keeps its size and is centered.
const MAX_STRETCH_RATIO: f64 = 1.75;

fn stretch_amount(current: f64, target: f64) -> f64 {
    if target <= current || target > current * MAX_STRETCH_RATIO { 0.0 } else { target - current }
}

fn point_rect(point: PlanPoint) -> PlanRect {
    PlanRect::new(point.x, point.y, point.x, point.y)
}

fn translate_rect(rect: PlanRect, by: PlanVector) -> PlanRect {
    PlanRect::new(rect.x0 + by.x, rect.y0 + by.y, rect.x1 + by.x, rect.y1 + by.y)
}

/// Grows `rect` symmetrically by `extra_width` along x and `extra_depth` along y.
fn grow(rect: PlanRect, extra_width: f64, extra_depth: f64) -> PlanRect {
    PlanRect::new(
        rect.x0 - extra_width / 2.0,
        rect.y0 - extra_depth / 2.0,
        rect.x1 + extra_width / 2.0,
        rect.y1 + extra_depth / 2.0,
    )
}

impl Block {
    /// A group with nothing in it (extent and core collapse on the origin).
    pub fn empty() -> Self {
        let origin = point_rect(PlanPoint::default());
        Self {
            extent: origin,
            core: origin,
            nodes: Vec::new(),
            zones: Vec::new(),
            own_zone: false,
            stretchable: false,
            anchored: false,
        }
    }

    /// A single node centered on `center`, reserving a cell of `(along, across)` the flow.
    pub fn node_in_cell(index: usize, center: PlanPoint, (along, across): (f64, f64), anchored: bool) -> Self {
        Self {
            extent: PlanRect::around(center, along / 2.0, across / 2.0),
            core: point_rect(center),
            nodes: vec![(index, center)],
            anchored,
            ..Self::empty()
        }
    }

    /// True when the block holds no node and no zone.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.zones.is_empty()
    }

    /// Moves everything by `by`.
    pub fn translate(&mut self, by: PlanVector) {
        self.extent = translate_rect(self.extent, by);
        self.core = translate_rect(self.core, by);
        for (_, center) in &mut self.nodes {
            *center = *center + by;
        }
        for (_, rect) in &mut self.zones {
            *rect = translate_rect(*rect, by);
        }
    }

    /// Moves the block so its extent is centered on `center`.
    pub fn center_on(&mut self, center: PlanPoint) {
        self.translate(center - self.extent.center());
    }

    /// Grows a stretchable zone (symmetrically, content stays centered) until its extent is at least
    /// `width` × `depth`, axis by axis, within [`MAX_STRETCH_RATIO`]. Other blocks are left untouched.
    pub fn stretch_to(&mut self, width: f64, depth: f64) {
        if !self.stretchable || !self.own_zone {
            return;
        }
        let extra_width = stretch_amount(self.extent.width(), width);
        let extra_depth = stretch_amount(self.extent.depth(), depth);
        self.extent = grow(self.extent, extra_width, extra_depth);
        self.core = grow(self.core, extra_width, extra_depth);
        if let Some((_, rect)) = self.zones.first_mut() {
            *rect = grow(*rect, extra_width, extra_depth);
        }
    }

    /// Adds `other` to this group: unions the rectangles, appends nodes and zones.
    pub fn absorb(&mut self, other: Block) {
        if self.is_empty() {
            self.extent = other.extent;
            self.core = other.core;
        } else if !other.is_empty() {
            self.extent = self.extent.union(other.extent);
            self.core = self.core.union(other.core);
        }
        self.anchored |= other.anchored;
        self.nodes.extend(other.nodes);
        self.zones.extend(other.zones);
    }
}

/// Lanes along the flow, each lane a stack across the flow. Lanes touch (the extents already carry the
/// spacing); each stack is centered on `y = 0`, and zones in a lane are stretched to the lane width.
pub(super) fn lanes(lanes: Vec<Vec<Block>>) -> Block {
    let mut group = Block::empty();
    let mut lane_start = 0.0;
    for lane in lanes {
        let width = lane.iter().map(|block| block.extent.width()).fold(0.0, f64::max);
        let stack_depth: f64 = lane.iter().map(|block| block.extent.depth()).sum();
        let mut cursor = -stack_depth / 2.0;
        for mut block in lane {
            let depth = block.extent.depth();
            block.stretch_to(width, depth);
            block.center_on(PlanPoint::new(lane_start + width / 2.0, cursor + depth / 2.0));
            cursor += depth;
            group.absorb(block);
        }
        lane_start += width;
    }
    group
}

/// An aligned table: columns along the flow, rows across it. Column widths and row depths are the largest
/// extents in them; blocks are centered in their cell and stretchable zones fill it. `None` leaves a cell empty.
pub(super) fn grid(rows: Vec<Vec<Option<Block>>>) -> Block {
    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = vec![0.0_f64; column_count];
    let mut depths = vec![0.0_f64; rows.len()];
    for (row, cells) in rows.iter().enumerate() {
        for (column, block) in cells.iter().enumerate() {
            if let Some(block) = block {
                widths[column] = widths[column].max(block.extent.width());
                depths[row] = depths[row].max(block.extent.depth());
            }
        }
    }
    let column_starts = running_starts(&widths);
    let row_starts = running_starts(&depths);
    let mut group = Block::empty();
    for (row, cells) in rows.into_iter().enumerate() {
        for (column, block) in cells.into_iter().enumerate() {
            let Some(mut block) = block else { continue };
            block.stretch_to(widths[column], depths[row]);
            block.center_on(PlanPoint::new(
                column_starts[column] + widths[column] / 2.0,
                row_starts[row] + depths[row] / 2.0,
            ));
            group.absorb(block);
        }
    }
    group
}

/// Rows across the flow, each packed along the flow at its own widths (no shared columns, so a narrow zone
/// does not leave a hole the width of a wide one). Zones are stretched to the depth of their row. Rows follow
/// a serpentine flow: an odd row ends where the previous one ends, an even row starts where it starts.
/// `None` cells are skipped.
pub(super) fn shelves(rows: Vec<Vec<Option<Block>>>) -> Block {
    let mut group = Block::empty();
    let mut row_start = 0.0;
    let mut previous: Option<PlanRect> = None;
    for (index, cells) in rows.into_iter().enumerate() {
        let blocks: Vec<Block> = cells.into_iter().flatten().collect();
        let depth = blocks.iter().map(|block| block.extent.depth()).fold(0.0, f64::max);
        let mut row = lanes(
            blocks
                .into_iter()
                .map(|mut block| {
                    block.stretch_to(block.extent.width(), depth);
                    vec![block]
                })
                .collect(),
        );
        if row.is_empty() {
            continue;
        }
        let x0 = match previous {
            None => 0.0,
            Some(previous) if index % 2 == 1 => previous.x1 - row.extent.width(),
            Some(previous) => previous.x0,
        };
        row.translate(PlanVector::new(x0 - row.extent.x0, row_start - row.extent.y0));
        row_start += row.extent.depth();
        previous = Some(row.extent);
        group.absorb(row);
    }
    group
}

fn running_starts(sizes: &[f64]) -> Vec<f64> {
    sizes
        .iter()
        .scan(0.0, |start, size| {
            let current = *start;
            *start += size;
            Some(current)
        })
        .collect()
}

/// Translation that puts `floating` next to `obstacles` (touching, never overlapping) on one of `sides`,
/// choosing the side with the smallest combined bounding area (ties keep the order of `sides`).
pub(super) fn beside(floating: PlanRect, obstacles: PlanRect, sides: &[Side]) -> PlanVector {
    let target_center = obstacles.center();
    let candidates = sides.iter().map(|side| {
        let origin = match side {
            Side::After => PlanPoint::new(obstacles.x1, obstacles.y0),
            Side::Below => PlanPoint::new(obstacles.x0, obstacles.y1),
            Side::Before => PlanPoint::new(obstacles.x0 - floating.width(), obstacles.y0),
            Side::Above => PlanPoint::new(obstacles.x0, obstacles.y0 - floating.depth()),
        };
        let shift = origin - PlanPoint::new(floating.x0, floating.y0);
        let placed = translate_rect(floating, shift);
        let union = placed.union(obstacles);
        let area = union.width() * union.depth();
        // Prefer compact unions, then blocks whose center is closest to the pinned items.
        (area, placed.center().distance(target_center), shift)
    });
    candidates
        .fold(None, |best: Option<(f64, f64, PlanVector)>, candidate| match best {
            Some(best) if (best.0, best.1) <= (candidate.0, candidate.1) => Some(best),
            _ => Some(candidate),
        })
        .map_or(PlanVector::default(), |best| best.2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn zone_block(index: usize, width: f64, depth: f64) -> Block {
        let rect = PlanRect::new(0.0, 0.0, width, depth);
        Block {
            extent: rect,
            core: rect,
            zones: vec![(index, rect)],
            own_zone: true,
            stretchable: true,
            ..Block::empty()
        }
    }

    #[test]
    fn lanes_keep_pitch_between_node_centers() {
        let pitch = 2.0;
        let node = |index| Block::node_in_cell(index, PlanPoint::default(), (pitch, pitch), false);
        let group = lanes(vec![vec![node(0)], vec![node(1), node(2)]]);
        let centers: Vec<PlanPoint> = group.nodes.iter().map(|(_, center)| *center).collect();
        assert!(close(centers[0].x, 1.0) && close(centers[0].y, 0.0));
        assert!(close(centers[1].x, 3.0) && close(centers[1].y, -1.0));
        assert!(close(centers[2].x, 3.0) && close(centers[2].y, 1.0));
    }

    #[test]
    fn grid_stretches_zones_to_their_cell() {
        let group = grid(vec![vec![Some(zone_block(0, 2.0, 3.0)), Some(zone_block(1, 3.0, 5.0))]]);
        let first = group.zones[0].1;
        assert!(close(first.depth(), 5.0) && close(first.width(), 2.0));
        assert!(close(group.zones[1].1.x0, 2.0));
    }

    #[test]
    fn stretch_keeps_content_centered() {
        let mut block = zone_block(0, 2.0, 2.0);
        block.nodes.push((7, PlanPoint::new(1.0, 1.0)));
        block.stretch_to(3.0, 2.0);
        assert!(close(block.zones[0].1.center().x, 1.0));
        assert!(close(block.zones[0].1.width(), 3.0));
    }

    #[test]
    fn stretch_stops_where_the_zone_would_turn_into_empty_floor() {
        let mut block = zone_block(0, 2.0, 2.0);
        block.stretch_to(3.0, 9.0);
        assert!(close(block.extent.width(), 3.0) && close(block.extent.depth(), 2.0));
    }

    #[test]
    fn unstretchable_blocks_keep_their_size() {
        let mut block = zone_block(0, 2.0, 2.0);
        block.stretchable = false;
        block.stretch_to(9.0, 9.0);
        assert!(close(block.extent.width(), 2.0));
    }

    #[test]
    fn shelves_pack_rows_independently_and_turn_at_the_row_end() {
        let rows = vec![
            vec![Some(zone_block(0, 2.0, 1.0)), Some(zone_block(1, 2.0, 1.0)), Some(zone_block(2, 1.0, 1.0))],
            vec![None, Some(zone_block(3, 1.0, 2.0)), Some(zone_block(4, 3.0, 2.0))],
        ];
        let group = shelves(rows);
        let rect = |index: usize| group.zones.iter().find(|(zone, _)| *zone == index).map(|(_, rect)| *rect);
        let (Some(last_of_first), Some(first_of_second), Some(second_of_second)) = (rect(2), rect(4), rect(3)) else {
            panic!("missing zones");
        };
        assert!(close(first_of_second.x1, last_of_first.x1), "the second row turns where the first one ends");
        assert!(close(second_of_second.x1, first_of_second.x0), "cells of a row touch, without a shared column");
        assert!(close(first_of_second.y0, 1.0) && close(group.extent.width(), 5.0));
    }

    #[test]
    fn beside_never_overlaps_the_obstacles() {
        let obstacles = PlanRect::new(10.0, 10.0, 14.0, 12.0);
        let floating = PlanRect::new(0.0, 0.0, 3.0, 2.0);
        for sides in [&Side::ALL[..], &Side::FORWARD[..], &[Side::Above][..], &[Side::Before][..]] {
            let shift = beside(floating, obstacles, sides);
            let placed = translate_rect(floating, shift);
            let overlap_x = placed.x0 < obstacles.x1 - 1e-9 && obstacles.x0 < placed.x1 - 1e-9;
            let overlap_y = placed.y0 < obstacles.y1 - 1e-9 && obstacles.y0 < placed.y1 - 1e-9;
            assert!(!(overlap_x && overlap_y), "{sides:?} overlapped: {placed:?}");
        }
    }

    #[test]
    fn beside_prefers_the_flow_side_on_ties() {
        let obstacles = PlanRect::new(0.0, 0.0, 2.0, 2.0);
        let shift = beside(PlanRect::new(0.0, 0.0, 2.0, 2.0), obstacles, &Side::ALL);
        assert!(close(shift.x, 2.0) && close(shift.y, 0.0));
    }
}
