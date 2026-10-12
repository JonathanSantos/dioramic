//! Continuous placement of the nodes: localized stress majorization (Gansner, Koren, North; with the
//! constraint projections of Dwyer, Koren, Marriott) on a short-range stress, plus a degree-weighted pull to
//! the middle.
//!
//! Each round moves every node to the weighted mean of where its terms want it:
//! - **neighbours** at their ideal edge length (one cell inside a zone, the zone padding across zones);
//! - **two hops away**, pushed out to √2 ideal lengths only when closer (an L-turn is fine, a fold is not), so a
//!   long chain snakes into a block instead of stretching into a line;
//! - the **centroid**, harder the busier the node: hubs end up in the middle;
//! - the **centroid of its zone**, so zones stay compact clusters.
//!
//! Then three projections: edges keep pointing forward along the flow (soft separation constraints, as the
//! directed edges of Dig-CoLa), members of each container stop overlapping (zones as rectangles, nodes as
//! their cells), moving whole subtrees, bottom-up.

use crate::geometry::{PlanPoint, PlanRect, PlanVector};

use super::graph::HubGraph;
use super::seed::SeedCell;
use super::tree::{Container, Member, ZoneTree};

/// Majorization rounds (30 nodes stay far below a millisecond per round).
const ROUNDS: usize = 100;
/// Overlap-removal sweeps per round.
const OVERLAP_SWEEPS: usize = 3;
/// Two nodes two hops apart keep at least this many ideal lengths between them.
const SECOND_HOP_RATIO: f64 = std::f64::consts::SQRT_2;
/// Weight of the second-hop term relative to an edge.
const SECOND_HOP_WEIGHT: f64 = 0.5;
/// Pull to the centroid every node gets, relative to an edge of one cell.
const GRAVITY_BASE: f64 = 0.04;
/// Extra pull to the centroid of the busiest node (scaled by degree / largest degree).
const GRAVITY_HUB: f64 = 0.45;
/// Pull to the centroid of the node's zone.
const ZONE_COHESION: f64 = 0.15;
/// An edge along the flow keeps its head at least this many cells ahead of its tail.
const FORWARD_RUN: f64 = 0.3;
/// Fraction of a forward violation fixed per round (soft constraint).
const FORWARD_STIFFNESS: f64 = 0.5;
/// Distances below this (tiles) carry no direction.
const COINCIDENT: f64 = 1e-9;

/// What the relaxation needs besides the positions.
pub(super) struct RelaxInput<'a> {
    /// Edge graph.
    pub graph: &'a HubGraph,
    /// Zone tree.
    pub tree: &'a ZoneTree,
    /// `(along, across)` cell of a node (tiles).
    pub cell: (f64, f64),
    /// Room each zone keeps around its members on every side: padding plus half the zone gap (tiles).
    pub zone_room: &'a [f64],
    /// Seed cell of each top-level member: top-level members are pushed apart along the seed's axis and order,
    /// so they never pass through each other.
    pub root_cells: &'a [SeedCell],
}

/// Relaxes `positions` (node centers, flow frame) in place. Deterministic: fixed visiting order, no randomness.
pub(super) fn relax(input: &RelaxInput<'_>, positions: &mut [PlanPoint]) {
    if positions.is_empty() {
        return;
    }
    let containers = input.tree.bottom_up();
    let subtrees: Vec<Vec<usize>> =
        (0..input.zone_room.len()).map(|zone| input.tree.subtree_nodes(Member::Zone(zone))).collect();
    for _ in 0..ROUNDS {
        majorize(input, positions);
        keep_forward(input, positions);
        for _ in 0..OVERLAP_SWEEPS {
            for &container in &containers {
                separate_members(input, container, &subtrees, positions);
            }
        }
    }
}

fn mean_cell(input: &RelaxInput<'_>) -> f64 {
    (input.cell.0 + input.cell.1) / 2.0
}

/// Ideal distance between two neighbours: one cell in the same container, else both zone rooms.
fn ideal_length(input: &RelaxInput<'_>, a: usize, b: usize) -> f64 {
    let cell = mean_cell(input);
    let (zone_a, zone_b) = (input.tree.node_zone[a], input.tree.node_zone[b]);
    if zone_a == zone_b {
        return cell;
    }
    let room = |zone: Option<usize>| zone.map_or(cell / 2.0, |zone| input.zone_room[zone]);
    (room(zone_a) + room(zone_b)).max(cell)
}

/// Running weighted sum of targets.
#[derive(Default)]
struct Targets {
    x: f64,
    y: f64,
    weight: f64,
}

impl Targets {
    fn add(&mut self, target: PlanPoint, weight: f64) {
        self.x += target.x * weight;
        self.y += target.y * weight;
        self.weight += weight;
    }

