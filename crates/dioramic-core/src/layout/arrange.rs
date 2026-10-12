//! Arrangement of the zone tree: nodes inside zones (`arrange`), nested zones inside their parent,
//! top-level zones and zone-less nodes in rows (`density`, `wrap`), and `at` / `size` pins.
//!
//! Works in the flow frame (see [`super::block`]); [`FlowFrame`] converts spec coordinates in and out.

use std::collections::BTreeMap;

use dioramic_spec::{Arrange, Density, FlowAxis, Model, NodeId, Wrap};

use super::block::{Block, Side, beside, grid, lanes, shelves};
use super::ranks::FlowRanks;
use crate::geometry::{COS_30, PlanPoint, PlanRect, SIN_30};

/// A rank group taller than this is folded into several lanes (keeps a 5-way fan-out from becoming a wall).
const MAX_LANE_STACK: usize = 4;
/// Edges at a node from which it counts as a hub for [`Spacing::pill_run`].
const HUB_DEGREE: usize = 6;

/// `spaced` keeps the zones in one row while the picture is at most this wide (tiles): at the final size its
/// 14 px labels stay ≥ 1% of the width (the demo scenes). Wider rows become a long, sparse diagonal and are
/// folded (see [`best_fold`]).
const LEGIBLE_PICTURE_WIDTH: f64 = 25.0;

/// Aspect of the frame a folded picture is fitted into (16:10, a slide or a README column): a fold that is
/// too tall for it is shrunk to its height, which makes its text as small as a too-wide one.
const FRAME_ASPECT: f64 = 1.6;

/// Width the picture adds around the floor (tiles): the canvas margins.
const PICTURE_SIDE_ROOM: f64 = 2.1;
/// Height the picture adds above the floor (tiles): title, icon bodies and label towers.
const PICTURE_HEAD_ROOM: f64 = 3.7;

/// Half side of the floor a node takes in the picture estimate (tiles).
const NODE_PICTURE_HALF: f64 = 0.6;

/// Host key of zone-less nodes that stay on their own in the rows (no zone index reaches it).
const NO_HOST: usize = usize::MAX;

/// Folds whose fitted widths differ by less than this (tiles) count as a tie.
const FIT_TIE_TOLERANCE: f64 = 0.25;

/// Swaps plan axes when the flow runs along `y`, so the arrangement code only knows "along x".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FlowFrame {
    transposed: bool,
}

impl FlowFrame {
    /// Frame for a flow axis.
    pub fn new(flow: FlowAxis) -> Self {
        Self { transposed: flow == FlowAxis::Y }
    }

    /// Spec/plan point ↔ flow-frame point (the mapping is its own inverse).
    pub fn point(self, point: PlanPoint) -> PlanPoint {
        if self.transposed { PlanPoint::new(point.y, point.x) } else { point }
    }

    /// Spec/plan rectangle ↔ flow-frame rectangle.
    pub fn rect(self, rect: PlanRect) -> PlanRect {
        if self.transposed { PlanRect::new(rect.y0, rect.x0, rect.y1, rect.x1) } else { rect }
    }

    /// Spec `size: [w, d]` ↔ flow-frame `(along, across)`.
    pub fn size(self, size: [f64; 2]) -> (f64, f64) {
        if self.transposed { (size[1], size[0]) } else { (size[0], size[1]) }
    }
}

/// Spacing derived from the density and the footprints.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Spacing {
    /// Distance between neighbouring node centers along the flow (where most edges run).
    pub pitch_along: f64,
    /// Distance between neighbouring node centers across the flow (stacked siblings).
    pub pitch_across: f64,
    /// Smallest inner zone padding around node centers and nested zones.
    pub padding: f64,
    /// Free floor kept between a footprint and the border of its zone (the zone label lies there):
    /// a zone holding a larger icon gets a larger padding.
    pub border_room: f64,
    /// Half the gap between sibling zones (each zone reserves it on every side).
    pub zone_margin: f64,
    /// Extra room along the flow for a node joined to a zone-mate by a labelled edge, so the pill of that
    /// short edge has a stretch of its own beside the stroke instead of landing on a neighbour's label.
    pub pill_run: f64,
}

