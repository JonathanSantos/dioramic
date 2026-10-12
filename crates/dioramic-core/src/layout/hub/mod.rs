//! `layout.algorithm: hub` (T-411): a hub-centered, tight layout. The most connected places go in the middle,
//! edges are as short as the spacing allows, every node sits on the half-tile lattice, zones are packed
//! rectangles, and sources still lean towards the start of the flow.
//!
//! Pipeline (in the flow frame, `x` along the flow):
//! 1. [`seed`]: the top-level items (zones, zone-less nodes) grow from the busiest one on a coarse grid.
//! 2. [`relax`]: node positions by stress majorization with a degree-weighted pull to the middle, soft forward
//!    constraints on the edges and overlap removal between the members of each zone.
//! 3. [`lattice`]: inside every zone (bottom-up) the relaxed positions become an aligned grid.
//! 4. [`compact`]: the top-level blocks keep their relative sides and slide together to shorten and straighten
//!    the edges between them; or, without zones, the top level is one more lattice.
//!
//! See `docs/06-renderizacao.md#layout-hub` and contract C-004.

mod align;
mod compact;
mod graph;
mod improve;
mod lattice;
mod qp;
mod reading;
mod relax;
mod score;
mod seed;
mod tree;

use std::cell::Cell;

use dioramic_spec::{Arrange, Density, Model, Routing};

use super::arrange::{Arrangement, FlowFrame, Spacing};
use super::block::Block;
use super::ranks::FlowRanks;
use crate::geometry::{PlanPoint, PlanRect, PlanVector};
use compact::{PackItem, PackLink, pack};
use graph::HubGraph;
use improve::{SwapGroup, improve};
use lattice::{LatticeItem, lattice, snap_up};
use reading::{MIRROR_COST_SLACK, Mirror};
use relax::{RelaxInput, relax};

pub use reading::source_reading_rank;
use score::{EdgeShape, SharpBends, drawing_cost, sharp_misses};
use seed::{SPREAD_VARIANTS, SeedCell, SeedInput, seed_cells};
use tree::{Container, Member, ZoneTree};

pub(super) use lattice::LATTICE_STEP;

/// `spaced`: extra room between neighbouring nodes (tiles). On a 2-D lattice every neighbour is "across the
/// flow" in some camera rotation, so the label room `compact` keeps only across the flow is needed both ways;
/// at the bare 2.4-tile pitch the label towers of a dense zone run into the icons behind them.
const SPACED_LABEL_ROOM: f64 = 0.5;
/// `compact` with elbow routing (`rails`, `orthogonal`): room between neighbouring nodes for the corridor a rail
/// runs in (tiles). Nodes about a tile apart leave the rails of a busy node no lane between their neighbours,
/// so they jog or squeeze past bodies; spaced already keeps [`SPACED_LABEL_ROOM`], which is a corridor too
/// (T-413).
const COMPACT_RAIL_CORRIDOR: f64 = 0.5;
/// Weight, in shares of the reading of the sources, of an edge whose rail will turn sharply back on itself at
/// rotation 0 ([`sharp_misses`]): a mirror that saves one is worth its sources reading a little later (T-425).
const SHARP_MISS_WEIGHT: f64 = 0.05;
/// Candidates whose costs differ by less than this tie (the earlier one wins).
const COST_TIE: f64 = 1e-9;
/// Rounds of local improvement, each followed by a new legalization (the re-packing can open new swaps).
const IMPROVE_ROUNDS: usize = 1;
/// [`IMPROVE_ROUNDS`] with elbow edges: the re-packing slides blocks onto the alignments of their rails, which
/// opens moves inside the zones that pay (T-425).
const ELBOW_IMPROVE_ROUNDS: usize = 2;

/// Lays out `model` with the hub algorithm, or `None` when something is pinned with `at` (the `flow` layout
/// knows how to build around pins; the hub layout would have to treat them as fixed obstacles).
pub(super) fn arrange(
    model: &Model,
    ranks: &FlowRanks,
    spacing: &Spacing,
    footprint_half: &[f64],
    frame: FlowFrame,
    density: Density,
) -> Option<Arrangement> {
    let pinned = model.nodes.iter().any(|node| node.at.is_some()) || model.zones.iter().any(|zone| zone.at.is_some());
    if pinned {
        return None;
    }
    let tree = ZoneTree::new(model);
    let hub = HubLayout::new(model, ranks, spacing, footprint_half, frame, density, &tree);
    Some(hub.run())
}

