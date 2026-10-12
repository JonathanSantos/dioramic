//! Label placement (`docs/06-renderizacao.md#labels-e-colisões`): zone labels, node labels and edge pills,
//! in that order, each avoiding everything placed before plus the nodes and the edge paths.
//!
//! Zone labels go first (T-311): they name the layers, have the fewest good spots (the floor along a
//! zone border) and nothing may cover them, so they are reserved right after the nodes and edges and
//! become obstacles for node labels, leader lines and pills.

use dioramic_spec::{Diagnostics, Model};

use crate::canvas::ItemRole;
use crate::geometry::{Affine, PlanPoint, PlanRect, Projection, Rect, ScreenPoint, ScreenVector};
use crate::text::{self, FontFamily, FontSpec};
use crate::theme::{ArrowPosition, Theme};

use super::collision::{
    Candidate, Collisions, Layer, Obstacle, ObstacleIndex, Owner, costs_against, is_free, price_against,
};
use super::visibility::{Cover, MIN_VISIBLE_EDGE_PX, Trace, coincident_length, covered_length};
use super::{CollisionReport, EdgePath, ItemRef, LabelKind, PathWalker, PlacedLabel, SceneEdge, SceneNode, SceneZone};

mod attribution;
mod repair;

use attribution::{MISATTRIBUTION_CHOICE_WEIGHT, Misreading};

/// Node label box: one-line height, extra height per line, horizontal padding, corner radius (px).
pub const NODE_LABEL_HEIGHT: f64 = 28.0;
/// Extra height of each additional node label line (px).
pub const NODE_LABEL_LINE_STEP: f64 = 17.0;
/// Horizontal padding inside a node label box (px, each side).
pub const NODE_LABEL_PADDING_X: f64 = 11.0;
/// Baseline of the first node label line below the box top (px).
pub const NODE_LABEL_BASELINE: f64 = 18.5;
/// Node labels longer than this wrap to 2 lines.
pub const NODE_LABEL_MAX_CHARS: usize = 20;
/// Leader heights tried above the base lift (px), in 7 px steps; the highest is the doc's cap (base + 84).
const NODE_LABEL_EXTRA_LIFTS: [f64; 13] = [0.0, 7.0, 14.0, 21.0, 28.0, 35.0, 42.0, 49.0, 56.0, 63.0, 70.0, 77.0, 84.0];
const NODE_LABEL_LOWER_LIFT: f64 = 18.0;
const NODE_LABEL_MIN_LIFT: f64 = 16.0;
/// Half width of the leader line collision box (px).
const LEADER_HALF_WIDTH_PX: f64 = 1.0;
/// Misread leader length (px) still accepted (grazing a corner).
const LEADER_MISREAD_TOLERANCE_PX: f64 = 4.0;
/// Lateral shifts tried at each height, as a fraction of the box width (8 % steps, T-394).
const NODE_LABEL_SHIFTS: [f64; 13] = [0.0, -0.08, 0.08, -0.16, 0.16, -0.24, 0.24, -0.32, 0.32, -0.4, 0.4, -0.48, 0.48];

/// Edge pill: height, extra height per line, horizontal padding (px).
pub const PILL_HEIGHT: f64 = 20.0;
/// Extra height of each additional pill line (px).
pub const PILL_LINE_STEP: f64 = 13.0;
/// Horizontal padding inside a pill (px, each side).
pub const PILL_PADDING_X: f64 = 7.0;
/// Edge pills longer than this wrap to 2 lines.
pub const PILL_MAX_CHARS: usize = 24;
/// Arc-length fractions tried for pills, in order.
const PILL_FRACTIONS: [f64; 13] = [0.5, 0.4, 0.6, 0.32, 0.68, 0.25, 0.75, 0.45, 0.55, 0.36, 0.64, 0.2, 0.8];

/// Fractions where pills may sit beside the path, the gap kept from the stroke (px), and the fraction
/// step used to estimate the tangent.
const PILL_SIDE_FRACTIONS: [f64; 13] = [0.5, 0.4, 0.6, 0.32, 0.68, 0.25, 0.75, 0.2, 0.8, 0.15, 0.85, 0.1, 0.9];
/// Extra distance from the stroke of the side positions, in pill heights (0 = just clear of it). The outer
/// ring stays within the gap a pill may keep from its edge (`attribution::PILL_MAX_GAP_SHARE`, T-394).
const PILL_SIDE_RINGS: [f64; 4] = [0.0, 0.5, 0.9, 1.25];
/// Gaps (px) of the positions straight above, below, left and right of a path point, tried after the
/// rings: on a short edge squeezed between nodes the normal often points into a node.
const PILL_AXIS_GAPS_PX: [f64; 2] = [4.0, 14.0];
const PILL_SIDE_GAP_PX: f64 = 4.0;
const PILL_TANGENT_STEP: f64 = 0.02;
/// Choice cost of a pill spot hiding its own edge, as a share of the pill area (not added to the score), so
/// an edge stays readable as a line first (T-310). Lower than a misread spot
/// ([`MISATTRIBUTION_CHOICE_WEIGHT`]): a pill over its own short edge still names that edge.
const HIDING_PILL_COST: f64 = 1.8;
/// Largest share of an edge's visible stroke its own pill may cover.
const PILL_MAX_COVER: f64 = 0.5;

/// Zone label pill: height and horizontal padding (px); insets from the zone border (tiles).
pub const ZONE_LABEL_HEIGHT: f64 = 20.0;
/// Horizontal padding inside a zone label (px, each side).
pub const ZONE_LABEL_PADDING_X: f64 = 8.0;
/// Baseline of a zone label inside its local box (px).
pub const ZONE_LABEL_BASELINE: f64 = 14.5;
const ZONE_LABEL_ALONG_INSET: f64 = 0.3;
const ZONE_LABEL_ACROSS_INSET: f64 = 0.5;
/// Step of the fallback slides along zone borders (px on screen at the tile size).
const ZONE_LABEL_SLIDE_PX: f64 = 12.0;
/// Gaps (tiles) between the border and the chip tried inside the zone: the doc's inset (capped), then
/// hugging the border.
const ZONE_LABEL_INSIDE_INSETS: [f64; 2] = [f64::INFINITY, 0.06];
/// Gap (tiles) between the border and a chip placed outside its zone.
const ZONE_LABEL_OUTSIDE_GAP: f64 = 0.08;
/// Choice weight of a zone label spot across the column a node's leader will take (see
/// [`leader_columns`]): as much as covering a zone label weighs for the labels placed later.
const ZONE_LABEL_LEADER_WEIGHT: f64 = 4.0;
/// Length of the slices used to test floor pills for collisions (px).
const ZONE_LABEL_SLICE_PX: f64 = 8.0;

/// Horizontal bands a node body is cut into for collisions.
const NODE_BANDS: usize = 6;
/// Edge collision boxes: Bézier segments sampled and polyline pieces no longer than this (px).
const EDGE_SAMPLES: usize = 32;
const EDGE_MAX_PIECE_PX: f64 = 10.0;

/// What label placement needs besides the scene parts.
pub struct LabelInputs<'a> {
    /// Theme (typography).
    pub theme: &'a Theme,
    /// Projection.
    pub projection: &'a Projection,
    /// Base leader length (density).
    pub lift_px: f64,
}

/// What nodes draw outside their body silhouette, which labels must avoid as well.
pub struct DrawnExtras<'a> {
    /// Silhouettes grown over the back replica copies (same order as the nodes).
    pub outlines: &'a [Vec<ScreenPoint>],
    /// Badge and replica-count pills (same order as the nodes).
    pub badges: &'a [Vec<Rect>],
    /// Where each node's leader starts (same order as the nodes).
    pub anchors: &'a [ScreenPoint],
}

/// Result of placing every label.
pub struct Placement {
    /// Residual collisions.
    pub report: CollisionReport,
    /// `W_LABEL_OVERLAP` findings.
    pub diagnostics: Diagnostics,
}