impl Spacing {
    /// The `(along, across)` cell a node reserves.
    pub fn cell(&self) -> (f64, f64) {
        (self.pitch_along, self.pitch_across)
    }

    /// Inner padding of a zone whose largest direct footprint has half extent `largest_half`.
    pub fn zone_padding(&self, largest_half: f64) -> f64 {
        self.padding.max(largest_half + self.border_room)
    }
}

/// Sort key of an item along the flow and across it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ItemKey {
    /// Earliest slot of the nodes it holds (`None` for an empty zone: it goes last).
    slot: Option<u32>,
    /// Zone-less nodes at the top level come after the zones of their slot, right before what they feed.
    trails_slot: bool,
    /// Mean slot (tie-break along the flow).
    mean_slot: f64,
    /// Mean across hint (order inside a stack).
    across: f64,
    /// Spec order (final tie-break).
    order: usize,
}

impl ItemKey {
    fn flow_order(&self, other: &Self) -> std::cmp::Ordering {
        let slot = |key: &Self| key.slot.unwrap_or(u32::MAX);
        slot(self)
            .cmp(&slot(other))
            .then(self.trails_slot.cmp(&other.trails_slot))
            .then(self.mean_slot.total_cmp(&other.mean_slot))
            .then(self.across.total_cmp(&other.across))
            .then(self.order.cmp(&other.order))
    }

    fn stack_order(&self, other: &Self) -> std::cmp::Ordering {
        self.across.total_cmp(&other.across).then(self.order.cmp(&other.order))
    }
}

struct Item {
    key: ItemKey,
    block: Block,
}

/// A top-level cell of the rows: a zone (or a zone-less group) and the zone-less nodes it hosts.
#[derive(Clone)]
struct RowCell {
    block: Block,
    guests: Option<Block>,
}

impl RowCell {
    /// Puts the guests across the flow from the zone: behind it, or in front of it when `guests_in_front`.
    fn compose(self, guests_in_front: bool) -> Block {
        match self.guests {
            None => self.block,
            Some(guests) if guests_in_front => lanes(vec![vec![self.block, guests]]),
            Some(guests) => lanes(vec![vec![guests, self.block]]),
        }
    }
}

/// Pins of the model, already in the flow frame.
struct Pins {
    node_at: Vec<Option<PlanPoint>>,
    zone_at: Vec<Option<PlanPoint>>,
    zone_size: Vec<Option<(f64, f64)>>,
}

/// The zone tree and everything needed to lay it out.
pub(super) struct Arranger<'a> {
    model: &'a Model,
    ranks: &'a FlowRanks,
    spacing: Spacing,
    /// Largest half extent of each node footprint, indexed like `model.nodes`.
    footprint_half: Vec<f64>,
    pins: Pins,
    /// Valid parent of each zone (unknown or cyclic parents make it top-level).
    zone_parent: Vec<Option<usize>>,
    zone_children: Vec<Vec<usize>>,
    zone_nodes: Vec<Vec<usize>>,
    free_nodes: Vec<usize>,
}

/// Result of the arrangement, in the flow frame.
pub(super) struct Arrangement {
    /// Node centers indexed like `model.nodes`.
    pub node_centers: Vec<PlanPoint>,
    /// Zone rectangles indexed like `model.zones`.
    pub zone_rects: Vec<PlanRect>,
    /// Valid parent of each zone.
    pub zone_parent: Vec<Option<usize>>,
    /// Whether anything is pinned (then coordinates are absolute and must not be normalized).
    pub anchored: bool,
}