/// Everything the stages share.
struct HubLayout<'a> {
    model: &'a Model,
    tree: &'a ZoneTree,
    graph: HubGraph,
    slots: &'a [u32],
    across: &'a [f64],
    /// `(along, across)` node cell, on the lattice.
    cell: (f64, f64),
    /// Inner padding of each zone.
    zone_padding: Vec<f64>,
    /// Half the gap between sibling zones.
    zone_margin: f64,
    frame: FlowFrame,
    /// How the edges will be drawn (elbows need the L's corner free, not the diagonal).
    edge_shape: Cell<EdgeShape>,
}

impl<'a> HubLayout<'a> {
    fn new(
        model: &'a Model,
        ranks: &'a FlowRanks,
        spacing: &Spacing,
        footprint_half: &[f64],
        frame: FlowFrame,
        density: Density,
        tree: &'a ZoneTree,
    ) -> Self {
        let edge_shape =
            if routes_on_elbows(model) { EdgeShape::Elbow(SharpBends::Unknown) } else { EdgeShape::Straight };
        let label_room = match density {
            Density::Spaced => SPACED_LABEL_ROOM,
            Density::Compact if edge_shape.is_elbow() => COMPACT_RAIL_CORRIDOR,
            Density::Compact => 0.0,
        };
        let zone_padding = (0..model.zones.len())
            .map(|zone| {
                let largest = tree
                    .members(Container::Zone(zone))
                    .iter()
                    .filter_map(|member| match member {
                        Member::Node(node) => Some(footprint_half[*node]),
                        Member::Zone(_) => None,
                    })
                    .fold(0.0, f64::max);
                spacing.zone_padding(largest)
            })
            .collect();
        Self {
            model,
            tree,
            graph: HubGraph::new(model, &ranks.slot),
            slots: &ranks.slot,
            across: &ranks.across,
            cell: (snap_up(spacing.pitch_along + label_room), snap_up(spacing.pitch_across + label_room)),
            zone_padding,
            zone_margin: spacing.zone_margin,
            frame,
            edge_shape: Cell::new(edge_shape),
        }
    }

    /// Lays out the model: the cheapest drawing ([`Self::search`]), mirrored for its reading
    /// ([`Self::read_from_sources`]).
    fn run(&self) -> Arrangement {
        let first = self.search();
        let (block, mirror) = self.read_from_sources(first);
        if !self.edge_shape.get().is_elbow() {
            return self.arrangement(block);
        }
        // Which bends turn sharply on screen depends on the mirror, which depends on the drawing: lay out again
        // knowing the mirror the reading picked, and pick again (T-425).
        let sharp =
            if mirror.sign_product() > 0.0 { SharpBends::WhenOffsetsAgree } else { SharpBends::WhenOffsetsDiffer };
        self.edge_shape.set(EdgeShape::Elbow(sharp));
        let second = self.search();
        let (block, _) = self.read_from_sources(second);
        self.arrangement(block)
    }

    /// The cheapest drawing ([`drawing_cost`]) over the pipeline stages of every seed variant; ties keep the
    /// earlier candidate.
    fn search(&self) -> Block {
        let members = self.tree.members(Container::Root);
        let mut seeds: Vec<Vec<SeedCell>> = Vec::new();
        let mut best: Option<(f64, Block)> = None;
        for spread in SPREAD_VARIANTS {
            let seed = seed_cells(&self.seed_input(members, spread));
            if seeds.contains(&seed) {
                continue;
            }
            for candidate in self.candidates(&seed) {
                let cost = self.cost(&candidate);
                if best.as_ref().is_none_or(|(best_cost, _)| cost < *best_cost - COST_TIE) {
                    best = Some((cost, candidate));
                }
            }
            seeds.push(seed);
        }
        best.map_or_else(Block::empty, |(_, block)| block)
    }