/// Font of node labels.
pub fn node_label_font(theme: &Theme) -> FontSpec {
    let typography = &theme.typography;
    FontSpec {
        family: FontFamily::Inter,
        weight: typography.node_label_weight,
        size: typography.node_label_size,
        letter_spacing: 0.0,
    }
}

/// Font of edge pills.
pub fn pill_font(theme: &Theme) -> FontSpec {
    let typography = &theme.typography;
    FontSpec {
        family: FontFamily::Inter,
        weight: typography.pill_weight,
        size: typography.pill_size,
        letter_spacing: 0.0,
    }
}

/// Font of zone labels.
pub fn zone_label_font(theme: &Theme) -> FontSpec {
    let typography = &theme.typography;
    FontSpec {
        family: FontFamily::Inter,
        weight: typography.zone_label_weight,
        size: typography.zone_label_size,
        letter_spacing: typography.zone_label_letter_spacing,
    }
}

fn widest_line(lines: &[String], font: &FontSpec) -> f64 {
    lines.iter().map(|line| text::measure(line, font).width).fold(0.0, f64::max)
}

fn extra_lines(lines: &[String]) -> f64 {
    lines.len().saturating_sub(1) as f64
}

/// Places all labels in computation order and fills the scene parts.
pub fn place_labels(
    model: &Model,
    inputs: &LabelInputs<'_>,
    nodes: &mut [SceneNode],
    edges: &mut [SceneEdge],
    zones: &mut [SceneZone],
    silhouettes: &[Vec<ScreenPoint>],
    drawn: &DrawnExtras<'_>,
) -> Placement {
    let node_obstacles: Vec<Obstacle> = nodes
        .iter()
        .zip(drawn.outlines)
        .zip(drawn.badges)
        .flat_map(|((node, silhouette), badges)| {
            let item = ItemRef { role: ItemRole::Node, id: node.id.to_string() };
            let badge = ItemRef { role: ItemRole::Badge, id: node.id.to_string() };
            let body = node_parts(node.shape.screen_bbox, silhouette)
                .into_iter()
                .map(move |rect| Obstacle { rect, item: item.clone() });
            body.chain(badges.iter().map(move |rect| Obstacle { rect: *rect, item: badge.clone() }))
        })
        .collect();
    let (strokes, heads): (Vec<Obstacle>, Vec<Obstacle>) =
        edge_obstacles(edges).into_iter().partition(|obstacle| obstacle.item.role == ItemRole::Edge);
    let mut context = PlacementContext {
        inputs,
        anchors: drawn.anchors,
        node_candidates: Vec::new(),
        incident: Vec::new(),
        nodes: ObstacleIndex::new(node_obstacles),
        strokes: ObstacleIndex::new(strokes),
        heads: ObstacleIndex::new(heads),
        node_texts: measure_node_labels(model, inputs.theme, nodes),
        pills: pill_requests(model, inputs.theme, edges, silhouettes),
        leaders: LeaderCheck::new(
            silhouettes.iter().map(|silhouette| Cover::new(silhouette)).collect(),
            edges.iter().map(|edge| Trace::of(&edge.path)).collect(),
            drawn.anchors,
            inputs.lift_px,
        ),
    };
    let mut reserved = Collisions::default();
    let leaders = leader_columns(nodes, drawn.anchors, &context.node_texts, inputs.lift_px);
    let zone_labels = place_zone_labels(
        model,
        inputs,
        zones,
        &ZoneLabelObstacles {
            fixed: &[Layer::Indexed(&context.nodes), Layer::Indexed(&context.strokes), Layer::Indexed(&context.heads)],
            leaders: &leaders,
        },
        &mut reserved,
    );
    let zone_labels = ObstacleIndex::new(zone_labels);
    context.price(nodes, edges, &zone_labels);
    // The greedy result depends on which node label claims a spot first: try a few orders, keep the best
    // (first wins ties, so the doc's top-to-bottom order is the default).
    let mut best: Option<Trial> = None;
    for order in NodeLabelOrder::ALL {
        let mut trial = Trial {
            placement: Placement { report: CollisionReport::default(), diagnostics: Diagnostics::new() },
            nodes: nodes.to_vec(),
            edges: edges.to_vec(),
            order,
        };
        // A trial whose score already reaches the best one cannot win (ties keep the earlier order): it stops
        // as soon as it gets there.
        let budget = best.as_ref().map_or(f64::INFINITY, |current| current.placement.report.score);
        let targets = (&mut trial.nodes[..], &mut trial.edges[..]);
        trial.placement = context.run(order, reserved.clone(), &zone_labels, targets, budget);
        let score = trial.placement.report.score;
        if best.as_ref().is_none_or(|current| score < current.placement.report.score) {
            best = Some(trial);
            if score == 0.0 {
                break;
            }
        }
    }
    let Some(mut best) = best else {
        return Placement { report: reserved.report, diagnostics: reserved.diagnostics };
    };
    if best.placement.report.score > 0.0 {
        context.repair(&mut best, reserved, &zone_labels);
    }
    nodes.clone_from_slice(&best.nodes);
    edges.clone_from_slice(&best.edges);
    best.placement
}

/// One complete placement attempt.
struct Trial {
    placement: Placement,
    nodes: Vec<SceneNode>,
    edges: Vec<SceneEdge>,
    order: NodeLabelOrder,
}

/// Order in which node labels claim their spots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeLabelOrder {
    /// Top of the screen first (doc 06).
    TopDown,
    /// Bottom of the screen first.
    BottomUp,
    /// Widest labels first.
    WidestFirst,
    /// Pills before node labels (top to bottom): edges crowded around a hub leave pills few spots, and a
    /// node label can usually go elsewhere.
    PillsFirst,
}

impl NodeLabelOrder {
    const ALL: [NodeLabelOrder; 4] = [Self::TopDown, Self::BottomUp, Self::WidestFirst, Self::PillsFirst];

    /// Node indices in the order their labels are placed.
    fn sequence(self, anchors: &[ScreenPoint], texts: &[Option<NodeLabelText>]) -> Vec<usize> {
        let mut order: Vec<usize> = (0..anchors.len()).collect();
        order.sort_by(|&a, &b| {
            let (pa, pb) = (anchors[a], anchors[b]);
            pa.y.total_cmp(&pb.y).then(pa.x.total_cmp(&pb.x)).then(a.cmp(&b))
        });
        match self {
            NodeLabelOrder::TopDown | NodeLabelOrder::PillsFirst => {}
            NodeLabelOrder::BottomUp => order.reverse(),
            NodeLabelOrder::WidestFirst => {
                let width = |index: usize| texts[index].as_ref().map_or(0.0, |text| text.width);
                order.sort_by(|&a, &b| width(b).total_cmp(&width(a)).then(a.cmp(&b)));
            }
        }
        order
    }
}

/// Inputs shared by every placement trial.
struct PlacementContext<'a, 's> {
    inputs: &'a LabelInputs<'a>,
    /// Leader start of each node (same order as the nodes).
    anchors: &'a [ScreenPoint],
    leaders: LeaderCheck<'s>,
    nodes: ObstacleIndex,
    strokes: ObstacleIndex,
    heads: ObstacleIndex,
    node_texts: Vec<Option<NodeLabelText>>,
    /// Node label candidates per node (same order as the nodes), priced against the fixed obstacles.
    node_candidates: Vec<Vec<Candidate<()>>>,
    /// Ids of the edges touching each node (same order as the nodes).
    incident: Vec<Vec<String>>,
    pills: Vec<PillRequest>,
}