impl<'a> Arranger<'a> {
    /// Indexes the zone tree of `model`.
    pub fn new(
        model: &'a Model,
        ranks: &'a FlowRanks,
        spacing: Spacing,
        footprint_half: Vec<f64>,
        frame: FlowFrame,
    ) -> Self {
        let zone_index: BTreeMap<&str, usize> =
            model.zones.iter().enumerate().map(|(index, zone)| (zone.id.as_str(), index)).collect();
        let zone_parent = valid_parents(model, &zone_index);
        let mut zone_children = vec![Vec::new(); model.zones.len()];
        for (zone, parent) in zone_parent.iter().enumerate() {
            if let Some(parent) = parent {
                zone_children[*parent].push(zone);
            }
        }
        let mut zone_nodes = vec![Vec::new(); model.zones.len()];
        let mut free_nodes = Vec::new();
        for (index, node) in model.nodes.iter().enumerate() {
            match node.zone.as_ref().and_then(|zone| zone_index.get(zone.as_str())) {
                Some(&zone) => zone_nodes[zone].push(index),
                None => free_nodes.push(index),
            }
        }
        let pins = Pins {
            node_at: model.nodes.iter().map(|node| node.at.map(|at| frame.point(PlanPoint::new(at.x, at.y)))).collect(),
            zone_at: model.zones.iter().map(|zone| zone.at.map(|at| frame.point(PlanPoint::new(at.x, at.y)))).collect(),
            zone_size: model.zones.iter().map(|zone| zone.size.map(|size| frame.size(size))).collect(),
        };
        Self { model, ranks, spacing, footprint_half, pins, zone_parent, zone_children, zone_nodes, free_nodes }
    }

    /// Lays out the whole plan.
    pub fn arrange(&self, density: Density) -> Arrangement {
        let root = self.root_block(density);
        let anchored = root.anchored;
        let mut node_centers = vec![PlanPoint::default(); self.model.nodes.len()];
        for (index, center) in root.nodes {
            node_centers[index] = center;
        }
        let mut zone_rects = vec![PlanRect::new(0.0, 0.0, 0.0, 0.0); self.model.zones.len()];
        for (index, rect) in root.zones {
            zone_rects[index] = rect;
        }
        Arrangement { node_centers, zone_rects, zone_parent: self.zone_parent.clone(), anchored }
    }

    /// Top level: unpinned zones and zone-less node groups in serpentine rows, pinned ones where they say.
    fn root_block(&self, density: Density) -> Block {
        let mut floating = Vec::new();
        let mut anchored = Vec::new();
        let mut hosted = self.host_zones();
        for zone in self.top_level_zones() {
            let block = self.zone_block(zone);
            let guests = hosted.remove(&zone).unwrap_or_default();
            if block.anchored {
                anchored.push(block);
                hosted.entry(NO_HOST).or_default().extend(guests);
            } else {
                floating.push((self.zone_key(zone), RowCell { block, guests: self.guest_block(&guests) }));
            }
        }
        let mut free_by_slot: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        for node in hosted.into_values().flatten() {
            match self.pins.node_at[node] {
                Some(at) => anchored.push(Block::node_in_cell(node, at, self.spacing.cell(), true)),
                None => free_by_slot.entry(self.ranks.slot[node]).or_default().push(node),
            }
        }
        for nodes in free_by_slot.into_values() {
            let group = self.free_group(&nodes);
            floating.push((group.key, RowCell { block: group.block, guests: None }));
        }
        floating.sort_by(|a, b| a.0.flow_order(&b.0));
        let content = self.fold_rows(density, floating.into_iter().map(|(_, cell)| cell).collect());
        place_around_pins(content, anchored, &Side::ALL)
    }

    /// Zone-less, unpinned nodes grouped by the top-level zone they have the most edges with ([`NO_HOST`]
    /// for pinned nodes and nodes linked to no zone). Ties go to the zone declared first.
    fn host_zones(&self) -> BTreeMap<usize, Vec<usize>> {
        let mut hosted: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for &node in &self.free_nodes {
            let host = if self.pins.node_at[node].is_some() { NO_HOST } else { self.busiest_zone(node) };
            hosted.entry(host).or_default().push(node);
        }
        hosted
    }

