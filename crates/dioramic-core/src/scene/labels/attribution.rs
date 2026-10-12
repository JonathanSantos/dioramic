//! Label attribution (T-394): a label that touches nothing can still be read as another item's.
//!
//! - An edge pill belongs to the edge it is nearest to. A pill nearer another edge or another node's label
//!   than to its own edge (a label by more than [`NEARER_SLACK_SHARE`] of its height), or farther from its own edge
//!   than [`PILL_MAX_GAP_SHARE`] of its height, names the wrong thing (or nothing).
//! - A node's leader that runs behind another node's label makes the node look like it carries that label.
//!
//! [`audit`] measures a finished placement; the scene adds each misattribution to the collision score as if
//! the misread label covered the wrong item ([`Misattribution::penalty`]) and reports it as a
//! `W_LABEL_OVERLAP`, so the trials, the repair and `rotation: auto` all avoid it. Placement also charges the
//! candidates that would be misread ([`MISATTRIBUTION_CHOICE_WEIGHT`]) so the greedy rarely needs that.

use crate::canvas::ItemRole;
use crate::geometry::{Rect, ScreenPoint};

use super::super::visibility::{Trace, squared_distance_to_segment};
use super::super::{ItemRef, SceneEdge, SceneNode};
use super::LEADER_MISREAD_TOLERANCE_PX;

/// Farthest a pill may sit from its own edge (gap between the pill box and the stroke), as a share of the
/// pill height: 30 px for a one-line pill, the outer side ring of the candidates. Farther, it floats free
/// of any edge (`OrderPlaced` 80 px above its edge, T-394).
pub(super) const PILL_MAX_GAP_SHARE: f64 = 1.5;
/// A pill this close to its own stroke (share of its height: the inner side ring) sits on it and reads as
/// its label whatever else is around.
pub(super) const ON_EDGE_SHARE: f64 = 0.25;
/// How much nearer (as a share of the pill height) a node label must be than the own edge for the pill to
/// read as the node's tag: a dark pill and a white label look different, so within half a pill height the
/// stroke still wins. Another stroke looks the same, so any edge nearer than the own one takes the pill.
pub(super) const NEARER_SLACK_SHARE: f64 = 0.5;
/// Choice cost of a misread spot, in multiples of its score penalty: worse than covering a node with the
/// same area would be, so any spot read correctly wins unless it collides heavily.
pub(super) const MISATTRIBUTION_CHOICE_WEIGHT: f64 = 2.0;
/// A leader this close (px) to a side of a label box only grazes its rounded corner.
const LEADER_CORNER_INSET_PX: f64 = 3.0;

/// Why a label reads as another item's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Misreading {
    /// The pill floats this far (px) from its own edge, over the allowed gap.
    TooFar { gap: f64, max_gap: f64 },
    /// The pill is nearer another edge than its own.
    NearerEdge,
    /// The pill is nearer a node label than its own edge.
    NearerNodeLabel,
    /// The node's leader runs behind another node's label.
    LeaderThroughLabel,
}

/// One label read as belonging to something else.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Misattribution {
    /// The misread label: an edge label, or the leader of a node label.
    pub(super) label: ItemRef,
    /// What it is read as (the own edge for [`Misreading::TooFar`]).
    pub(super) other: ItemRef,
    /// Why.
    pub(super) reason: Misreading,
    /// Score penalty (px²): the area of the label that reads wrong.
    pub(super) penalty: f64,
}

impl Misattribution {
    /// Diagnostic message.
    pub(super) fn message(&self) -> String {
        let (label, other) = (&self.label.id, &self.other.id);
        match self.reason {
            Misreading::TooFar { gap, max_gap } => {
                format!(
                    "edge-label '{label}' sits {gap:.0} px from its edge (at most {max_gap:.0} px reads as attached)"
                )
            }
            Misreading::NearerEdge => format!("edge-label '{label}' is nearer edge '{other}' and reads as its label"),
            Misreading::NearerNodeLabel => {
                format!("edge-label '{label}' is nearer the label of node '{other}' than its own edge")
            }
            Misreading::LeaderThroughLabel => {
                format!(
                    "node-label '{label}' leader line runs behind the label of node '{other}', as if it were that node's"
                )
            }
        }
    }
}

/// Gap (px) between a box and a stroke: 0 when the stroke enters the box.
pub(super) fn gap_to_trace(rect: &Rect, trace: &Trace) -> f64 {
    gap_to_trace_below(rect, trace, f64::INFINITY)
}