impl PlacementContext<'_, '_> {
    /// Builds the node label candidates and prices them and the pills against what never moves between
    /// trials (nodes, strokes, heads, zone labels), so a trial only searches the labels it places.
    fn price(&mut self, nodes: &[SceneNode], edges: &[SceneEdge], zone_labels: &ObstacleIndex) {
        let (fixed_nodes, strokes, heads, zone_labels) = (
            Layer::Indexed(&self.nodes),
            Layer::Indexed(&self.strokes),
            Layer::Indexed(&self.heads),
            Layer::Indexed(zone_labels),
        );
        self.node_candidates = nodes
            .iter()
            .zip(&self.node_texts)
            .zip(self.anchors)
            .map(|((node, text), &anchor)| {
                let Some(text) = text else { return Vec::new() };
                let mut candidates = node_label_candidates(anchor, text.width, text.height, self.inputs.lift_px);
                let incident = incident_edges(node, edges);
                price_against(&mut candidates, &[fixed_nodes, strokes, heads, zone_labels], |other| {
                    is_own_node_or_stroke(other, node, &incident)
                });
                // Over its own edges is tolerated, not wanted: those spots go after the clear ones.
                let over_own_edges = costs_against(&candidates, &[strokes], |other| {
                    !(other.role == ItemRole::Edge && incident.contains(&other.id))
                });
                let (mut clear, over_own): (Vec<_>, Vec<_>) =
                    candidates.into_iter().zip(over_own_edges).partition(|(_, cost)| *cost <= 0.0);
                clear.extend(over_own);
                clear.into_iter().map(|(candidate, _)| candidate).collect()
            })
            .collect();
        self.incident = nodes.iter().map(|node| incident_edges(node, edges)).collect();
        for request in &mut self.pills {
            let own = edges[request.edge].id.to_string();
            price_against(&mut request.candidates, &[fixed_nodes, heads, zone_labels, strokes], |other| {
                is_own_edge(other, &own, OwnHeads::of(self.inputs.theme))
            });
            // A pill nearer another edge, or floating away from its own (T-394), names the wrong thing: any
            // spot read right wins, and a small collision is still preferred to it.
            let traces = &self.leaders.edges;
            request.nearer_edges = request
                .candidates
                .iter()
                .zip(&request.own_gaps)
                .map(|(candidate, &own_gap)| attribution::nearer_edge(&candidate.rect, request.edge, own_gap, traces))
                .collect();
            for ((candidate, &own_gap), &nearer) in
                request.candidates.iter_mut().zip(&request.own_gaps).zip(&request.nearer_edges)
            {
                let measured = (own_gap, nearer);
                if attribution::pill_misreading_from(&candidate.rect, request.edge, measured, std::iter::empty())
                    .is_some()
                {
                    candidate.fixed_cost += MISATTRIBUTION_CHOICE_WEIGHT * candidate.rect.area();
                }
            }
        }
    }

    /// Node labels then pills (or the reverse), starting from the reserved zone labels and their collisions.
    fn run(
        &self,
        order: NodeLabelOrder,
        mut collisions: Collisions,
        zone_labels: &ObstacleIndex,
        (nodes, edges): (&mut [SceneNode], &mut [SceneEdge]),
        budget: f64,
    ) -> Placement {
        let (fixed_nodes, strokes, heads, zone_labels) = (
            Layer::Indexed(&self.nodes),
            Layer::Indexed(&self.strokes),
            Layer::Indexed(&self.heads),
            Layer::Indexed(zone_labels),
        );
        let job = NodeLabelJob {
            anchors: self.anchors,
            texts: &self.node_texts,
            candidates: &self.node_candidates,
            incident: &self.incident,
            leaders: &self.leaders,
            order,
        };
        let label_priced = [fixed_nodes, strokes, heads, zone_labels];
        let pill_priced = [fixed_nodes, heads, zone_labels, strokes];
        if order == NodeLabelOrder::PillsFirst {
            let pills = PillJob { node_labels: &[], own_heads: OwnHeads::of(self.inputs.theme) };
            let placed = place_pills(&pills, &self.pills, edges, (&[], &pill_priced), &mut collisions, budget);
            place_node_labels(&job, nodes, (&placed, &label_priced), &mut collisions, budget);
        } else {
            let node_labels = place_node_labels(&job, nodes, (&[], &label_priced), &mut collisions, budget);
            let pills = PillJob { node_labels: &node_labels, own_heads: OwnHeads::of(self.inputs.theme) };
            let placed = [Layer::Loose(&node_labels)];
            place_pills(&pills, &self.pills, edges, (&placed, &pill_priced), &mut collisions, budget);
        }
        if collisions.report.score < budget {
            self.record_misattributions(nodes, edges, &mut collisions);
        }
        Placement { report: collisions.report, diagnostics: collisions.diagnostics }
    }

    /// Adds every label of the finished placement that reads as another item's ([`attribution::audit`]) to
    /// the score and the diagnostics.
    fn record_misattributions(&self, nodes: &[SceneNode], edges: &[SceneEdge], collisions: &mut Collisions) {
        for found in attribution::audit(nodes, edges, &self.leaders.edges) {
            let path = match found.label.role {
                ItemRole::EdgeLabel => self
                    .pills
                    .iter()
                    .find(|request| edges[request.edge].id.as_str() == found.label.id)
                    .map(|request| request.path.as_str()),
                _ => nodes
                    .iter()
                    .position(|node| node.id.as_str() == found.label.id)
                    .and_then(|index| self.node_texts[index].as_ref())
                    .map(|text| text.path.as_str()),
            };
            let message = found.message();
            collisions.record_misread(path.unwrap_or_default(), found.label, found.other, found.penalty, message);
        }
    }

    /// Moves the labels left in a residual overlap of `trial` ([`repair`]) and keeps the result when its
    /// score, measured like a trial's, is lower.
    fn repair(&self, trial: &mut Trial, reserved: Collisions, zone_labels: &ObstacleIndex) {
        let mut movables: Vec<repair::Movable<'_>> = Vec::new();
        let mut targets: Vec<RepairTarget> = Vec::new();
        for (index, node) in trial.nodes.iter().enumerate() {
            let candidates = &self.node_candidates[index];
            let Some(chosen) = chosen_candidate(candidates, node.label.as_ref()) else { continue };
            let leader = Some(repair::LeaderFoot { node: index, anchor: self.anchors[index] });
            let in_overlap = in_overlap(&trial.placement.report, ItemRole::NodeLabel, node.id.as_str());
            movables.push(repair::Movable { candidates, chosen, leader, in_overlap, own_gaps: &[] });
            targets.push(RepairTarget::NodeLabel(index));
        }
        for request in &self.pills {
            let candidates = &request.candidates;
            let Some(chosen) = chosen_candidate(candidates, trial.edges[request.edge].label.as_ref()) else { continue };
            let in_overlap =
                in_overlap(&trial.placement.report, ItemRole::EdgeLabel, trial.edges[request.edge].id.as_str());
            let own_gaps = &request.own_gaps;
            movables.push(repair::Movable { candidates, chosen, leader: None, in_overlap, own_gaps });
            targets.push(RepairTarget::Pill(request.edge));
        }
        let before: Vec<usize> = movables.iter().map(|movable| movable.chosen).collect();
        repair::repair(&mut movables, &self.leaders);
        if movables.iter().map(|movable| movable.chosen).eq(before) {
            return;
        }
        let mut repaired = Trial {
            placement: Placement { report: CollisionReport::default(), diagnostics: Diagnostics::new() },
            nodes: trial.nodes.clone(),
            edges: trial.edges.clone(),
            order: trial.order,
        };
        for (movable, target) in movables.iter().zip(&targets) {
            let rect = movable.candidates[movable.chosen].rect;
            match *target {
                RepairTarget::NodeLabel(index) => {
                    let node = &mut repaired.nodes[index];
                    let anchor = self.anchors[index];
                    node.leader = Some((anchor, ScreenPoint::new(anchor.x, rect.y1)));
                    if let Some(label) = &mut node.label {
                        label.rect = rect;
                    }
                }
                RepairTarget::Pill(edge) => {
                    if let Some(label) = &mut repaired.edges[edge].label {
                        label.rect = rect;
                    }
                }
            }
        }
        repaired.placement = self.rescore(&repaired, reserved, zone_labels);
        if repaired.placement.report.score < trial.placement.report.score {
            *trial = repaired;
        }
    }