    /// Top-level zone with the most edges to `node` ([`NO_HOST`] when it has none).
    fn busiest_zone(&self, node: usize) -> usize {
        let id = &self.model.nodes[node].id;
        let mut counts: BTreeMap<usize, usize> = BTreeMap::new();
        for edge in &self.model.edges {
            let other = if &edge.from == id {
                &edge.to
            } else if &edge.to == id {
                &edge.from
            } else {
                continue;
            };
            if let Some(zone) = self.node_index(other.as_str()).and_then(|other| self.top_level_zone_of(other)) {
                *counts.entry(zone).or_default() += 1;
            }
        }
        counts
            .into_iter()
            .fold(None, |best: Option<(usize, usize)>, (zone, count)| match best {
                Some((_, best_count)) if best_count >= count => best,
                _ => Some((zone, count)),
            })
            .map_or(NO_HOST, |(zone, _)| zone)
    }

    fn node_index(&self, id: &str) -> Option<usize> {
        self.model.nodes.iter().position(|node| node.id.as_str() == id)
    }

    /// Top-level ancestor of the zone holding `node` (`None` for a zone-less node).
    fn top_level_zone_of(&self, node: usize) -> Option<usize> {
        let mut zone = self.zone_nodes.iter().position(|nodes| nodes.contains(&node))?;
        while let Some(parent) = self.zone_parent[zone] {
            zone = parent;
        }
        Some(zone)
    }

    /// The zone-less nodes hosted by a zone, in one row along the flow. They will sit beside the zone,
    /// across the flow, instead of on the line between two zones where the flow edges would cross them.
    fn guest_block(&self, guests: &[usize]) -> Option<Block> {
        if guests.is_empty() {
            return None;
        }
        let mut items: Vec<Item> = guests.iter().map(|&node| self.node_item(node)).collect();
        items.sort_by(|a, b| a.key.flow_order(&b.key));
        Some(lanes(items.into_iter().map(|item| vec![item.block]).collect()))
    }

    /// Folds the top-level items in serpentine rows: `layout.wrap: N` as given, `compact` ⌈√n⌉ per row,
    /// `spaced` one row while it is legible (see [`LEGIBLE_PICTURE_WIDTH`]), else the best fold.
    fn fold_rows(&self, density: Density, cells: Vec<RowCell>) -> Block {
        let count = cells.len();
        match (self.model.layout.wrap, density) {
            (Wrap::Count(per_row), _) => fold(cells, usize::try_from(per_row).unwrap_or(usize::MAX), Packing::Grid),
            (Wrap::Auto, Density::Compact) => fold(cells, ceil_sqrt(count), Packing::Grid),
            (Wrap::Auto, Density::Spaced) => {
                let row = fold(cells.clone(), count, Packing::Grid);
                if picture_size(&row).0 <= LEGIBLE_PICTURE_WIDTH { row } else { best_fold(cells) }
            }
        }
    }

    /// Zone-less nodes of the same slot share one stack, ordered across like the rest of the flow.
    fn free_group(&self, nodes: &[usize]) -> Item {
        let mut items: Vec<Item> = nodes.iter().map(|&node| self.node_item(node)).collect();
        items.sort_by(|a, b| a.key.stack_order(&b.key));
        let key = ItemKey {
            trails_slot: true,
            across: mean(items.iter().map(|item| item.key.across)),
            order: items.iter().map(|item| item.key.order).min().unwrap_or(usize::MAX),
            ..items.first().map_or(self.empty_key(0), |item| item.key)
        };
        Item { key, block: lanes(vec![items.into_iter().map(|item| item.block).collect()]) }
    }