    /// `block` mirrored so its sources come first along the flow (T-415), among the mirrors that cost no more than
    /// [`MIRROR_COST_SLACK`] above the cheapest; ties keep the earlier mirror (the identity first). With rails the
    /// reading is the screen's at rotation 0 with the default flow, plus [`SHARP_MISS_WEIGHT`] for each rail that
    /// would turn back on itself (T-425). Returns the mirror too.
    fn read_from_sources(&self, block: Block) -> (Block, Mirror) {
        let count = self.model.nodes.len();
        let sources = reading::sources(&self.graph.edges, count);
        if sources.is_empty() {
            return (block, Mirror::ALL[0]);
        }
        let options: Vec<(Block, f64, Mirror)> = Mirror::ALL
            .iter()
            .map(|&mirror| {
                let mirrored = mirrored(&block, mirror);
                let cost = self.cost(&mirrored);
                (mirrored, cost, mirror)
            })
            .collect();
        let cheapest = options.iter().map(|(_, cost, _)| *cost).fold(f64::INFINITY, f64::min);
        let affordable = cheapest + MIRROR_COST_SLACK * cheapest.abs() + COST_TIE;
        // Read in the flow frame, so `layout.flow: y` stays the transpose of `x` (the default flow is the plan).
        let rank = |block: &Block| {
            let centers = node_centers(block, count);
            if self.edge_shape.get().is_elbow() {
                let sharp = sharp_misses(&self.graph.edges, &centers) as f64;
                reading::source_reading_rank(&centers, &sources).unwrap_or(0.0) + SHARP_MISS_WEIGHT * sharp
            } else {
                reading::source_flow_rank(&centers, &sources).unwrap_or(0.0)
            }
        };
        options
            .into_iter()
            .filter(|(_, cost, _)| *cost <= affordable)
            .map(|(block, _, mirror)| (rank(&block), block, mirror))
            .fold(None, |best: Option<(f64, Block, Mirror)>, (rank, block, mirror)| match best {
                Some(best) if best.0 <= rank + COST_TIE => Some(best),
                _ => Some((rank, block, mirror)),
            })
            .map_or((block, Mirror::ALL[0]), |(_, chosen, mirror)| (chosen, mirror))
    }

    /// The legalized relaxation of a seed, the result of each improvement round, and the last one polished by
    /// swaps.
    fn candidates(&self, seed: &[SeedCell]) -> Vec<Block> {
        let mut positions = self.initial_positions(seed);
        let zone_room: Vec<f64> = self.zone_padding.iter().map(|padding| padding + self.zone_margin).collect();
        relax(
            &RelaxInput {
                graph: &self.graph,
                tree: self.tree,
                cell: self.cell,
                zone_room: &zone_room,
                root_cells: seed,
            },
            &mut positions,
        );
        let mut root = self.root_block(&positions, seed);
        let rounds = if self.edge_shape.get().is_elbow() { ELBOW_IMPROVE_ROUNDS } else { IMPROVE_ROUNDS };
        let mut candidates = Vec::with_capacity(rounds + 2);
        for _ in 0..rounds {
            let mut centers = node_centers(&root, self.model.nodes.len());
            let mut groups = self.swap_groups(&centers);
            improve(&self.graph.edges, self.cell, self.edge_shape.get(), &mut centers, &mut groups);
            // Legalize again from the improved centers: cells emptied by the moves collapse, blocks re-pack.
            candidates.push(std::mem::replace(&mut root, self.root_block(&centers, seed)));
        }
        // Swaps never leave a cell, so the packed blocks stay legal: one last pass catches what the packing
        // changed (an edge straightened by the packing may now run over a node).
        let polished = self.swapped(&root);
        candidates.push(root);
        candidates.push(polished);
        candidates
    }

    /// `root` after a swap-only improvement pass.
    fn swapped(&self, root: &Block) -> Block {
        let mut centers = node_centers(root, self.model.nodes.len());
        let mut groups = self.swap_groups(&centers);
        for group in &mut groups {
            group.free_cells.clear();
        }
        improve(&self.graph.edges, self.cell, self.edge_shape.get(), &mut centers, &mut groups);
        let mut polished = root.clone();
        for (node, center) in &mut polished.nodes {
            *center = centers[*node];
        }
        polished
    }