    /// Scores a finished placement the way [`Self::run`] would have scored it, in the trial's order.
    fn rescore(&self, trial: &Trial, mut collisions: Collisions, zone_labels: &ObstacleIndex) -> Placement {
        let (fixed_nodes, strokes, heads, zone_labels) = (
            Layer::Indexed(&self.nodes),
            Layer::Indexed(&self.strokes),
            Layer::Indexed(&self.heads),
            Layer::Indexed(zone_labels),
        );
        let label_priced = [fixed_nodes, strokes, heads, zone_labels];
        let pill_priced = [fixed_nodes, heads, zone_labels, strokes];
        if trial.order == NodeLabelOrder::PillsFirst {
            let pills = self.rescore_pills(trial, &[], &pill_priced, &mut collisions);
            self.rescore_node_labels(trial, &pills, &label_priced, &mut collisions);
        } else {
            let node_labels = self.rescore_node_labels(trial, &[], &label_priced, &mut collisions);
            self.rescore_pills(trial, &[Layer::Loose(&node_labels)], &pill_priced, &mut collisions);
        }
        self.record_misattributions(&trial.nodes, &trial.edges, &mut collisions);
        Placement { report: collisions.report, diagnostics: collisions.diagnostics }
    }

    /// Records the node labels of `trial` in its order against `placed_before`, `priced` and each other.
    fn rescore_node_labels(
        &self,
        trial: &Trial,
        placed_before: &[Obstacle],
        priced: &[Layer<'_>],
        collisions: &mut Collisions,
    ) -> Vec<Obstacle> {
        let mut placed: Vec<Obstacle> = Vec::new();
        for index in trial.order.sequence(self.anchors, &self.node_texts) {
            let node = &trial.nodes[index];
            let (Some(text), Some(label)) = (&self.node_texts[index], &node.label) else { continue };
            let ignore = |other: &ItemRef| is_own_node_or_stroke(other, node, &self.incident[index]);
            let item = ItemRef { role: ItemRole::NodeLabel, id: node.id.to_string() };
            let leader = repair::leader_rect(self.anchors[index], label.rect);
            let mut chosen = Candidate::new(label.rect, ()).with_leader(leader);
            price_against(std::slice::from_mut(&mut chosen), priced, ignore);
            let owner = Owner { item: item.clone(), path: &text.path };
            let layers = [Layer::Loose(placed_before), Layer::Loose(&placed)];
            collisions.pick_priced(&owner, vec![chosen], &layers, priced, ignore);
            placed.push(Obstacle { rect: leader, item: ItemRef { role: ItemRole::Leader, id: item.id.clone() } });
            placed.push(Obstacle { rect: label.rect, item });
        }
        placed
    }

    /// Records the pills of `trial` against `placed`, `priced` and each other.
    fn rescore_pills(
        &self,
        trial: &Trial,
        placed: &[Layer<'_>],
        priced: &[Layer<'_>],
        collisions: &mut Collisions,
    ) -> Vec<Obstacle> {
        let mut pills: Vec<Obstacle> = Vec::new();
        for request in &self.pills {
            let edge = &trial.edges[request.edge];
            let Some(label) = &edge.label else { continue };
            let own = edge.id.to_string();
            let ignore = |other: &ItemRef| is_own_edge(other, &own, OwnHeads::of(self.inputs.theme));
            let item = ItemRef { role: ItemRole::EdgeLabel, id: own.clone() };
            let mut chosen = Candidate::new(label.rect, ());
            price_against(std::slice::from_mut(&mut chosen), priced, ignore);
            let owner = Owner { item: item.clone(), path: &request.path };
            let layers = [placed, &[Layer::Loose(&pills)]].concat();
            collisions.pick_priced(&owner, vec![chosen], &layers, priced, ignore);
            pills.push(Obstacle { rect: label.rect, item });
        }
        pills
    }
}

/// Which scene label a repaired candidate belongs to.
#[derive(Debug, Clone, Copy)]
enum RepairTarget {
    /// Label of the node at this index.
    NodeLabel(usize),
    /// Pill of the edge at this index.
    Pill(usize),
}

/// True when the label `(role, id)` is one side of a residual overlap of `report` (a node label's leader
/// line counts as the node label).
fn in_overlap(report: &CollisionReport, role: ItemRole, id: &str) -> bool {
    let as_label = |item_role: ItemRole| if item_role == ItemRole::Leader { ItemRole::NodeLabel } else { item_role };
    let is = |item: &ItemRef| as_label(item.role) == role && item.id == id;
    report.overlaps.iter().any(|overlap| is(&overlap.a) || is(&overlap.b))
}

/// Index of the candidate a trial placed `label` on (`None` when there is no label).
fn chosen_candidate(candidates: &[Candidate<()>], label: Option<&PlacedLabel>) -> Option<usize> {
    let label = label?;
    candidates.iter().position(|candidate| candidate.rect == label.rect)
}

/// Collision boxes of a node: horizontal bands of its body silhouette (the screen bbox also holds the
/// soft shadow and the empty corners of the hexagon, which would push floor labels away for nothing).
/// Anything drawn above the silhouette (antennas, monitors) is covered by extending the top band.
fn node_parts(bbox: Rect, silhouette: &[ScreenPoint]) -> Vec<Rect> {
    let body = Rect::from_points(silhouette.iter().copied());
    if body.is_empty() || bbox.is_empty() {
        return vec![bbox];
    }
    let top = bbox.y0.min(body.y0);
    let bottom = body.y1;
    let step = (bottom - top) / NODE_BANDS as f64;
    (0..NODE_BANDS)
        .filter_map(|band| {
            let (y0, y1) = (top + band as f64 * step, top + (band + 1) as f64 * step);
            let (x0, x1) = band_x_range(silhouette, y0.max(body.y0), y1.max(body.y0))?;
            Some(Rect::new(x0, y0, x1, y1))
        })
        .collect()
}

/// Horizontal extent of a convex polygon between two heights.
fn band_x_range(polygon: &[ScreenPoint], y0: f64, y1: f64) -> Option<(f64, f64)> {
    let mut range: Option<(f64, f64)> = None;
    let mut include = |x: f64| range = Some(range.map_or((x, x), |(low, high)| (low.min(x), high.max(x))));
    for (index, &a) in polygon.iter().enumerate() {
        let b = polygon[(index + 1) % polygon.len()];
        if a.y >= y0 && a.y <= y1 {
            include(a.x);
        }
        for y in [y0, y1] {
            if (a.y - y) * (b.y - y) < 0.0 {
                include(a.x + (b.x - a.x) * (y - a.y) / (b.y - a.y));
            }
        }
    }
    range
}

/// Collision boxes of edge strokes (thin boxes along the path) and of arrow heads.
fn edge_obstacles(edges: &[SceneEdge]) -> Vec<Obstacle> {
    let mut obstacles = Vec::new();
    for edge in edges {
        let half = edge.width / 2.0;
        let samples = edge.path.sample(EDGE_SAMPLES, EDGE_MAX_PIECE_PX);
        for pair in samples.windows(2) {
            obstacles.push(Obstacle {
                rect: Rect::from_points([pair[0], pair[1]]).inflate(half),
                item: ItemRef { role: ItemRole::Edge, id: edge.id.to_string() },
            });
        }
        for head in edge.painted_heads() {
            obstacles.push(Obstacle {
                rect: Rect::from_points([head.tip, head.base[0], head.base[1]]),
                item: ItemRef { role: ItemRole::ArrowHead, id: edge.id.to_string() },
            });
        }
        // With heads painted midway, the stroke runs on to the end heads' tips.
        if !edge.midway.is_empty() {
            for head in &edge.heads {
                let base = head.base[0].midpoint(head.base[1]);
                obstacles.push(Obstacle {
                    rect: Rect::from_points([base, head.tip]).inflate(half),
                    item: ItemRef { role: ItemRole::Edge, id: edge.id.to_string() },
                });
            }
        }
    }
    obstacles
}

/// A node label measured once per scene (trials only change where it goes).
#[derive(Debug, Clone)]
pub(crate) struct NodeLabelText {
    lines: Vec<String>,
    /// Box width (px).
    pub(crate) width: f64,
    /// Box height (px).
    pub(crate) height: f64,
    path: String,
}

/// Measures every node label, indexed like `nodes`.
pub(crate) fn measure_node_labels(model: &Model, theme: &Theme, nodes: &[SceneNode]) -> Vec<Option<NodeLabelText>> {
    let font = node_label_font(theme);
    nodes
        .iter()
        .map(|node| {
            let spec = model.node(&node.id)?;
            let label = spec.label.as_deref().filter(|label| !label.trim().is_empty())?;
            let lines = text::wrap_label(label, NODE_LABEL_MAX_CHARS);
            let width = widest_line(&lines, &font) + 2.0 * NODE_LABEL_PADDING_X;
            let height = NODE_LABEL_HEIGHT + extra_lines(&lines) * NODE_LABEL_LINE_STEP;
            Some(NodeLabelText { lines, width, height, path: format!("{}.label", spec.path) })
        })
        .collect()
}

/// Ids of the edges that start or end at `node`.
fn incident_edges(node: &SceneNode, edges: &[SceneEdge]) -> Vec<String> {
    edges.iter().filter(|edge| edge.from == node.id || edge.to == node.id).map(|edge| edge.id.to_string()).collect()
}

/// What a node label may cover without it counting as a collision: its own node and the strokes of its
/// own edges (T-313). An edge leaving a switch straight up runs under its label whatever the label does
/// within the doc's leader cap; the reader still sees which node the label and the edge belong to. Heads
/// are not included: an arrow under a label is lost.
fn is_own_node_or_stroke(item: &ItemRef, node: &SceneNode, incident: &[String]) -> bool {
    match item.role {
        ItemRole::Node => item.id == node.id.as_str(),
        ItemRole::Edge => incident.contains(&item.id),
        _ => false,
    }
}

/// What node label placement needs besides the scene parts.
struct NodeLabelJob<'a> {
    anchors: &'a [ScreenPoint],
    texts: &'a [Option<NodeLabelText>],
    candidates: &'a [Vec<Candidate<()>>],
    incident: &'a [Vec<String>],
    leaders: &'a LeaderCheck<'a>,
    order: NodeLabelOrder,
}

/// What a leader line must not run through (T-313): other nodes' silhouettes (indexed like the nodes)
/// and the edge strokes (a vertical leader on top of a vertical edge reads as part of it).
struct LeaderCheck<'s> {
    covers: Vec<Cover<'s>>,
    edges: Vec<Trace>,
    /// Per node: `(leader end y, fixed misread length)` for every candidate height (same for all trials).
    fixed: Vec<Vec<(f64, f64)>>,
}

impl<'s> LeaderCheck<'s> {
    /// Checks against `covers` and `edges`, with the fixed part measured once for every candidate height.
    fn new(covers: Vec<Cover<'s>>, edges: Vec<Trace>, anchors: &[ScreenPoint], lift_px: f64) -> Self {
        let mut check = Self { covers, edges, fixed: Vec::new() };
        check.fixed = anchors
            .iter()
            .enumerate()
            .map(|(index, &anchor)| {
                node_label_lifts(lift_px)
                    .map(|lift| {
                        let end = ScreenPoint::new(anchor.x, anchor.y - lift);
                        (end.y, check.fixed_misread_length(index, anchor, end))
                    })
                    .collect()
            })
            .collect();
        check
    }