    fn top_level_zones(&self) -> impl Iterator<Item = usize> + '_ {
        self.zone_parent.iter().enumerate().filter(|(_, parent)| parent.is_none()).map(|(zone, _)| zone)
    }

    fn node_item(&self, node: usize) -> Item {
        let pin = self.pins.node_at[node];
        let block = Block::node_in_cell(node, pin.unwrap_or_default(), self.node_cell(node), pin.is_some());
        let slot = self.ranks.slot[node];
        let key = ItemKey {
            slot: Some(slot),
            trails_slot: false,
            mean_slot: f64::from(slot),
            across: self.ranks.across[node],
            order: node,
        };
        Item { key, block }
    }

    /// The `(along, across)` cell of a node: the density's, longer along the flow when one of its labelled
    /// edges stays inside its zone.
    fn node_cell(&self, node: usize) -> (f64, f64) {
        let (along, across) = self.spacing.cell();
        if self.has_labelled_edge_in_zone(node) { (along + self.spacing.pill_run, across) } else { (along, across) }
    }

    /// True when `node` has a labelled edge to another node of the same zone and one of the two is a hub
    /// ([`HUB_DEGREE`] edges or more): around a hub the short edge competes with many strokes and labels.
    fn has_labelled_edge_in_zone(&self, node: usize) -> bool {
        let Some(zone) = self.model.nodes[node].zone.as_ref() else { return false };
        let id = &self.model.nodes[node].id;
        let degree = |of: &NodeId| self.model.edges.iter().filter(|edge| &edge.from == of || &edge.to == of).count();
        self.model.edges.iter().filter(|edge| edge.label.as_deref().is_some_and(|label| !label.trim().is_empty())).any(
            |edge| {
                let mate = if &edge.from == id {
                    &edge.to
                } else if &edge.to == id {
                    &edge.from
                } else {
                    return false;
                };
                mate != id
                    && self
                        .node_index(mate.as_str())
                        .is_some_and(|other| self.model.nodes[other].zone.as_ref() == Some(zone))
                    && degree(id).max(degree(mate)) >= HUB_DEGREE
            },
        )
    }

    /// A zone with its nodes and nested zones, wrapped in its rectangle.
    fn zone_block(&self, zone: usize) -> Block {
        let children = self.zone_nodes[zone].iter().map(|&node| self.node_item(node)).chain(
            self.zone_children[zone]
                .iter()
                .map(|&child| Item { key: self.zone_key(child), block: self.zone_block(child) }),
        );
        let (anchored, floating): (Vec<Item>, Vec<Item>) = children.partition(|item| item.block.anchored);
        let content = arrange_items(floating, self.model.zones[zone].arrange);
        let anchored: Vec<Block> = anchored.into_iter().map(|item| item.block).collect();
        self.enclose(zone, content, anchored)
    }

    /// Wraps the content in the zone rectangle, honoring `at` (top-left corner) and `size` exactly.
    fn enclose(&self, zone: usize, content: Block, anchored: Vec<Block>) -> Block {
        let at = self.pins.zone_at[zone];
        let size = self.pins.zone_size[zone];
        let padding = self.zone_padding(zone);
        let has_pins = at.is_some() || !anchored.is_empty();
        let (content, rect) = if anchored.is_empty() {
            let mut content = content;
            let natural = content.core.inflate(padding);
            let rect = match (at, size) {
                (Some(at), Some((width, depth))) => PlanRect::new(at.x, at.y, at.x + width, at.y + depth),
                (Some(at), None) => PlanRect::new(at.x, at.y, at.x + natural.width(), at.y + natural.depth()),
                (None, Some((width, depth))) => PlanRect::around(natural.center(), width / 2.0, depth / 2.0),
                (None, None) => natural,
            };
            content.translate(rect.center() - content.core.center());
            (content, rect)
        } else {
            let sides: &[Side] = if at.is_some() { &Side::FORWARD } else { &Side::ALL };
            let content = place_around_pins(content, anchored, sides);
            let natural = content.core.inflate(padding);
            let rect = match (at, size) {
                (Some(at), Some((width, depth))) => PlanRect::new(at.x, at.y, at.x + width, at.y + depth),
                (Some(at), None) => PlanRect::new(
                    at.x,
                    at.y,
                    natural.x1.max(at.x + 2.0 * padding),
                    natural.y1.max(at.y + 2.0 * padding),
                ),
                (None, Some((width, depth))) => {
                    PlanRect::new(natural.x0, natural.y0, natural.x0 + width, natural.y0 + depth)
                }
                (None, None) => natural,
            };
            (content, rect)
        };
        let mut zones = Vec::with_capacity(content.zones.len() + 1);
        zones.push((zone, rect));
        zones.extend(content.zones);
        Block {
            // An undersized `size` must still keep its overflowing content clear of the neighbours.
            extent: rect.inflate(self.spacing.zone_margin).union(content.extent),
            core: rect,
            nodes: content.nodes,
            zones,
            own_zone: true,
            stretchable: !has_pins && size.is_none(),
            anchored: has_pins,
        }
    }

    /// Padding of a zone, grown for the largest footprint placed directly in it.
    fn zone_padding(&self, zone: usize) -> f64 {
        let largest_half = self.zone_nodes[zone].iter().map(|&node| self.footprint_half[node]).fold(0.0, f64::max);
        self.spacing.zone_padding(largest_half)
    }

    fn zone_key(&self, zone: usize) -> ItemKey {
        let mut nodes = Vec::new();
        self.collect_subtree_nodes(zone, &mut nodes);
        if nodes.is_empty() {
            return self.empty_key(zone);
        }
        let slots = nodes.iter().map(|&node| self.ranks.slot[node]);
        ItemKey {
            slot: slots.clone().min(),
            trails_slot: false,
            mean_slot: mean(slots.map(f64::from)),
            across: mean(nodes.iter().map(|&node| self.ranks.across[node])),
            order: nodes.iter().copied().min().unwrap_or(usize::MAX),
        }
    }

    /// Empty zones go after everything else, in spec order.
    fn empty_key(&self, zone: usize) -> ItemKey {
        ItemKey { slot: None, trails_slot: false, mean_slot: 0.0, across: 0.0, order: self.model.nodes.len() + zone }
    }

    fn collect_subtree_nodes(&self, zone: usize, into: &mut Vec<usize>) {
        into.extend(&self.zone_nodes[zone]);
        for &child in &self.zone_children[zone] {
            self.collect_subtree_nodes(child, into);
        }
    }
}