    fn cost(&self, block: &Block) -> f64 {
        let centers = node_centers(block, self.model.nodes.len());
        let floor: Vec<PlanRect> = block
            .zones
            .iter()
            .map(|(_, rect)| *rect)
            .chain(
                block.nodes.iter().map(|(_, center)| PlanRect::around(*center, self.cell.0 / 2.0, self.cell.1 / 2.0)),
            )
            .collect();
        drawing_cost(&self.graph.edges, &centers, &floor, self.cell, self.edge_shape.get())
    }

    /// Nodes that may trade cells: the nodes of each zone (and of a top level without zones), with the empty
    /// cells of their grid when the zone holds no nested zone.
    fn swap_groups(&self, centers: &[PlanPoint]) -> Vec<SwapGroup> {
        let root_has_zones = self.tree.members(Container::Root).iter().any(|member| matches!(member, Member::Zone(_)));
        let containers =
            (0..self.model.zones.len()).map(Container::Zone).chain((!root_has_zones).then_some(Container::Root));
        containers
            .filter_map(|container| {
                let members = self.tree.members(container);
                let nodes: Vec<usize> = members
                    .iter()
                    .filter_map(|member| match member {
                        Member::Node(node) => Some(*node),
                        Member::Zone(_) => None,
                    })
                    .collect();
                if nodes.is_empty() {
                    return None;
                }
                let nested = nodes.len() < members.len();
                let free_cells = if nested { Vec::new() } else { free_cells(&nodes, centers, self.cell) };
                let padding = match container {
                    Container::Zone(zone) => self.zone_padding[zone],
                    Container::Root => 0.0,
                };
                Some(SwapGroup { nodes, free_cells, padding })
            })
            .collect()
    }

    /// Seed cell of each top-level item, scaled by the largest item, plus each node's place in its item by flow
    /// slot (along) and barycenter hint (across).
    fn initial_positions(&self, seed: &[SeedCell]) -> Vec<PlanPoint> {
        let members = self.tree.members(Container::Root);
        let (step_x, step_y) = self.seed_step(members);
        let mut positions = vec![PlanPoint::default(); self.model.nodes.len()];
        for (member, &cell) in members.iter().zip(seed) {
            let nodes = self.tree.subtree_nodes(*member);
            let mean_slot = mean(nodes.iter().map(|&node| f64::from(self.slots[node])));
            let center = PlanPoint::new(f64::from(cell.0) * step_x, f64::from(cell.1) * step_y);
            for node in nodes {
                positions[node] = center
                    + PlanVector::new(
                        (f64::from(self.slots[node]) - mean_slot) * self.cell.0,
                        self.across[node] * self.cell.1,
                    );
            }
        }
        positions
    }

    fn seed_input(&self, members: &[Member], spread: f64) -> SeedInput {
        let position_of = |node: usize| members.iter().position(|member| *member == self.tree.top_member(node));
        let mut weights = vec![vec![0.0; members.len()]; members.len()];
        for &(from, to) in &self.graph.edges {
            if let (Some(a), Some(b)) = (position_of(from), position_of(to))
                && a != b
            {
                weights[a][b] += 1.0;
                weights[b][a] += 1.0;
            }
        }
        let slots = members
            .iter()
            .map(|member| {
                let nodes = self.tree.subtree_nodes(*member);
                (!nodes.is_empty()).then(|| mean(nodes.iter().map(|&node| f64::from(self.slots[node]))))
            })
            .collect();
        SeedInput { weights, slots, spread }
    }

    /// Distance between seed cells: room for the largest item (a zone of k nodes is about ⌈√k⌉ cells wide).
    fn seed_step(&self, members: &[Member]) -> (f64, f64) {
        members.iter().fold(self.cell, |(x, y), member| match member {
            Member::Node(_) => (x, y),
            Member::Zone(zone) => {
                let side = (self.tree.subtree_nodes(*member).len() as f64).sqrt().ceil();
                let room = 2.0 * (self.zone_padding[*zone] + self.zone_margin);
                (x.max(side * self.cell.0 + room), y.max(side * self.cell.1 + room))
            }
        })
    }