    fn mean(&self) -> Option<PlanPoint> {
        (self.weight > 0.0).then(|| PlanPoint::new(self.x / self.weight, self.y / self.weight))
    }
}

/// The point at `length` from `anchor` towards `point` (a fixed direction when they coincide).
fn at_distance(anchor: PlanPoint, point: PlanPoint, length: f64, tie: usize) -> PlanPoint {
    let offset = anchor.to(point);
    let distance = offset.length();
    let direction = if distance > COINCIDENT {
        offset * (1.0 / distance)
    } else {
        let angle = tie as f64 * 2.399_963; // golden angle: distinct, deterministic directions
        PlanVector::new(angle.cos(), angle.sin())
    };
    anchor + direction * length
}

fn centroid(points: impl Iterator<Item = PlanPoint>) -> Option<PlanPoint> {
    let (sum_x, sum_y, count) =
        points.fold((0.0, 0.0, 0_u32), |(x, y, count), point| (x + point.x, y + point.y, count + 1));
    (count > 0).then(|| PlanPoint::new(sum_x / f64::from(count), sum_y / f64::from(count)))
}

/// One Gauss–Seidel round of the majorization.
fn majorize(input: &RelaxInput<'_>, positions: &mut [PlanPoint]) {
    let graph = input.graph;
    let unit = 1.0 / (mean_cell(input) * mean_cell(input));
    let max_degree = graph.max_degree() as f64;
    let Some(middle) = centroid(positions.iter().copied()) else { return };
    let zone_centers: Vec<Option<PlanPoint>> = (0..input.zone_room.len())
        .map(|zone| {
            centroid(
                positions
                    .iter()
                    .enumerate()
                    .filter(|(node, _)| input.tree.node_zone[*node] == Some(zone))
                    .map(|(_, p)| *p),
            )
        })
        .collect();
    for node in 0..positions.len() {
        let here = positions[node];
        let mut targets = Targets::default();
        for &other in &graph.neighbours[node] {
            let length = ideal_length(input, node, other);
            targets.add(at_distance(positions[other], here, length, node), 1.0 / (length * length));
        }
        for &other in &graph.second_neighbours[node] {
            let length = ideal_length(input, node, other) * SECOND_HOP_RATIO;
            if here.distance(positions[other]) < length {
                targets.add(at_distance(positions[other], here, length, node), SECOND_HOP_WEIGHT / (length * length));
            }
        }
        let busy = graph.degree[node] as f64 / max_degree;
        targets.add(middle, (GRAVITY_BASE + GRAVITY_HUB * busy) * unit);
        if let Some(Some(center)) = input.tree.node_zone[node].map(|zone| zone_centers[zone]) {
            targets.add(center, ZONE_COHESION * unit);
        }
        if let Some(target) = targets.mean() {
            positions[node] = target;
        }
    }
}

/// Edges along the flow keep their head ahead of their tail (soft: a fraction of the violation per round).
fn keep_forward(input: &RelaxInput<'_>, positions: &mut [PlanPoint]) {
    let run = FORWARD_RUN * input.cell.0;
    for &(from, to) in &input.graph.forward {
        let violation = run - (positions[to].x - positions[from].x);
        if violation > 0.0 {
            let step = violation * FORWARD_STIFFNESS / 2.0;
            positions[from].x -= step;
            positions[to].x += step;
        }
    }
}

/// Box a member reserves: a node's cell, or a zone's members grown by the zone room.
fn member_box(input: &RelaxInput<'_>, member: Member, positions: &[PlanPoint]) -> Option<PlanRect> {
    match member {
        Member::Node(node) => Some(PlanRect::around(positions[node], input.cell.0 / 2.0, input.cell.1 / 2.0)),
        Member::Zone(zone) => {
            let inner = input
                .tree
                .members(Container::Zone(zone))
                .iter()
                .filter_map(|&child| member_box(input, child, positions))
                .reduce(PlanRect::union)?;
            let half_cell = input.cell.0.min(input.cell.1) / 2.0;
            Some(inner.inflate((input.zone_room[zone] - half_cell).max(0.0)))
        }
    }
}

fn shifted(rect: PlanRect, by: PlanVector) -> PlanRect {
    PlanRect::new(rect.x0 + by.x, rect.y0 + by.y, rect.x1 + by.x, rect.y1 + by.y)
}