/// Parent index of each zone; a missing parent, or a parent chain that loops, makes the zone top-level.
pub(super) fn valid_parents(model: &Model, zone_index: &BTreeMap<&str, usize>) -> Vec<Option<usize>> {
    let declared: Vec<Option<usize>> = model
        .zones
        .iter()
        .map(|zone| zone.parent.as_ref().and_then(|parent| zone_index.get(parent.as_str()).copied()))
        .collect();
    (0..declared.len())
        .map(|zone| {
            let mut current = declared[zone];
            for _ in 0..declared.len() {
                match current {
                    None => return declared[zone],
                    Some(ancestor) if ancestor == zone => return None,
                    Some(ancestor) => current = declared[ancestor],
                }
            }
            None
        })
        .collect()
}

/// Places the floating content next to the pinned blocks (if any) and merges them.
fn place_around_pins(mut content: Block, anchored: Vec<Block>, sides: &[Side]) -> Block {
    let Some(obstacles) = anchored.iter().map(|block| block.extent).reduce(PlanRect::union) else {
        return content;
    };
    if !content.is_empty() {
        content.translate(beside(content.extent, obstacles, sides));
    }
    for block in anchored {
        content.absorb(block);
    }
    content
}

/// Lays out the floating children of a zone according to `arrange`.
fn arrange_items(mut items: Vec<Item>, arrange: Arrange) -> Block {
    items.sort_by(|a, b| a.key.flow_order(&b.key));
    match arrange {
        Arrange::Auto => lanes(rank_lanes(items)),
        Arrange::Row => lanes(items.into_iter().map(|item| vec![item.block]).collect()),
        Arrange::Column => lanes(vec![items.into_iter().map(|item| item.block).collect()]),
        Arrange::Grid => {
            let columns = ceil_sqrt(items.len());
            grid(chunk_rows(items.into_iter().map(|item| Some(item.block)).collect(), columns))
        }
    }
}