    /// Length (px) of the leader from `anchor` to `end` that a reader would misread: inside another node,
    /// on top of an edge, or across a label already placed.
    #[cfg(test)]
    fn misread_length(&self, own: usize, anchor: ScreenPoint, end: ScreenPoint, placed: &[Obstacle]) -> f64 {
        self.fixed_misread_length(own, anchor, end) + Self::across_labels(anchor, end, placed)
    }

    /// The part of [`Self::misread_length`] that does not depend on the labels placed so far.
    fn fixed_misread_length(&self, own: usize, anchor: ScreenPoint, end: ScreenPoint) -> f64 {
        let trace = Trace::segment(anchor, end);
        let through: f64 = self
            .covers
            .iter()
            .enumerate()
            .filter(|(node, _)| *node != own)
            .map(|(_, cover)| covered_length(&trace, cover))
            .sum();
        let along_edges: f64 = self.edges.iter().map(|edge| coincident_length(&trace, edge)).sum();
        through + along_edges
    }

    /// Length (px) of the leader running across labels already placed.
    fn across_labels(anchor: ScreenPoint, end: ScreenPoint, placed: &[Obstacle]) -> f64 {
        let leader = Rect::from_points([anchor, end]).inflate(LEADER_HALF_WIDTH_PX);
        placed
            .iter()
            .filter(|obstacle| obstacle.item.role != ItemRole::Leader)
            .map(|obstacle| {
                let overlap = Rect::new(
                    leader.x0.max(obstacle.rect.x0),
                    leader.y0.max(obstacle.rect.y0),
                    leader.x1.min(obstacle.rect.x1),
                    leader.y1.min(obstacle.rect.y1),
                );
                if overlap.is_empty() { 0.0 } else { overlap.height() }
            })
            .sum()
    }
}

/// Places node labels in `job.order`, avoiding `placed_before` (pills placed first) and the labels placed
/// so far; `priced` are the fixed obstacles the candidates were priced against.
fn place_node_labels(
    job: &NodeLabelJob<'_>,
    nodes: &mut [SceneNode],
    (placed_before, priced): (&[Obstacle], &[Layer<'_>]),
    collisions: &mut Collisions,
    budget: f64,
) -> Vec<Obstacle> {
    let texts = job.texts;
    let order = job.order.sequence(job.anchors, texts);
    let mut placed: Vec<Obstacle> = Vec::new();
    for index in order {
        if collisions.report.score >= budget {
            break;
        }
        let node = &nodes[index];
        let Some(text) = &texts[index] else { continue };
        let (width, height) = (text.width, text.height);
        let anchor = job.anchors[index];
        let candidates = &job.candidates[index];
        // Candidates whose leader would be misread go last: a label that collides a little is better than
        // a leader through another icon or along an edge.
        let fixed_by_height = &job.leaders.fixed[index];
        let leader_reads_right = |candidate: &Candidate<()>| {
            let end = ScreenPoint::new(anchor.x, candidate.rect.y1);
            let fixed = fixed_by_height.iter().find(|(height, _)| *height == end.y).map_or(0.0, |(_, fixed)| *fixed);
            let across = LeaderCheck::across_labels(anchor, end, &placed)
                + LeaderCheck::across_labels(anchor, end, placed_before);
            fixed + across <= LEADER_MISREAD_TOLERANCE_PX
        };
        let (reads_right, misread): (Vec<&Candidate<()>>, Vec<&Candidate<()>>) =
            candidates.iter().partition(|candidate| leader_reads_right(candidate));
        let item = ItemRef { role: ItemRole::NodeLabel, id: node.id.to_string() };
        let chosen = collisions.pick_priced_with(
            &Owner { item: item.clone(), path: &text.path },
            reads_right.into_iter().chain(misread).map(|candidate| {
                let misread = misread_leader_area(anchor, candidate.rect, placed.iter().chain(placed_before));
                (candidate, MISATTRIBUTION_CHOICE_WEIGHT * misread)
            }),
            &[Layer::Loose(placed_before), Layer::Loose(&placed)],
            priced,
            |other| is_own_node_or_stroke(other, &nodes[index], &job.incident[index]),
        );
        let Some(chosen) = chosen else { continue };
        let leader_end = ScreenPoint::new(anchor.x, chosen.rect.y1);
        // The leader line is part of the label: later labels and pills must not cover it.
        let leader = ItemRef { role: ItemRole::Leader, id: item.id.clone() };
        placed.push(Obstacle { rect: repair::leader_rect(anchor, chosen.rect), item: leader });
        placed.push(Obstacle { rect: chosen.rect, item });
        let node = &mut nodes[index];
        node.leader = Some((anchor, leader_end));
        node.label = Some(PlacedLabel {
            kind: LabelKind::Node,
            lines: text.lines.clone(),
            rect: chosen.rect,
            size: (width, height),
            transform: None,
        });
    }
    placed
}

/// Area (px²) of the node labels a node label at `rect` (leader from `anchor`) would make readers misplace
/// (T-394): another label its own leader runs behind, or itself when it sits in front of another leader.
fn misread_leader_area<'o>(anchor: ScreenPoint, rect: Rect, placed: impl Iterator<Item = &'o Obstacle>) -> f64 {
    let end = ScreenPoint::new(anchor.x, rect.y1);
    placed
        .map(|obstacle| match obstacle.item.role {
            ItemRole::NodeLabel if attribution::leader_runs_behind(anchor, end, &obstacle.rect) => obstacle.rect.area(),
            ItemRole::Leader => {
                let x = obstacle.rect.center().x;
                let (bottom, top) = (ScreenPoint::new(x, obstacle.rect.y1), ScreenPoint::new(x, obstacle.rect.y0));
                if attribution::leader_runs_behind(bottom, top, &rect) { rect.area() } else { 0.0 }
            }
            _ => 0.0,
        })
        .sum()
}

/// Node label positions, shortest leader first: at each leader height (base, the lower fallback, then
/// +7 px steps up to +84 px, never higher: doc 06) every lateral shift (centered, then ±8 % … ±48 % of the
/// width) is tried before going up. The fine grid (T-394) lets a label in a crowded cluster slip between
/// its neighbours' leaders instead of hiding one of them.
fn node_label_candidates(anchor: ScreenPoint, width: f64, height: f64, base_lift: f64) -> Vec<Candidate<()>> {
    let lifts = node_label_lifts(base_lift);
    let at = move |lift: f64, shift: f64| {
        let bottom = anchor.y - lift;
        let center_x = anchor.x + shift * width;
        let rect = Rect::new(center_x - width / 2.0, bottom - height, center_x + width / 2.0, bottom);
        Candidate::new(rect, ()).with_leader(repair::leader_rect(anchor, rect))
    };
    lifts.flat_map(|lift| NODE_LABEL_SHIFTS.iter().map(move |shift| at(lift, *shift))).collect()
}

/// Leader heights of node labels, in the order they are tried.
fn node_label_lifts(base_lift: f64) -> impl Iterator<Item = f64> {
    let lower = (base_lift - NODE_LABEL_LOWER_LIFT).max(NODE_LABEL_MIN_LIFT);
    std::iter::once(base_lift)
        .chain([lower])
        .chain(NODE_LABEL_EXTRA_LIFTS.iter().skip(1).map(move |extra| base_lift + extra))
}

/// Pills with their candidates, built once per scene (candidates do not depend on the trial).
fn pill_requests(
    model: &Model,
    theme: &Theme,
    edges: &[SceneEdge],
    silhouettes: &[Vec<ScreenPoint>],
) -> Vec<PillRequest> {
    let font = pill_font(theme);
    let covers: Vec<Cover<'_>> = silhouettes.iter().map(|silhouette| Cover::new(silhouette)).collect();
    let mut requests = Vec::new();
    for (index, edge) in edges.iter().enumerate() {
        let Some(spec) = model.edges.iter().find(|candidate| candidate.id == edge.id) else { continue };
        let Some(label) = spec.label.as_deref().filter(|label| !label.trim().is_empty()) else { continue };
        let lines = text::wrap_label(label, PILL_MAX_CHARS);
        let width = widest_line(&lines, &font) + 2.0 * PILL_PADDING_X;
        let height = PILL_HEIGHT + extra_lines(&lines) * PILL_LINE_STEP;
        let candidates = keep_edge_visible(&edge.path, pill_candidates(&edge.path, width, height), &covers);
        let trace = Trace::of(&edge.path);
        let own_gaps = candidates.iter().map(|candidate| attribution::gap_to_trace(&candidate.rect, &trace)).collect();
        requests.push(PillRequest {
            edge: index,
            lines,
            size: (width, height),
            candidates,
            own_gaps,
            nearer_edges: Vec::new(),
            path: format!("{}.label", spec.path),
        });
    }
    requests
}

/// Moves the pill candidates that would hide more than [`PILL_MAX_COVER`] of the edge's visible stroke, or
/// leave less than [`MIN_VISIBLE_EDGE_PX`] of it, to the end and charges them (T-310): a short edge must still read as a line, so its pill goes beside it
/// even at the price of a small collision.
fn keep_edge_visible(path: &EdgePath, candidates: Vec<Candidate<()>>, covers: &[Cover<'_>]) -> Vec<Candidate<()>> {
    let trace = Trace::of(path);
    let visible: Vec<(ScreenPoint, f64)> = trace
        .pieces()
        .filter(|(middle, _, _)| covers.iter().all(|cover| !cover.hides(*middle)))
        .map(|(middle, _, length)| (middle, length))
        .collect();
    let total: f64 = visible.iter().map(|(_, length)| length).sum();
    let hides_edge = |candidate: &Candidate<()>| {
        let under: f64 =
            visible.iter().filter(|(middle, _)| candidate.rect.contains(*middle)).map(|(_, length)| length).sum();
        // Past half the stroke, or leaving less than a readable stub (T-394: pills now hug their edge).
        under > PILL_MAX_COVER * total || (under > 0.0 && total - under < MIN_VISIBLE_EDGE_PX)
    };
    let (mut kept, mut demoted): (Vec<_>, Vec<_>) =
        candidates.into_iter().partition(|candidate| !hides_edge(candidate));
    for candidate in &mut demoted {
        candidate.fixed_cost += HIDING_PILL_COST * candidate.rect.area();
    }
    kept.extend(demoted);
    kept
}

/// Places the pills; their candidates were priced against `priced` (nodes, heads, zone labels, strokes)
/// and `placed` holds the node labels of this trial.
fn place_pills(
    job: &PillJob<'_>,
    requests: &[PillRequest],
    edges: &mut [SceneEdge],
    (placed, priced): (&[Layer<'_>], &[Layer<'_>]),
    collisions: &mut Collisions,
    budget: f64,
) -> Vec<Obstacle> {
    // Pills avoid labels, nodes, other pills, arrow heads and the other edges' strokes: a pill on top of
    // another edge reads as that edge's label (T-312). Their own stroke is fine.
    let mut pills: Vec<Obstacle> = Vec::new();
    let free: Vec<usize> = requests
        .iter()
        .map(|request| {
            let own = edges[request.edge].id.to_string();
            request
                .candidates
                .iter()
                .filter(|candidate| is_free(candidate, placed, |other| is_own_edge(other, &own, job.own_heads)))
                .count()
        })
        .collect();
    // Most constrained pills first: a pill with a single free spot should get it before a flexible one
    // that could have gone elsewhere takes it. Ties keep the spec order.
    let mut order: Vec<usize> = (0..requests.len()).collect();
    order.sort_by_key(|&position| (free[position], requests[position].edge));
    let labels: Vec<(&Rect, usize)> = job
        .node_labels
        .iter()
        .filter(|obstacle| obstacle.item.role == ItemRole::NodeLabel)
        .map(|obstacle| (&obstacle.rect, 0))
        .collect();
    for position in order {
        if collisions.report.score >= budget {
            break;
        }
        let request = &requests[position];
        let edge = &mut edges[request.edge];
        let item = ItemRef { role: ItemRole::EdgeLabel, id: edge.id.to_string() };
        let own = edge.id.to_string();
        let layers = [placed, &[Layer::Loose(&pills)]].concat();
        let owner = Owner { item: item.clone(), path: &request.path };
        let candidates = request
            .candidates
            .iter()
            .enumerate()
            .map(|(index, candidate)| (candidate, near_node_label_cost(request, index, candidate, &labels)));
        let chosen = collisions
            .pick_priced_with(&owner, candidates, &layers, priced, |other| is_own_edge(other, &own, job.own_heads));
        let Some(chosen) = chosen else { continue };
        pills.push(Obstacle { rect: chosen.rect, item });
        let (lines, size) = (request.lines.clone(), request.size);
        edge.label = Some(PlacedLabel { kind: LabelKind::Edge, lines, rect: chosen.rect, size, transform: None });
    }
    pills
}

/// What pill placement needs to tell whether a spot reads as another item's.
struct PillJob<'a> {
    /// Node labels (and leaders) placed before the pills in this trial.
    node_labels: &'a [Obstacle],
    /// Whether the edge's own heads are in the way.
    own_heads: OwnHeads,
}

/// Choice cost of candidate `index` of `request` when it sits nearer a node label placed in this trial
/// (`labels`) than to the pill's own edge (T-394; the other misreadings were charged once in
/// [`PlacementContext::price`]).
fn near_node_label_cost(
    request: &PillRequest,
    index: usize,
    candidate: &Candidate<()>,
    labels: &[(&Rect, usize)],
) -> f64 {
    if labels.is_empty() {
        return 0.0;
    }
    let measured = (request.own_gaps[index], request.nearer_edges[index]);
    let misreading = attribution::pill_misreading_from(&candidate.rect, request.edge, measured, labels.iter().copied());
    if matches!(misreading, Some((Misreading::NearerNodeLabel, _))) {
        MISATTRIBUTION_CHOICE_WEIGHT * candidate.rect.area()
    } else {
        0.0
    }
}

/// A pill waiting to be placed.
#[derive(Debug, Clone)]
struct PillRequest {
    edge: usize,
    lines: Vec<String>,
    size: (f64, f64),
    candidates: Vec<Candidate<()>>,
    /// Gap (px) between each candidate and the edge stroke (same order as `candidates`).
    own_gaps: Vec<f64>,
    /// Edge each candidate sits nearer to than to its own ([`attribution::nearer_edge`], same order as
    /// `candidates`), measured once in [`PlacementContext::price`].
    nearer_edges: Vec<Option<usize>>,
    path: String,
}

fn is_own_edge(item: &ItemRef, own: &str, heads: OwnHeads) -> bool {
    let own_part = match heads {
        OwnHeads::Ignored => matches!(item.role, ItemRole::Edge | ItemRole::ArrowHead),
        OwnHeads::Avoided => item.role == ItemRole::Edge,
    };
    own_part && item.id == own
}

/// Whether a pill may cover the heads of its own edge: yes at the ends (classic), no when the heads are
/// painted halfway, where the pill would sit on them (T-408).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OwnHeads {
    Ignored,
    Avoided,
}

impl OwnHeads {
    fn of(theme: &Theme) -> Self {
        match theme.edge_drawing.arrow_position {
            ArrowPosition::End => Self::Ignored,
            ArrowPosition::Middle => Self::Avoided,
        }
    }
}

/// Pill positions: centered on the path at [`PILL_FRACTIONS`], then (fallback) beside the path, just clear
/// of the stroke on either side, at the central fractions.
fn pill_candidates(path: &EdgePath, width: f64, height: f64) -> Vec<Candidate<()>> {
    let walker = PathWalker::new(path);
    let walker = &walker;
    let on_path = PILL_FRACTIONS
        .iter()
        .map(|&fraction| Candidate::new(Rect::centered(walker.point_at(fraction), width, height), ()));
    let beside = PILL_SIDE_FRACTIONS.iter().flat_map(move |&fraction| {
        let center = walker.point_at(fraction);
        let ahead = walker.point_at((fraction + PILL_TANGENT_STEP).min(1.0));
        let behind = walker.point_at((fraction - PILL_TANGENT_STEP).max(0.0));
        let tangent = (ahead - behind).normalized();
        let normal = ScreenVector::new(-tangent.y, tangent.x);
        let clearance = normal.x.abs() * width / 2.0 + normal.y.abs() * height / 2.0 + PILL_SIDE_GAP_PX;
        PILL_SIDE_RINGS.into_iter().flat_map(move |ring| {
            let distance = clearance + ring * height;
            [1.0, -1.0]
                .map(|side| Candidate::new(Rect::centered(center + normal * (side * distance), width, height), ()))
        })
    });
    let axis = PILL_SIDE_FRACTIONS.iter().flat_map(move |&fraction| {
        let point = walker.point_at(fraction);
        PILL_AXIS_GAPS_PX.into_iter().flat_map(move |gap| {
            let (dx, dy) = ((width / 2.0 + gap), (height / 2.0 + gap));
            [(0.0, -dy), (0.0, dy), (-dx, 0.0), (dx, 0.0)]
                .map(|(x, y)| Candidate::new(Rect::centered(point + ScreenVector::new(x, y), width, height), ()))
        })
    });
    on_path.chain(beside).chain(axis).collect()
}

/// Zone rectangle in the rotated world, recovered from its projected floor polygon.
fn turned_zone_rect(zone: &SceneZone, projection: &Projection) -> PlanRect {
    let back = projection.unproject(zone.polygon[0], 0.0);
    let front = projection.unproject(zone.polygon[2], 0.0);
    PlanRect::new(back.x, back.y, front.x, front.y)
}

/// A zone label lying on the floor: local origin on the plan and the direction its text runs.
#[derive(Debug, Clone, Copy)]
struct FloorPlacement {
    origin: PlanPoint,
    along_x: bool,
}

/// What zone labels avoid: what is already drawn (`fixed`) and, as a preference only, the column of every
/// labelled node's leader line (`leaders`), which node labels place later and must not draw across a zone
/// label (T-381).
struct ZoneLabelObstacles<'a, 'l> {
    fixed: &'a [Layer<'l>],
    leaders: &'a [Obstacle],
}