    /// Laid-out block of a member (local coordinates, nodes on the lattice).
    fn member_block(&self, member: Member, positions: &[PlanPoint]) -> Block {
        match member {
            Member::Node(node) => Block::node_in_cell(node, PlanPoint::default(), self.cell, false),
            Member::Zone(zone) => self.zone_block(zone, positions),
        }
    }

    fn zone_block(&self, zone: usize, positions: &[PlanPoint]) -> Block {
        let content = self.container_block(Container::Zone(zone), positions, None);
        self.enclose(zone, content)
    }

    fn lattice_items(&self, container: Container, positions: &[PlanPoint]) -> Vec<LatticeItem> {
        let members = self.tree.members(container);
        let targets = self.member_targets(members, positions);
        members
            .iter()
            .zip(targets)
            .map(|(&member, position)| LatticeItem {
                block: self.member_block(member, positions),
                position,
                is_node: matches!(member, Member::Node(_)),
            })
            .collect()
    }

    /// Relaxed center of each member: a node's position, a zone's node centroid; empty zones line up after the
    /// rest along the flow.
    fn member_targets(&self, members: &[Member], positions: &[PlanPoint]) -> Vec<PlanPoint> {
        let centroids: Vec<Option<PlanPoint>> = members
            .iter()
            .map(|&member| {
                let nodes = self.tree.subtree_nodes(member);
                (!nodes.is_empty()).then(|| {
                    PlanPoint::new(
                        mean(nodes.iter().map(|&node| positions[node].x)),
                        mean(nodes.iter().map(|&node| positions[node].y)),
                    )
                })
            })
            .collect();
        let end = centroids.iter().flatten().map(|point| point.x).fold(0.0, f64::max);
        let mut empty_rank = 0.0;
        centroids
            .into_iter()
            .map(|centroid| {
                centroid.unwrap_or_else(|| {
                    empty_rank += 1.0;
                    PlanPoint::new(end + empty_rank * self.cell.0, 0.0)
                })
            })
            .collect()
    }

    /// The zone rectangle around its content: padding, or exactly `size` when the spec gives one.
    fn enclose(&self, zone: usize, content: Block) -> Block {
        let padding = self.zone_padding[zone];
        let natural = content.core.inflate(padding);
        let rect = match self.model.zones[zone].size.map(|size| self.frame.size(size)) {
            Some((width, depth)) => PlanRect::around(natural.center(), width / 2.0, depth / 2.0),
            None => natural,
        };
        let mut zones = Vec::with_capacity(content.zones.len() + 1);
        zones.push((zone, rect));
        zones.extend(content.zones);
        Block {
            extent: rect.inflate(self.zone_margin).union(content.extent),
            core: rect,
            nodes: content.nodes,
            zones,
            own_zone: true,
            stretchable: false,
            anchored: false,
        }
    }

    /// The top level.
    fn root_block(&self, positions: &[PlanPoint], seed: &[SeedCell]) -> Block {
        self.container_block(Container::Root, positions, Some(seed))
    }

    /// The members of a container laid out: one aligned lattice when they are all nodes, packed blocks when some
    /// are zones (blocks of different sizes would leave a lattice full of holes).
    fn container_block(&self, container: Container, positions: &[PlanPoint], seed: Option<&[SeedCell]>) -> Block {
        let members = self.tree.members(container);
        if members.iter().all(|member| matches!(member, Member::Node(_))) {
            let arrange = match container {
                Container::Root => Arrange::Auto,
                Container::Zone(zone) => self.model.zones[zone].arrange,
            };
            return lattice(self.lattice_items(container, positions), arrange, self.cell);
        }
        let targets = self.member_targets(members, positions);
        let items: Vec<PackItem> = members
            .iter()
            .zip(targets)
            .map(|(&member, target)| {
                let block = self.member_block(member, positions);
                let reference = node_centroid(&block).unwrap_or_else(|| block.extent.center());
                PackItem { anchor: target - reference, block }
            })
            .collect();
        let links = self.pack_links(members, &items);
        pack(items, &links, seed, self.edge_shape.get())
    }