/// [`gap_to_trace`] when only gaps under `limit` matter: exact below `limit`, otherwise some value at least
/// `limit` (segments that cannot come nearer are skipped).
fn gap_to_trace_below(rect: &Rect, trace: &Trace, limit: f64) -> f64 {
    if trace.points.len() == 1 {
        return trace.points.first().map_or(f64::INFINITY, |point| gap_to_point(rect, *point));
    }
    // The gap to a piece's box never exceeds the gap to the piece: a piece whose box is already farther than
    // the nearest piece found (or than `limit`) cannot win, and skipping it leaves the minimum (and so the
    // placement) bit for bit the same. The piece with the nearest box goes first, so the rest are mostly
    // skipped. The relative slack covers the rounding of the two formulas.
    let box_gap_squared = |bounds: &Rect| {
        let dx = (rect.x0 - bounds.x1).max(bounds.x0 - rect.x1).max(0.0);
        let dy = (rect.y0 - bounds.y1).max(bounds.y0 - rect.y1).max(0.0);
        dx * dx + dy * dy
    };
    let piece_box = |a: ScreenPoint, b: ScreenPoint| Rect::new(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y));
    // Seed with the nearest piece of the nearest run of pieces, then skip whole runs, and pieces, whose box
    // is already farther than the nearest piece found.
    let blocks = trace.blocks();
    let Some((seed_block, _)) = blocks
        .iter()
        .enumerate()
        .map(|(block, bounds)| (block, box_gap_squared(bounds)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
    else {
        return f64::INFINITY;
    };
    let seed_points = trace.block_points(seed_block);
    let Some(seed) = seed_points
        .windows(2)
        .min_by(|a, b| box_gap_squared(&piece_box(a[0], a[1])).total_cmp(&box_gap_squared(&piece_box(b[0], b[1]))))
    else {
        return f64::INFINITY;
    };
    let mut nearest = gap_to_segment(rect, seed[0], seed[1]);
    for (block, bounds) in blocks.iter().enumerate() {
        let bound = nearest.min(limit);
        if box_gap_squared(bounds) > bound * bound * PRUNE_SLACK {
            continue;
        }
        for pair in trace.block_points(block).windows(2) {
            let bound = nearest.min(limit);
            if box_gap_squared(&piece_box(pair[0], pair[1])) > bound * bound * PRUNE_SLACK {
                continue;
            }
            nearest = nearest.min(gap_to_segment(rect, pair[0], pair[1]));
        }
    }
    nearest
}

/// Relative slack (on squared distances) of the bounding-box pruning in [`gap_to_trace`].
const PRUNE_SLACK: f64 = 1.0 + 1e-9;

/// Gap (px) between two boxes: 0 when they touch.
pub(super) fn gap_between(a: &Rect, b: &Rect) -> f64 {
    let dx = (b.x0 - a.x1).max(a.x0 - b.x1).max(0.0);
    let dy = (b.y0 - a.y1).max(a.y0 - b.y1).max(0.0);
    dx.hypot(dy)
}

fn gap_to_point(rect: &Rect, point: ScreenPoint) -> f64 {
    let dx = (rect.x0 - point.x).max(point.x - rect.x1).max(0.0);
    let dy = (rect.y0 - point.y).max(point.y - rect.y1).max(0.0);
    dx.hypot(dy)
}

fn gap_to_segment(rect: &Rect, a: ScreenPoint, b: ScreenPoint) -> f64 {
    if segment_enters(rect, a, b) {
        return 0.0;
    }
    let corners = [
        ScreenPoint::new(rect.x0, rect.y0),
        ScreenPoint::new(rect.x1, rect.y0),
        ScreenPoint::new(rect.x1, rect.y1),
        ScreenPoint::new(rect.x0, rect.y1),
    ];
    // `sqrt` is correctly rounded and monotonic: the root of the smallest square is the smallest of the
    // roots (what `distance_to_segment` returns for each corner), with one root instead of four.
    let nearest_square =
        corners.iter().map(|corner| squared_distance_to_segment(*corner, a, b)).fold(f64::INFINITY, f64::min);
    nearest_square.sqrt().min(gap_to_point(rect, a)).min(gap_to_point(rect, b))
}

/// True when the segment `a`–`b` has a point inside `rect` (Liang–Barsky clipping).
fn segment_enters(rect: &Rect, a: ScreenPoint, b: ScreenPoint) -> bool {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let (mut low, mut high) = (0.0_f64, 1.0_f64);
    for (p, q) in [(-dx, a.x - rect.x0), (dx, rect.x1 - a.x), (-dy, a.y - rect.y0), (dy, rect.y1 - a.y)] {
        if p.abs() <= f64::EPSILON {
            if q < 0.0 {
                return false;
            }
            continue;
        }
        let t = q / p;
        if p < 0.0 {
            low = low.max(t);
        } else {
            high = high.min(t);
        }
        if low > high {
            return false;
        }
    }
    true
}

/// Where a pill box stands with respect to its own edge (`traces[own]`), the other edges and the node label
/// boxes: the first reason it reads wrong, with the item it reads as (`None` when it reads right).
pub(super) fn pill_misreading<'n>(
    rect: &Rect,
    own: usize,
    traces: &[Trace],
    node_labels: impl IntoIterator<Item = (&'n Rect, usize)>,
) -> Option<(Misreading, PillNeighbour)> {
    let own_gap = traces.get(own).map_or(f64::INFINITY, |trace| gap_to_trace(rect, trace));
    let edge = nearer_edge(rect, own, own_gap, traces);
    pill_misreading_from(rect, own, (own_gap, edge), node_labels)
}

/// The index of the edge nearer to the pill box `rect` than its own edge (`own_gap` away), the nearest one
/// when several are; `None` when the pill sits on its own stroke or no other edge is nearer. Strokes look
/// alike: whichever is nearer owns the pill. It does not depend on the labels, so placement measures it
/// once per candidate.
pub(super) fn nearer_edge(rect: &Rect, own: usize, own_gap: f64, traces: &[Trace]) -> Option<usize> {
    if own_gap <= ON_EDGE_SHARE * rect.height() {
        return None;
    }
    traces
        .iter()
        .enumerate()
        .filter(|(index, trace)| *index != own && gap_between(rect, &trace.bounds) < own_gap)
        .map(|(index, trace)| (index, gap_to_trace_below(rect, trace, own_gap)))
        .filter(|(_, gap)| *gap < own_gap)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

/// [`pill_misreading`] from the pill's gap to its own edge and its [`nearer_edge`], both measured before.
pub(super) fn pill_misreading_from<'n>(
    rect: &Rect,
    own: usize,
    (own_gap, nearer_edge): (f64, Option<usize>),
    node_labels: impl IntoIterator<Item = (&'n Rect, usize)>,
) -> Option<(Misreading, PillNeighbour)> {
    if own_gap <= ON_EDGE_SHARE * rect.height() {
        return None;
    }
    if let Some(index) = nearer_edge {
        return Some((Misreading::NearerEdge, PillNeighbour::Edge(index)));
    }
    // A white node label only takes the pill over when it is clearly nearer than the stroke.
    let nearer = own_gap - NEARER_SLACK_SHARE * rect.height();
    let nearest_label = node_labels
        .into_iter()
        .map(|(label, node)| (node, gap_between(rect, label)))
        .filter(|(_, gap)| *gap < nearer)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((node, _)) = nearest_label {
        return Some((Misreading::NearerNodeLabel, PillNeighbour::NodeLabel(node)));
    }
    let max_gap = PILL_MAX_GAP_SHARE * rect.height();
    (own_gap > max_gap).then_some((Misreading::TooFar { gap: own_gap, max_gap }, PillNeighbour::Edge(own)))
}

/// The item a misread pill is read as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PillNeighbour {
    /// Edge at this index.
    Edge(usize),
    /// Label of the node at this index.
    NodeLabel(usize),
}

/// True when a vertical leader from `anchor` up to `end` runs behind `label` (well inside it sideways and
/// over more than the misread tolerance in height).
pub(super) fn leader_runs_behind(anchor: ScreenPoint, end: ScreenPoint, label: &Rect) -> bool {
    let inside_x = anchor.x > label.x0 + LEADER_CORNER_INSET_PX && anchor.x < label.x1 - LEADER_CORNER_INSET_PX;
    let (top, bottom) = (anchor.y.min(end.y), anchor.y.max(end.y));
    let along = bottom.min(label.y1) - top.max(label.y0);
    inside_x && along > LEADER_MISREAD_TOLERANCE_PX
}

/// Every misattribution of a finished placement. `traces` are the edge strokes (same order as `edges`).
pub(super) fn audit(nodes: &[SceneNode], edges: &[SceneEdge], traces: &[Trace]) -> Vec<Misattribution> {
    let node_labels: Vec<(&Rect, usize)> =
        nodes.iter().enumerate().filter_map(|(index, node)| Some((&node.label.as_ref()?.rect, index))).collect();
    let mut found = Vec::new();
    for (index, edge) in edges.iter().enumerate() {
        let Some(label) = &edge.label else { continue };
        let Some((reason, neighbour)) = pill_misreading(&label.rect, index, traces, node_labels.iter().copied()) else {
            continue;
        };
        let other = match neighbour {
            PillNeighbour::Edge(other) => ItemRef { role: ItemRole::Edge, id: edges[other].id.to_string() },
            PillNeighbour::NodeLabel(node) => ItemRef { role: ItemRole::NodeLabel, id: nodes[node].id.to_string() },
        };
        let label_ref = ItemRef { role: ItemRole::EdgeLabel, id: edge.id.to_string() };
        found.push(Misattribution { label: label_ref, other, reason, penalty: label.rect.area() });
    }
    for node in nodes {
        let Some((anchor, end)) = node.leader else { continue };
        for (other_label, other) in &node_labels {
            let other = &nodes[*other];
            if other.id == node.id || !leader_runs_behind(anchor, end, other_label) {
                continue;
            }
            found.push(Misattribution {
                label: ItemRef { role: ItemRole::Leader, id: node.id.to_string() },
                other: ItemRef { role: ItemRole::NodeLabel, id: other.id.to_string() },
                reason: Misreading::LeaderThroughLabel,
                penalty: other_label.area(),
            });
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::EdgePath;

    fn line(from: (f64, f64), to: (f64, f64)) -> Trace {
        Trace::of(&EdgePath::Polyline(vec![ScreenPoint::new(from.0, from.1), ScreenPoint::new(to.0, to.1)]))
    }

    #[test]
    fn gaps_are_zero_inside_and_euclidean_outside() {
        let rect = Rect::new(0.0, 0.0, 40.0, 20.0);
        assert_eq!(gap_to_trace(&rect, &line((-10.0, 10.0), (50.0, 10.0))), 0.0);
        assert!((gap_to_trace(&rect, &line((-10.0, 30.0), (50.0, 30.0))) - 10.0).abs() < 1e-9);
        assert!((gap_between(&rect, &Rect::new(43.0, 24.0, 60.0, 40.0)) - 5.0).abs() < 1e-9);
        assert_eq!(gap_between(&rect, &Rect::new(10.0, 10.0, 60.0, 40.0)), 0.0);
    }

    #[test]
    fn a_pill_on_its_edge_reads_right_even_next_to_another() {
        let traces = [line((0.0, 10.0), (200.0, 10.0)), line((0.0, 32.0), (200.0, 32.0))];
        let on_own = Rect::new(80.0, 0.0, 140.0, 20.0);
        assert_eq!(pill_misreading(&on_own, 0, &traces, []), None);
    }

    #[test]
    fn a_pill_nearer_another_edge_reads_as_its_label() {
        // ShipmentCreated (event-driven, T-394): well above its own curve, just under another edge.
        let traces = [line((0.0, 100.0), (200.0, 100.0)), line((0.0, 40.0), (200.0, 40.0))];
        let pill = Rect::new(80.0, 55.0, 140.0, 75.0);
        assert_eq!(pill_misreading(&pill, 0, &traces, []), Some((Misreading::NearerEdge, PillNeighbour::Edge(1))));
    }

    #[test]
    fn a_node_label_must_be_clearly_nearer_than_the_own_edge() {
        let traces = [line((0.0, 100.0), (200.0, 100.0))];
        let pill = Rect::new(80.0, 72.0, 140.0, 92.0);
        let label = Rect::new(60.0, 40.0, 160.0, 68.0);
        assert_eq!(pill_misreading(&pill, 0, &traces, [(&label, 0)]), None, "8 px from the edge, 4 px from the label");
    }

    #[test]
    fn a_pill_stacked_on_a_node_label_reads_as_the_node() {
        // OrderPlaced (aws-serverless, T-394): far above its edge, right on top of the "SQS" label.
        let traces = [line((0.0, 200.0), (200.0, 200.0))];
        let pill = Rect::new(80.0, 100.0, 180.0, 120.0);
        let sqs = Rect::new(60.0, 124.0, 120.0, 152.0);
        let found = pill_misreading(&pill, 0, &traces, [(&sqs, 3)]);
        assert_eq!(found, Some((Misreading::NearerNodeLabel, PillNeighbour::NodeLabel(3))));
    }

    #[test]
    fn a_pill_floating_away_from_its_edge_is_too_far() {
        let traces = [line((0.0, 100.0), (200.0, 100.0))];
        let close = Rect::new(80.0, 55.0, 140.0, 75.0);
        assert_eq!(pill_misreading(&close, 0, &traces, []), None, "25 px gap is still the outer side ring");
        let far = Rect::new(80.0, 40.0, 140.0, 60.0);
        let found = pill_misreading(&far, 0, &traces, []);
        assert!(
            matches!(found, Some((Misreading::TooFar { gap, .. }, PillNeighbour::Edge(0))) if (gap - 40.0).abs() < 1e-9)
        );
    }

    #[test]
    fn the_audit_names_the_misread_label_and_what_it_reads_as() {
        use crate::color::Color;
        use crate::icons::{Footprint, NodeShape};
        use crate::scene::{LabelKind, PlacedLabel};
        use dioramic_spec::FlowMode;

        let label = |rect: Rect, kind: LabelKind| PlacedLabel {
            kind,
            lines: vec!["x".to_owned()],
            rect,
            size: (rect.width(), rect.height()),
            transform: None,
        };
        let node = |id: &str, anchor: ScreenPoint, rect: Rect| SceneNode {
            id: id.into(),
            plan_center: crate::geometry::PlanPoint::new(0.0, 0.0),
            floor_center: anchor,
            shape: NodeShape {
                footprint: Footprint::Rect { half_width: 0.5, half_depth: 0.5 },
                height: 0.0,
                screen_bbox: Rect::EMPTY,
            },
            markup: String::new(),
            label: Some(label(rect, LabelKind::Node)),
            leader: Some((anchor, ScreenPoint::new(anchor.x, rect.y1))),
            rank: 0,
        };
        let edge = |id: &str, y: f64, pill: Option<Rect>| SceneEdge {
            id: id.into(),
            from: "a".into(),
            to: "b".into(),
            path: EdgePath::Polyline(vec![ScreenPoint::new(300.0, y), ScreenPoint::new(500.0, y)]),
            heads: Vec::new(),
            midway: Vec::new(),
            kind: dioramic_spec::EdgeKind::Service,
            color: Color::rgb(0, 0, 0),
            width: 2.0,
            dashed: false,
            flow: FlowMode::default(),
            label: pill.map(|rect| label(rect, LabelKind::Edge)),
        };
        // `ap` sits behind `ws`: its leader runs up through the workstation label to its own, above it.
        let nodes = [
            node("ap", ScreenPoint::new(100.0, 300.0), Rect::new(50.0, 100.0, 150.0, 128.0)),
            node("ws", ScreenPoint::new(160.0, 260.0), Rect::new(40.0, 180.0, 200.0, 208.0)),
        ];
        let edges = [edge("near", 400.0, Some(Rect::new(380.0, 340.0, 440.0, 360.0))), edge("other", 365.0, None)];
        let traces: Vec<Trace> = edges.iter().map(|edge| Trace::of(&edge.path)).collect();
        let found: Vec<(ItemRole, &str, &str, Misreading)> = audit(&nodes, &edges, &traces)
            .iter()
            .map(|m| (m.label.role, leak(&m.label.id), leak(&m.other.id), m.reason))
            .collect();
        assert_eq!(
            found,
            [
                (ItemRole::EdgeLabel, "near", "other", Misreading::NearerEdge),
                (ItemRole::Leader, "ap", "ws", Misreading::LeaderThroughLabel),
            ]
        );
    }

    fn leak(text: &str) -> &'static str {
        Box::leak(text.to_owned().into_boxed_str())
    }

    #[test]
    fn a_leader_behind_another_label_is_misread_but_one_past_its_corner_is_not() {
        // Staff WiFi AP × Teller Workstation 1 (bank-branch, T-394).
        let workstation = Rect::new(0.0, 100.0, 200.0, 128.0);
        let (anchor, end) = (ScreenPoint::new(120.0, 240.0), ScreenPoint::new(120.0, 60.0));
        assert!(leader_runs_behind(anchor, end, &workstation));
        assert!(!leader_runs_behind(ScreenPoint::new(201.0, 240.0), ScreenPoint::new(201.0, 60.0), &workstation));
        assert!(!leader_runs_behind(ScreenPoint::new(198.5, 240.0), ScreenPoint::new(198.5, 60.0), &workstation));
        assert!(!leader_runs_behind(anchor, ScreenPoint::new(120.0, 130.0), &workstation), "ends below it");
    }
}