/// Pushes overlapping members of one container apart along the axis of least overlap, half each.
fn separate_members(
    input: &RelaxInput<'_>,
    container: Container,
    subtrees: &[Vec<usize>],
    positions: &mut [PlanPoint],
) {
    let members = input.tree.members(container);
    let mut boxes: Vec<Option<PlanRect>> = members.iter().map(|&member| member_box(input, member, positions)).collect();
    for first in 0..members.len() {
        for second in first + 1..members.len() {
            let (Some(a), Some(b)) = (boxes[first], boxes[second]) else { continue };
            let overlap_x = a.x1.min(b.x1) - a.x0.max(b.x0);
            let overlap_y = a.y1.min(b.y1) - a.y0.max(b.y0);
            if overlap_x <= 0.0 || overlap_y <= 0.0 {
                continue;
            }
            let push = match (container, input.root_cells.get(first), input.root_cells.get(second)) {
                (Container::Root, Some(&cell_a), Some(&cell_b)) => seed_push(cell_a, cell_b, a, b),
                _ => least_overlap_push(input, a, b, overlap_x, overlap_y),
            };
            for (index, by) in [(first, push * -1.0), (second, push)] {
                translate_member(members[index], subtrees, by, positions);
                boxes[index] = boxes[index].map(|rect| shifted(rect, by));
            }
        }
    }
}

/// Push of the second member away from the first along the axis of least overlap (half the overlap).
fn least_overlap_push(input: &RelaxInput<'_>, a: PlanRect, b: PlanRect, overlap_x: f64, overlap_y: f64) -> PlanVector {
    let (center_a, center_b) = (a.center(), b.center());
    if overlap_x / input.cell.0 <= overlap_y / input.cell.1 {
        let sign = if center_a.x <= center_b.x { 1.0 } else { -1.0 };
        PlanVector::new(sign * overlap_x / 2.0, 0.0)
    } else {
        let sign = if center_a.y <= center_b.y { 1.0 } else { -1.0 };
        PlanVector::new(0.0, sign * overlap_y / 2.0)
    }
}

/// Push of the second member away from the first that restores the seed's side: along the axis of the larger
/// cell offset, by half of what it takes for the second box to clear the first on its seed side.
fn seed_push(cell_a: SeedCell, cell_b: SeedCell, a: PlanRect, b: PlanRect) -> PlanVector {
    let (dx, dy) = (cell_b.0 - cell_a.0, cell_b.1 - cell_a.1);
    if dx.abs() >= dy.abs() {
        let needed = if dx > 0 { a.x1 - b.x0 } else { b.x1 - a.x0 };
        PlanVector::new(f64::from(dx.signum()) * needed.max(0.0) / 2.0, 0.0)
    } else {
        let needed = if dy > 0 { a.y1 - b.y0 } else { b.y1 - a.y0 };
        PlanVector::new(0.0, f64::from(dy.signum()) * needed.max(0.0) / 2.0)
    }
}

/// Moves a member (a node, or every node under a zone) by `by`.
fn translate_member(member: Member, subtrees: &[Vec<usize>], by: PlanVector, positions: &mut [PlanPoint]) {
    match member {
        Member::Node(node) => positions[node] = positions[node] + by,
        Member::Zone(zone) => {
            for &node in &subtrees[zone] {
                positions[node] = positions[node] + by;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::fixtures::ModelBuilder;

    fn relaxed(builder: ModelBuilder, start: Vec<PlanPoint>) -> Vec<PlanPoint> {
        let model = builder.build();
        let slots = vec![0; model.nodes.len()];
        let graph = HubGraph::new(&model, &slots);
        let tree = ZoneTree::new(&model);
        let zone_room = vec![1.95; model.zones.len()];
        let input = RelaxInput { graph: &graph, tree: &tree, cell: (2.5, 2.5), zone_room: &zone_room, root_cells: &[] };
        let mut positions = start;
        relax(&input, &mut positions);
        positions
    }

    #[test]
    fn the_hub_of_a_star_ends_in_the_middle() {
        let builder = ModelBuilder::new()
            .nodes(&["a", "b", "hub", "c", "d"])
            .edges(&["hub -> a", "hub -> b", "hub -> c", "hub -> d"]);
        let start = (0..5).map(|index| PlanPoint::new(index as f64 * 2.5, 0.0)).collect();
        let positions = relaxed(builder, start);
        let middle = centroid(positions.iter().copied()).unwrap_or_default();
        let hub_offset = positions[2].distance(middle);
        assert!(positions.iter().enumerate().all(|(node, point)| node == 2 || point.distance(middle) >= hub_offset));
    }

    #[test]
    fn cells_never_overlap_after_relaxing() {
        let builder = ModelBuilder::new().nodes(&["a", "b", "c", "d"]).edges(&["a -> b", "b -> c", "c -> d", "d -> a"]);
        let positions = relaxed(builder, vec![PlanPoint::default(); 4]);
        for (index, a) in positions.iter().enumerate() {
            for b in &positions[index + 1..] {
                let apart = (a.x - b.x).abs() >= 2.5 - 1e-6 || (a.y - b.y).abs() >= 2.5 - 1e-6;
                assert!(apart, "{a:?} and {b:?} overlap");
            }
        }
    }
}