/// Column every labelled node's leader may take (from the node up to the highest lift), as `Leader`
/// obstacles.
fn leader_columns(
    nodes: &[SceneNode],
    anchors: &[ScreenPoint],
    texts: &[Option<NodeLabelText>],
    lift_px: f64,
) -> Vec<Obstacle> {
    nodes
        .iter()
        .zip(anchors)
        .zip(texts)
        .filter(|(_, text)| text.is_some())
        .map(|((node, &anchor), _)| {
            let highest = node_label_lifts(lift_px).fold(lift_px, f64::max);
            let top = ScreenPoint::new(anchor.x, anchor.y - highest);
            let rect = Rect::from_points([anchor, top]).inflate(LEADER_HALF_WIDTH_PX);
            Obstacle { rect, item: ItemRef { role: ItemRole::Leader, id: node.id.to_string() } }
        })
        .collect()
}

/// Places every zone label (parents first) and returns their collision boxes.
fn place_zone_labels(
    model: &Model,
    inputs: &LabelInputs<'_>,
    zones: &mut [SceneZone],
    obstacles: &ZoneLabelObstacles<'_, '_>,
    collisions: &mut Collisions,
) -> Vec<Obstacle> {
    let fixed = obstacles.fixed;
    let leaders = [Layer::Loose(obstacles.leaders)];
    let font = zone_label_font(inputs.theme);
    let tile = inputs.projection.tile;
    let mut zone_labels: Vec<Obstacle> = Vec::new();
    for zone in zones.iter_mut() {
        let Some(spec) = model.zone(&zone.id) else { continue };
        let Some(label) = spec.label.as_deref().filter(|label| !label.trim().is_empty()) else { continue };
        let width = text::measure(label, &font).width + 2.0 * ZONE_LABEL_PADDING_X;
        let rect = turned_zone_rect(zone, inputs.projection);
        let step = ZONE_LABEL_SLIDE_PX / tile;
        let candidates: Vec<Candidate<FloorPlacement>> =
            zone_label_spots(rect, width / tile, ZONE_LABEL_HEIGHT / tile, step)
                .into_iter()
                .map(|spot| {
                    let affine = floor_affine(inputs.projection, spot);
                    Candidate {
                        fixed_cost: 0.0,
                        rect: affine.bounds_of(width, ZONE_LABEL_HEIGHT),
                        parts: floor_pill_parts(&affine, width, ZONE_LABEL_HEIGHT),
                        leader: None,
                        value: spot,
                    }
                })
                .collect();
        let item = ItemRef { role: ItemRole::ZoneLabel, id: zone.id.to_string() };
        let path = format!("{}.label", spec.path);
        let layers = [fixed, &[Layer::Loose(&zone_labels)]].concat();
        // Crossing a future leader is a choice cost here; the leader itself reports the overlap when placed.
        let mut candidates = candidates;
        price_against(&mut candidates, &leaders, |_| false);
        for candidate in &mut candidates {
            // The leader will pay the zone label weight when it crosses: charge the zone label the same.
            candidate.fixed_cost *= ZONE_LABEL_LEADER_WEIGHT;
        }
        let Some(chosen) = collisions.pick(&Owner { item: item.clone(), path: &path }, candidates, &layers, |_| false)
        else {
            continue;
        };
        zone_labels.extend(chosen.parts.iter().map(|part| Obstacle { rect: *part, item: item.clone() }));
        zone.label = Some(PlacedLabel {
            kind: LabelKind::Zone,
            lines: vec![label.to_owned()],
            rect: chosen.rect,
            size: (width, ZONE_LABEL_HEIGHT),
            transform: Some(floor_affine(inputs.projection, chosen.value)),
        });
    }
    zone_labels
}