    /// Edges between different members of a container, with their end points inside the members' blocks.
    fn pack_links(&self, members: &[Member], items: &[PackItem]) -> Vec<PackLink> {
        let mut item_of = vec![None; self.model.nodes.len()];
        for (item, &member) in members.iter().enumerate() {
            for node in self.tree.subtree_nodes(member) {
                item_of[node] = Some(item);
            }
        }
        let local = |item: usize, node: usize| {
            items[item].block.nodes.iter().find(|(index, _)| *index == node).map(|(_, center)| *center)
        };
        self.graph
            .edges
            .iter()
            .filter_map(|&(from, to)| {
                let (from_item, to_item) = (item_of[from]?, item_of[to]?);
                (from_item != to_item).then_some(PackLink {
                    from: from_item,
                    from_point: local(from_item, from)?,
                    to: to_item,
                    to_point: local(to_item, to)?,
                })
            })
            .collect()
    }

    fn arrangement(&self, root: Block) -> Arrangement {
        let node_centers = node_centers(&root, self.model.nodes.len());
        let mut zone_rects = vec![PlanRect::new(0.0, 0.0, 0.0, 0.0); self.model.zones.len()];
        for (index, rect) in root.zones {
            zone_rects[index] = rect;
        }
        Arrangement { node_centers, zone_rects, zone_parent: self.tree.parent.clone(), anchored: false }
    }
}

/// True when most edges are drawn as elbows (`rails`, `orthogonal`), which need corridors between nodes.
fn routes_on_elbows(model: &Model) -> bool {
    let elbows = model.edges.iter().filter(|edge| matches!(edge.route, Routing::Rails | Routing::Orthogonal)).count();
    elbows * 2 > model.edges.len()
}

/// `block` with every node center and zone rectangle mirrored.
fn mirrored(block: &Block, mirror: Mirror) -> Block {
    Block {
        extent: mirror.rect(block.extent),
        core: mirror.rect(block.core),
        nodes: block.nodes.iter().map(|(node, center)| (*node, mirror.point(*center))).collect(),
        zones: block.zones.iter().map(|(zone, rect)| (*zone, mirror.rect(*rect))).collect(),
        ..block.clone()
    }
}

/// Center of every node of a laid-out block, indexed like `model.nodes`.
fn node_centers(block: &Block, count: usize) -> Vec<PlanPoint> {
    let mut centers = vec![PlanPoint::default(); count];
    for (index, center) in &block.nodes {
        centers[*index] = *center;
    }
    centers
}

/// Cells that no node takes in the grid spanned by the nodes' columns and rows, plus one column and one row
/// on every side (a zone may turn from an L into a line).
fn free_cells(nodes: &[usize], centers: &[PlanPoint], cell: (f64, f64)) -> Vec<PlanPoint> {
    let lines = |values: Vec<f64>, step: f64| {
        let mut values = values;
        values.sort_by(f64::total_cmp);
        values.dedup_by(|a, b| (*a - *b).abs() < GRID_TOLERANCE);
        if let (Some(&first), Some(&last)) = (values.first(), values.last()) {
            values.insert(0, first - step);
            values.push(last + step);
        }
        values
    };
    let columns = lines(nodes.iter().map(|&node| centers[node].x).collect(), cell.0);
    let rows = lines(nodes.iter().map(|&node| centers[node].y).collect(), cell.1);
    let taken = |cell: PlanPoint| nodes.iter().any(|&node| centers[node].distance(cell) < GRID_TOLERANCE);
    rows.iter().flat_map(|&y| columns.iter().map(move |&x| PlanPoint::new(x, y))).filter(|&cell| !taken(cell)).collect()
}

/// Lattice coordinates closer than this are the same line (tiles).
const GRID_TOLERANCE: f64 = 1e-6;

fn node_centroid(block: &Block) -> Option<PlanPoint> {
    (!block.nodes.is_empty()).then(|| {
        PlanPoint::new(
            mean(block.nodes.iter().map(|(_, center)| center.x)),
            mean(block.nodes.iter().map(|(_, center)| center.y)),
        )
    })
}

fn mean(values: impl Iterator<Item = f64>) -> f64 {
    let (sum, count) = values.fold((0.0, 0_u32), |(sum, count), value| (sum + value, count + 1));
    if count == 0 { 0.0 } else { sum / f64::from(count) }
}

#[cfg(test)]
mod tests;