/// `auto`: one lane per slot (siblings of the same slot stacked across the flow), tall stacks folded.
fn rank_lanes(items: Vec<Item>) -> Vec<Vec<Block>> {
    let mut groups: Vec<Vec<Item>> = Vec::new();
    for item in items {
        match groups.last_mut() {
            Some(group) if group[0].key.slot == item.key.slot && item.key.slot.is_some() => group.push(item),
            _ => groups.push(vec![item]),
        }
    }
    let mut result = Vec::new();
    for mut group in groups {
        group.sort_by(|a, b| a.key.stack_order(&b.key));
        let per_lane = if group.len() > MAX_LANE_STACK { ceil_sqrt(group.len()) } else { group.len() };
        let mut blocks = group.into_iter().map(|item| item.block).peekable();
        while blocks.peek().is_some() {
            result.push(blocks.by_ref().take(per_lane).collect());
        }
    }
    result
}

/// The serpentine fold that shows the text largest once fitted into a [`FRAME_ASPECT`] frame.
///
/// An isometric view of a long row is a sparse diagonal: wide, mostly empty, so its text shrinks when the image
/// is scaled to a page. Folding into rows fills the frame; too many rows make the picture too tall instead.
/// Ties keep the longer rows (fewer U-turns in the flow), then aligned columns.
fn best_fold(cells: Vec<RowCell>) -> Block {
    let count = cells.len();
    let mut best: Option<(f64, Block)> = None;
    for per_row in (ceil_sqrt(count)..=count).rev() {
        for packing in [Packing::Grid, Packing::Shelves] {
            let candidate = fold(cells.clone(), per_row, packing);
            let fitted = fitted_width(picture_size(&candidate));
            if best.as_ref().is_none_or(|(best_fitted, _)| fitted < best_fitted - FIT_TIE_TOLERANCE) {
                best = Some((fitted, candidate));
            }
        }
    }
    best.map_or_else(Block::empty, |(_, block)| block)
}

/// Width of the frame of [`FRAME_ASPECT`] that holds a picture of `(width, height)`.
fn fitted_width((width, height): (f64, f64)) -> f64 {
    width.max(FRAME_ASPECT * height)
}

/// Estimated `(width, height)` of the picture of a block, in tiles: the screen box of its zones and nodes
/// (an isometric plan point `(x, y)` lands at `((x − y)·cos 30°, (x + y)·sin 30°)`) plus the room around it.
fn picture_size(block: &Block) -> (f64, f64) {
    let corners = block
        .zones
        .iter()
        .map(|(_, rect)| *rect)
        .chain(block.nodes.iter().map(|(_, center)| PlanRect::around(*center, NODE_PICTURE_HALF, NODE_PICTURE_HALF)))
        .flat_map(|rect| [(rect.x0, rect.y0), (rect.x1, rect.y0), (rect.x0, rect.y1), (rect.x1, rect.y1)]);
    let mut across = Interval::default();
    let mut down = Interval::default();
    for (x, y) in corners {
        across.include(x - y);
        down.include(x + y);
    }
    (across.length() * COS_30 + PICTURE_SIDE_ROOM, down.length() * SIN_30 + PICTURE_HEAD_ROOM)
}

/// Running min/max of a set of values.
#[derive(Debug, Clone, Copy)]
struct Interval {
    min: f64,
    max: f64,
}

impl Default for Interval {
    fn default() -> Self {
        Self { min: f64::INFINITY, max: f64::NEG_INFINITY }
    }
}