/// Boxes covering a pill lying on the floor: its bbox is a large diamond, so the pill is cut into short
/// slices along its length and each slice's bbox is used instead.
fn floor_pill_parts(affine: &Affine, width: f64, height: f64) -> Vec<Rect> {
    let slices = (width / ZONE_LABEL_SLICE_PX).ceil().max(1.0) as usize;
    let step = width / slices as f64;
    (0..slices)
        .map(|index| {
            let start = index as f64 * step;
            Rect::from_points([
                affine.apply(start, 0.0),
                affine.apply(start + step, 0.0),
                affine.apply(start + step, height),
                affine.apply(start, height),
            ])
        })
        .collect()
}

fn floor_affine(projection: &Projection, spot: FloorPlacement) -> Affine {
    let origin = projection.project(spot.origin, 0.0);
    if spot.along_x { Affine::floor_along_x(origin) } else { Affine::floor_along_y(origin) }
}

/// Zone label spots, best first:
/// 1. the 4 front-border candidates of the doc (front-left start/end, front-right start/end) and the middle
///    of each front border;
/// 2. sliding along the front borders, then along the back borders, inside the zone (normal inset, then
///    hugging the border, which helps small compact zones);
/// 3. last resort, just outside the zone, along the front borders and then the back borders: the floor
///    there is usually free and the chip still touches its zone.
fn zone_label_spots(rect: PlanRect, length: f64, thickness: f64, step: f64) -> Vec<FloorPlacement> {
    let along = ZONE_LABEL_ALONG_INSET;
    let (x0, y0, x1, y1) = (rect.x0, rect.y0, rect.x1, rect.y1);
    let middle_x = (x0 + x1 - length) / 2.0;
    let middle_y = (y0 + y1 + length) / 2.0;
    let on_x = |x: f64, y: f64| FloorPlacement { origin: PlanPoint::new(x, y), along_x: true };
    let on_y = |x: f64, y: f64| FloorPlacement { origin: PlanPoint::new(x, y), along_x: false };
    let across = ZONE_LABEL_ACROSS_INSET;
    let mut spots = vec![
        on_x(x0 + along, y1 - across),
        on_x(x1 - along - length, y1 - across),
        on_y(x1 - across, y1 - along),
        on_y(x1 - across, y0 + along + length),
        on_x(middle_x, y1 - across),
        on_y(x1 - across, middle_y),
    ];
    let along_x = slide(x0 + along, x1 - along - length, step);
    let along_y = slide(y0 + along + length, y1 - along, step);
    // `inset` is the gap between the border and the chip's near side (negative = outside the zone).
    let front = |inset: f64| {
        let along_x = along_x.iter().map(move |&x| on_x(x, y1 - inset - thickness));
        along_x.chain(along_y.iter().map(move |&y| on_y(x1 - inset - thickness, y)))
    };
    let back = |inset: f64| {
        let along_x = along_x.iter().map(move |&x| on_x(x, y0 + inset));
        along_x.chain(along_y.iter().map(move |&y| on_y(x0 + inset, y)))
    };
    let normal = across - thickness;
    for inset in ZONE_LABEL_INSIDE_INSETS.map(|extra| normal.min(extra)) {
        spots.extend(front(inset));
        spots.extend(back(inset));
    }
    spots.extend(front(-ZONE_LABEL_OUTSIDE_GAP - thickness));
    spots.extend(back(-ZONE_LABEL_OUTSIDE_GAP - thickness));
    spots
}