impl Interval {
    fn include(&mut self, value: f64) {
        self.min = self.min.min(value);
        self.max = self.max.max(value);
    }

    fn length(&self) -> f64 {
        if self.max >= self.min { self.max - self.min } else { 0.0 }
    }
}

/// How the rows of a fold share the floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Packing {
    /// Aligned columns (zones of a column share its width).
    Grid,
    /// Each row at its own widths ([`shelves`]).
    Shelves,
}

/// Serpentine rows of `per_row` cells. Hosted guests go on the outer side of the fold (behind the first row,
/// in front of the last one), so the edges running between rows never cross them.
fn fold(cells: Vec<RowCell>, per_row: usize, packing: Packing) -> Block {
    let rows = serpentine_rows(cells, per_row.max(1));
    let last = rows.len().saturating_sub(1);
    let rows = rows
        .into_iter()
        .enumerate()
        .map(|(row, cells)| {
            let guests_in_front = row > 0 && row == last;
            cells.into_iter().map(|cell| cell.map(|cell| cell.compose(guests_in_front))).collect()
        })
        .collect();
    match packing {
        Packing::Grid => grid(rows),
        Packing::Shelves => shelves(rows),
    }
}

/// Rows of `per_row` cells; odd rows run backwards so the flow folds in a "U".
fn serpentine_rows<T>(blocks: Vec<T>, per_row: usize) -> Vec<Vec<Option<T>>> {
    let mut rows = chunk_rows(blocks.into_iter().map(Some).collect(), per_row);
    for (row, cells) in rows.iter_mut().enumerate() {
        if row % 2 == 1 {
            cells.resize_with(per_row, || None);
            cells.reverse();
        }
    }
    rows
}

fn chunk_rows<T>(cells: Vec<Option<T>>, per_row: usize) -> Vec<Vec<Option<T>>> {
    let per_row = per_row.max(1);
    let mut rows = Vec::new();
    let mut cells = cells.into_iter().peekable();
    while cells.peek().is_some() {
        rows.push(cells.by_ref().take(per_row).collect());
    }
    rows
}

/// ⌈√n⌉ for small counts, at least 1.
fn ceil_sqrt(count: usize) -> usize {
    let mut root = 1;
    while root * root < count {
        root += 1;
    }
    root
}

fn mean(values: impl Iterator<Item = f64>) -> f64 {
    let (sum, count) = values.fold((0.0, 0_u32), |(sum, count), value| (sum + value, count + 1));
    if count == 0 { 0.0 } else { sum / f64::from(count) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ceil_sqrt_matches_the_compact_rule() {
        assert_eq!([0, 1, 2, 3, 4, 5, 9, 10].map(ceil_sqrt), [1, 1, 2, 2, 2, 3, 3, 4]);
    }

    #[test]
    fn serpentine_reverses_odd_rows_and_pads_them() {
        let blocks: Vec<Block> =
            (0..5).map(|index| Block::node_in_cell(index, PlanPoint::default(), (1.0, 1.0), false)).collect();
        let rows = serpentine_rows(blocks, 3);
        let ids: Vec<Vec<Option<usize>>> = rows
            .iter()
            .map(|row| row.iter().map(|cell| cell.as_ref().map(|block| block.nodes[0].0)).collect())
            .collect();
        assert_eq!(ids, vec![vec![Some(0), Some(1), Some(2)], vec![None, Some(4), Some(3)]]);
    }

    #[test]
    fn frame_transposes_only_for_flow_y() {
        let point = PlanPoint::new(1.0, 2.0);
        assert_eq!(FlowFrame::new(FlowAxis::X).point(point), point);
        assert_eq!(FlowFrame::new(FlowAxis::Y).point(point), PlanPoint::new(2.0, 1.0));
        assert_eq!(FlowFrame::new(FlowAxis::Y).size([3.0, 4.0]), (4.0, 3.0));
    }
}