/// Evenly spaced positions from `start` to `end` (at most `step` apart, both ends included).
fn slide(start: f64, end: f64, step: f64) -> Vec<f64> {
    if end < start {
        return vec![start];
    }
    let steps = ((end - start) / step.max(f64::EPSILON)).ceil().max(1.0) as usize;
    (0..=steps).map(|index| start + (end - start) * index as f64 / steps as f64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_label_candidates_shift_sideways_before_rising() {
        let candidates = node_label_candidates(ScreenPoint::new(0.0, 0.0), 100.0, 28.0, 52.0);
        let shifts = NODE_LABEL_SHIFTS.len();
        assert_eq!(candidates.len(), (NODE_LABEL_EXTRA_LIFTS.len() + 1) * shifts);
        assert_eq!(candidates[0].rect, Rect::new(-50.0, -80.0, 50.0, -52.0));
        assert_eq!(candidates[1].rect.center().x, -8.0);
        assert_eq!(candidates[shifts - 1].rect.center().x, 48.0);
        assert!(candidates[..shifts].iter().all(|candidate| candidate.rect.y1 == -52.0));
        assert_eq!(candidates[shifts].rect.y1, -34.0);
        let highest = candidates.iter().map(|candidate| -candidate.rect.y1).fold(0.0, f64::max);
        assert_eq!(highest, 52.0 + 84.0);
    }

    #[test]
    fn leaders_through_nodes_or_along_edges_are_misread() {
        let node = vec![
            ScreenPoint::new(-20.0, -70.0),
            ScreenPoint::new(20.0, -70.0),
            ScreenPoint::new(20.0, -30.0),
            ScreenPoint::new(-20.0, -30.0),
        ];
        let edge = EdgePath::Polyline(vec![ScreenPoint::new(30.0, 0.0), ScreenPoint::new(30.0, -100.0)]);
        let check = LeaderCheck {
            covers: vec![Cover::new(&[]), Cover::new(&node)],
            edges: vec![Trace::of(&edge)],
            fixed: Vec::new(),
        };
        let anchor = ScreenPoint::new(0.0, 0.0);
        let clear = check.misread_length(0, ScreenPoint::new(-40.0, 0.0), ScreenPoint::new(-40.0, -90.0), &[]);
        assert_eq!(clear, 0.0);
        assert!(check.misread_length(0, anchor, ScreenPoint::new(0.0, -90.0), &[]) > 10.0, "through the node");
        let along = check.misread_length(0, ScreenPoint::new(31.0, 0.0), ScreenPoint::new(31.0, -90.0), &[]);
        assert!(along > 80.0, "collinear with the edge: {along}");
        let label = Obstacle {
            rect: Rect::new(-60.0, -30.0, -20.0, -10.0),
            item: ItemRef { role: ItemRole::NodeLabel, id: "x".to_owned() },
        };
        let across = check.misread_length(0, ScreenPoint::new(-40.0, 0.0), ScreenPoint::new(-40.0, -90.0), &[label]);
        assert!(across > 15.0);
    }

    #[test]
    fn zone_spots_lie_inside_the_zone() {
        let rect = PlanRect::new(0.0, 0.0, 4.0, 3.0);
        for spot in zone_label_spots(rect, 1.2, 0.36, 0.2).iter().take(6) {
            let end = if spot.along_x {
                PlanPoint::new(spot.origin.x + 1.2, spot.origin.y + 0.36)
            } else {
                PlanPoint::new(spot.origin.x + 0.36, spot.origin.y - 1.2)
            };
            assert!(rect.contains(spot.origin) && rect.contains(end), "{spot:?}");
        }
    }
}
