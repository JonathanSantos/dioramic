//! Arrow heads halfway along an edge (theme `arrow_position: middle`, T-408): the schematic look, where
//! the head reads the direction of the flow in the middle of a straight run and the stroke goes on to the
//! port. The end heads stay as they are for ports, aim and lint; these are only painted.

use crate::geometry::{PlanPoint, PlanVector, Projection, Rect, ScreenPoint, ScreenVector};

use dioramic_spec::{Diagnostic, DiagnosticCode, Diagnostics, Model};

use crate::canvas::ItemRole;

use super::{
    ARROW_HALF_WIDTH, ARROW_LENGTH, ArrowHead, COLLISION_MARGIN_PX, CollisionReport, EDGE_Z, EdgePath, ItemRef,
    SceneEdge,
};

/// Midway heads are a little larger than end heads: they sit on the stroke, not past its end, and must
/// stand out of a thick schematic line.
const MIDWAY_HEAD_SCALE: f64 = 1.25;
/// Shortest straight run (in head lengths) that takes the head; without one, the edge keeps its end head.
const MIN_RUN_HEADS: f64 = 1.8;
/// A run of at least this share of the longest one counts as "as long", and the run nearest the middle of
/// the path wins among those: the head sits in the middle of the edge when it can.
const LONG_RUN_SHARE: f64 = 0.8;
/// Gap between the bases of the two heads of a two-way edge, in head lengths: a clear gap, so `<->` reads as
/// two heads pointing apart, never as one "star" (T-414).
const PAIR_GAP_HEADS: f64 = 1.0;
/// Room a run needs beyond the pair itself to take both heads of a two-way edge, in head lengths (a little
/// stroke shows on either side).
const PAIR_RUN_MARGIN_HEADS: f64 = 0.5;
/// Clearance (px) between the two midway heads of one edge when one of them slides (T-414).
const PAIR_CLEARANCE_PX: f64 = 2.0;

/// Clearance (px) between a midway head and a label box.
const LABEL_CLEARANCE_PX: f64 = 4.0;
/// Steps (fractions of the path) tried when a midway head slides off a label, each way.
const SLIDE_STEPS: usize = 40;

/// Clearance (px) between a midway head and the stroke of another edge: a head on a crossing, or right beside
/// another rail, reads as belonging to either edge (T-413).
const STROKE_CLEARANCE_PX: f64 = 12.0;
/// Samples and longest step (px) used to follow the other strokes.
const STROKE_SAMPLES: usize = 64;
const STROKE_STEP_PX: f64 = 2.0;

/// What a midway head should stay clear of: label boxes, the strokes of the other edges and the heads already
/// placed on them.
struct Clearance<'a> {
    labels: &'a [Rect],
    strokes: Vec<Vec<ScreenPoint>>,
    heads: Vec<Vec<Rect>>,
}

impl Clearance<'_> {
    /// Clear of labels and of the other heads of its own edge (`siblings`) and, when `strict`, of the strokes
    /// and heads of every edge but `own`.
    fn admits(&self, head: &ArrowHead, own: usize, siblings: &[Rect], strict: bool) -> bool {
        let rect = head_rect(head);
        let near_siblings = rect.inflate(PAIR_CLEARANCE_PX);
        if siblings.iter().any(|sibling| sibling.overlap_area(&near_siblings, 0.0) > 0.0) {
            return false;
        }
        let near_labels = rect.inflate(LABEL_CLEARANCE_PX);
        if self.labels.iter().any(|label| label.overlap_area(&near_labels, 0.0) > 0.0) {
            return false;
        }
        if !strict {
            return true;
        }
        let near_strokes = rect.inflate(STROKE_CLEARANCE_PX);
        let others = |index: &usize| *index != own;
        let on_stroke = (0..self.strokes.len())
            .filter(others)
            .any(|index| self.strokes[index].iter().any(|point| near_strokes.contains(*point)));
        let on_head = (0..self.heads.len())
            .filter(others)
            .any(|index| self.heads[index].iter().any(|other| other.overlap_area(&near_strokes, 0.0) > 0.0));
        !on_stroke && !on_head
    }
}

/// Slides every midway head that lies under a label box (its own pill sits in the middle of the edge too), on
/// the stroke of another edge or on another midway head, along its edge to the nearest straight spot clear of
/// all of them (or, when there is none, clear of the labels). Runs after label placement.
pub fn clear_of_labels(edges: &mut [SceneEdge], labels: &[Rect], projection: &Projection) {
    let mut clearance = Clearance {
        labels,
        strokes: edges.iter().map(|edge| edge.path.sample(STROKE_SAMPLES, STROKE_STEP_PX)).collect(),
        heads: edges.iter().map(|edge| edge.midway.iter().map(head_rect).collect()).collect(),
    };
    for (index, edge) in edges.iter_mut().enumerate().filter(|(_, edge)| !edge.midway.is_empty()) {
        // One head at a time, each clear of where the others of its edge stand by then (T-414: a two-way edge
        // whose heads slid onto each other read as a "star").
        for head in 0..edge.midway.len() {
            let siblings: Vec<Rect> =
                edge.midway.iter().enumerate().filter(|(other, _)| *other != head).map(|(_, h)| head_rect(h)).collect();
            let mover = Mover { path: &edge.path, own: index, siblings: &siblings };
            edge.midway[head] = slide_off(&mover, edge.midway[head], &clearance, projection);
        }
        clearance.heads[index] = edge.midway.iter().map(head_rect).collect();
    }
}

/// Drops from the label placement's report the overlaps between a label and a midway head that
/// [`clear_of_labels`] has since moved away (the placement ran with the heads where they started), and the
/// `W_LABEL_OVERLAP` of a label left with no overlap. `label_rect` finds a label's box by its item.
pub fn forget_cleared_heads(
    report: &mut CollisionReport,
    diagnostics: &mut Diagnostics,
    edges: &[SceneEdge],
    label_rect: impl Fn(&ItemRef) -> Option<Rect>,
) {
    let still_touches = |label: &ItemRef, head_edge: &str| {
        let Some(rect) = label_rect(label) else { return true };
        edges
            .iter()
            .filter(|edge| edge.id.as_str() == head_edge)
            .flat_map(SceneEdge::painted_heads)
            .any(|head| rect.overlap_area(&head_rect(head), COLLISION_MARGIN_PX) > 0.0)
    };
    let stale = |overlap: &super::Overlap| {
        overlap.b.role == ItemRole::ArrowHead
            && is_label(overlap.a.role)
            && edges.iter().any(|edge| edge.id.as_str() == overlap.b.id && !edge.midway.is_empty())
            && !still_touches(&overlap.a, &overlap.b.id)
    };
    let (dropped, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut report.overlaps).into_iter().partition(stale);
    report.overlaps = kept;
    if dropped.is_empty() {
        return;
    }
    report.score = report.overlaps.iter().map(|overlap| overlap.area).sum();
    for label in dropped.iter().map(|overlap| &overlap.a) {
        let clean = report.overlaps.iter().all(|overlap| overlap.a.id != label.id || overlap.a.role != label.role);
        if clean {
            let prefix = format!("{} '{}' overlaps", label.role.as_str(), label.id);
            diagnostics.0.retain(|diagnostic| {
                diagnostic.code != DiagnosticCode::LabelOverlap || !diagnostic.message.starts_with(&prefix)
            });
        }
    }
}

/// Records the two midway heads of one edge that still overlap after [`clear_of_labels`] (no run left them
/// room): the overlap joins the collision score and raises a `W_LABEL_OVERLAP` on the edge (T-414).
pub fn report_head_pairs(
    report: &mut CollisionReport,
    diagnostics: &mut Diagnostics,
    model: &Model,
    edges: &[SceneEdge],
) {
    for edge in edges.iter().filter(|edge| edge.midway.len() > 1) {
        let rects: Vec<Rect> = edge.midway.iter().map(head_rect).collect();
        let area: f64 = rects
            .iter()
            .enumerate()
            .flat_map(|(index, first)| rects[index + 1..].iter().map(move |second| first.overlap_area(second, 0.0)))
            .sum();
        if area <= 0.0 {
            continue;
        }
        let head = ItemRef { role: ItemRole::ArrowHead, id: edge.id.to_string() };
        report.score += area;
        report.overlaps.push(super::Overlap { a: head.clone(), b: head, area });
        let path =
            model.edges.iter().find(|spec| spec.id == edge.id).map_or_else(String::new, |spec| spec.path.clone());
        diagnostics.push(
            Diagnostic::new(
                DiagnosticCode::LabelOverlap,
                path,
                format!("the two arrow heads of edge '{}' overlap ({area:.0} px²)", edge.id),
            )
            .with_hint("lengthen the edge or try view.rotation: auto"),
        );
    }
}

fn is_label(role: ItemRole) -> bool {
    matches!(role, ItemRole::NodeLabel | ItemRole::EdgeLabel | ItemRole::ZoneLabel)
}

fn head_rect(head: &ArrowHead) -> Rect {
    Rect::from_points([head.tip, head.base[0], head.base[1]])
}

/// The edge a midway head slides along: its path, its position among the edges and the boxes of its other
/// midway heads.
struct Mover<'a> {
    path: &'a EdgePath,
    own: usize,
    siblings: &'a [Rect],
}

fn slide_off(mover: &Mover<'_>, head: ArrowHead, clearance: &Clearance<'_>, projection: &Projection) -> ArrowHead {
    let (path, own, siblings) = (mover.path, mover.own, mover.siblings);
    if clearance.admits(&head, own, siblings, true) {
        return head;
    }
    let center = head.tip.midpoint(head.base[0].midpoint(head.base[1]));
    let forward = head.direction();
    let length = path.length().max(f64::EPSILON);
    let start = nearest_fraction(path, center);
    let head_length_px = projection.tile * ARROW_LENGTH * MIDWAY_HEAD_SCALE;
    let half_span = head_length_px / length;
    let candidates = (1..=SLIDE_STEPS).flat_map(|step| {
        let offset = step as f64 / SLIDE_STEPS as f64 * 0.5;
        [start + offset, start - offset]
    });
    let fractions: Vec<f64> = candidates.filter(|fraction| (half_span..=1.0 - half_span).contains(fraction)).collect();
    // Clear of everything on a straight piece, then clear of everything anywhere; then the same for labels only
    // (the head was there before strokes counted).
    for (strict, straight_only) in [(true, true), (true, false), (false, true), (false, false)] {
        if !strict && clearance.admits(&head, own, siblings, false) {
            return head;
        }
        for &fraction in &fractions {
            let (before, after) =
                (path.point_at_fraction(fraction - half_span), path.point_at_fraction(fraction + half_span));
            let middle = path.point_at_fraction(fraction);
            let along = after - before;
            let bent = (middle - before).angle_between_degrees(after - middle) > RUN_TURN_TOLERANCE_DEG;
            if along.length() <= f64::EPSILON || (straight_only && bent) {
                continue;
            }
            let sign = if along.x * forward.x + along.y * forward.y >= 0.0 { 1.0 } else { -1.0 };
            let candidate = floor_head(projection, middle, along.normalized() * sign);
            if clearance.admits(&candidate, own, siblings, strict) {
                return candidate;
            }
        }
    }
    head
}

/// Arc-length fraction of the path point nearest to `point`.
fn nearest_fraction(path: &EdgePath, point: ScreenPoint) -> f64 {
    (0..=SLIDE_STEPS * 4)
        .map(|step| step as f64 / (SLIDE_STEPS * 4) as f64)
        .min_by(|first, second| {
            path.point_at_fraction(*first).distance(point).total_cmp(&path.point_at_fraction(*second).distance(point))
        })
        .unwrap_or(0.5)
}

/// Where on the path the midway head goes: a screen point and the screen direction of travel there.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Spot {
    at: ScreenPoint,
    along: ScreenVector,
}

/// The midway heads of an edge whose end heads are `heads` (the target head points along the path, a
/// source head against it). Empty for an edge without heads. The two heads of a two-way edge stand apart,
/// pointing away from each other with a clear gap between their bases ([`PAIR_GAP_HEADS`]), on one run when
/// it takes both, else each on a run of its own (the source head nearer the source); with no room for that,
/// the edge keeps its end heads (T-414).
pub fn midway_heads(path: &EdgePath, heads: &[ArrowHead], projection: &Projection) -> Vec<ArrowHead> {
    if heads.is_empty() {
        return Vec::new();
    }
    let head_length_px = projection.tile * ARROW_LENGTH * MIDWAY_HEAD_SCALE;
    let start = path.point_at_fraction(0.0);
    let forward = |head: &ArrowHead| head.tip.distance(start) > head.tip.distance(path.point_at_fraction(1.0));
    let spots = if heads.len() > 1 {
        pair_spots(path, head_length_px)
    } else {
        middle_spot(path, head_length_px).map(|spot| [spot, spot])
    };
    let Some([source_spot, target_spot]) = spots else {
        return Vec::new();
    };
    let shared = source_spot == target_spot && heads.len() > 1;
    let shift = if shared { head_length_px * (0.5 + PAIR_GAP_HEADS / 2.0) } else { 0.0 };
    heads
        .iter()
        .map(|head| {
            let (spot, sign) = if forward(head) { (target_spot, 1.0) } else { (source_spot, -1.0) };
            let along = spot.along * sign;
            floor_head(projection, spot.at + along * shift, along)
        })
        .collect()
}

/// Where the two heads of a two-way edge go: `[source head, target head]`, the same spot when one run takes
/// both (they are then pushed apart along it), else the middles of two runs of their own. `None` when the path
/// has no room for two heads (curves always do: their middle).
fn pair_spots(path: &EdgePath, head_length_px: f64) -> Option<[Spot; 2]> {
    let EdgePath::Polyline(points) = path else {
        return path_middle(path).map(|spot| [spot, spot]);
    };
    let pair_length = head_length_px * (2.0 + PAIR_GAP_HEADS + 2.0 * PAIR_RUN_MARGIN_HEADS);
    if let Some(spot) = run_spot(points, head_length_px, pair_length) {
        return Some([spot, spot]);
    }
    let runs = straight_runs(points);
    let mut fitting: Vec<&Run> = runs.iter().filter(|run| run.length >= head_length_px * MIN_RUN_HEADS).collect();
    fitting.sort_by(|a, b| b.length.total_cmp(&a.length).then(a.start_offset.total_cmp(&b.start_offset)));
    let [first, second] = [*fitting.first()?, *fitting.get(1)?];
    let (near_source, near_target) =
        if first.start_offset <= second.start_offset { (first, second) } else { (second, first) };
    Some([near_source.middle(), near_target.middle()])
}

/// The middle of the straight run nearest the middle of the path among the longest ones (polylines; none
/// without a run long enough), or the middle of the path (curves).
fn middle_spot(path: &EdgePath, head_length_px: f64) -> Option<Spot> {
    match path {
        // No straight run long enough (a short or hairpin rail): the head stays at the end.
        EdgePath::Polyline(points) => run_spot(points, head_length_px, head_length_px * MIN_RUN_HEADS),
        EdgePath::Curve(_) => path_middle(path),
    }
}

/// The middle of the run nearest the middle of the path among the longest ones, when the longest is at least
/// `shortest` px.
fn run_spot(points: &[ScreenPoint], head_length_px: f64, shortest: f64) -> Option<Spot> {
    let runs = straight_runs(points);
    let longest = runs.iter().map(|run| run.length).fold(0.0, f64::max);
    if longest < shortest.max(head_length_px * MIN_RUN_HEADS) {
        return None;
    }
    let total: f64 = points.windows(2).map(|pair| pair[0].distance(pair[1])).sum();
    let run = runs.iter().filter(|run| run.length >= longest * LONG_RUN_SHARE && run.length >= shortest).min_by(
        |first, second| {
            let off_middle = |run: &&Run| (run.start_offset + run.length / 2.0 - total / 2.0).abs();
            off_middle(first).total_cmp(&off_middle(second))
        },
    )?;
    Some(run.middle())
}

fn path_middle(path: &EdgePath) -> Option<Spot> {
    let (before, after) = (path.point_at_fraction(0.49), path.point_at_fraction(0.51));
    let along = after - before;
    (along.length() > f64::EPSILON).then(|| Spot { at: path.point_at_fraction(0.5), along: along.normalized() })
}

/// A straight run of a polyline: consecutive collinear legs merged (rails polylines are densely sampled).
#[derive(Debug, Clone, Copy)]
struct Run {
    from: ScreenPoint,
    to: ScreenPoint,
    length: f64,
    /// Arc length from the start of the path to `from`.
    start_offset: f64,
}

impl Run {
    /// The middle of the run and its direction.
    fn middle(&self) -> Spot {
        Spot { at: self.from + (self.to - self.from) * 0.5, along: (self.to - self.from).normalized() }
    }
}

/// Largest turn (degrees) between two legs that still belong to the same run.
const RUN_TURN_TOLERANCE_DEG: f64 = 1.0;

fn straight_runs(points: &[ScreenPoint]) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut offset = 0.0;
    for pair in points.windows(2) {
        let leg = pair[1] - pair[0];
        let length = leg.length();
        if length <= f64::EPSILON {
            continue;
        }
        let continues = runs.last().is_some_and(|run| {
            run.to.distance(pair[0]) < 1e-6 && (run.to - run.from).angle_between_degrees(leg) <= RUN_TURN_TOLERANCE_DEG
        });
        match runs.last_mut() {
            Some(run) if continues => {
                run.to = pair[1];
                run.length += length;
            }
            _ => runs.push(Run { from: pair[0], to: pair[1], length, start_offset: offset }),
        }
        offset += length;
    }
    runs
}

/// A head lying on the floor, centered at screen `at` (on the stroke, at [`EDGE_Z`]), pointing along the
/// screen direction `along`: built in the plan and projected, like every head.
fn floor_head(projection: &Projection, at: ScreenPoint, along: ScreenVector) -> ArrowHead {
    let center = projection.unproject(at, EDGE_Z);
    let ahead = projection.unproject(at + along, EDGE_Z);
    let aim = plan_direction(center, ahead);
    let half_length = ARROW_LENGTH * MIDWAY_HEAD_SCALE / 2.0;
    let side = aim.perpendicular() * (ARROW_HALF_WIDTH * MIDWAY_HEAD_SCALE);
    let tip = center + aim * half_length;
    let base = center + aim * -half_length;
    let project = |point: PlanPoint| projection.project(point, EDGE_Z);
    ArrowHead {
        tip: project(tip),
        base: [project(base + side), project(base + side * -1.0)],
        target_center: project(center + aim * (2.0 * half_length)),
    }
}

fn plan_direction(from: PlanPoint, to: PlanPoint) -> PlanVector {
    PlanVector::new(to.x - from.x, to.y - from.y).normalized()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head_at(tip: ScreenPoint) -> ArrowHead {
        ArrowHead { tip, base: [tip, tip], target_center: tip }
    }

    #[test]
    fn the_head_sits_in_the_middle_of_the_longest_run_pointing_forward() {
        let projection = Projection::new(56.0);
        // An L: a short first leg, a long second one.
        let points = vec![ScreenPoint::new(0.0, 0.0), ScreenPoint::new(40.0, 0.0), ScreenPoint::new(40.0, 300.0)];
        let path = EdgePath::Polyline(points);
        let heads = midway_heads(&path, &[head_at(ScreenPoint::new(40.0, 310.0))], &projection);
        assert_eq!(heads.len(), 1);
        let head = heads[0];
        let middle = head.tip.midpoint(head.base[0].midpoint(head.base[1]));
        assert!((middle.x - 40.0).abs() < 0.5 && (middle.y - 150.0).abs() < 0.5, "{middle:?}");
        assert!(head.direction().y > 0.0, "points down the run, toward the target");
    }

    #[test]
    fn a_source_head_points_back_and_two_heads_do_not_overlap() {
        let projection = Projection::new(56.0);
        let path = EdgePath::Polyline(vec![ScreenPoint::new(0.0, 0.0), ScreenPoint::new(400.0, 0.0)]);
        let ends = [head_at(ScreenPoint::new(410.0, 0.0)), head_at(ScreenPoint::new(-10.0, 0.0))];
        let heads = midway_heads(&path, &ends, &projection);
        assert!(heads[0].direction().x > 0.0 && heads[1].direction().x < 0.0);
        let span = |head: &ArrowHead| {
            let xs = [head.tip.x, head.base[0].x, head.base[1].x];
            (xs.iter().copied().fold(f64::INFINITY, f64::min), xs.iter().copied().fold(f64::NEG_INFINITY, f64::max))
        };
        let (first, second) = (span(&heads[0]), span(&heads[1]));
        assert!(first.0 >= second.1 || second.0 >= first.1, "{first:?} {second:?}");
    }

    #[test]
    fn two_way_heads_on_one_run_stand_apart_with_a_gap() {
        let projection = Projection::new(56.0);
        let path = EdgePath::Polyline(vec![ScreenPoint::new(0.0, 0.0), ScreenPoint::new(400.0, 0.0)]);
        let ends = [head_at(ScreenPoint::new(410.0, 0.0)), head_at(ScreenPoint::new(-10.0, 0.0))];
        let heads = midway_heads(&path, &ends, &projection);
        let base_x = |head: &ArrowHead| head.base[0].midpoint(head.base[1]).x;
        let gap = base_x(&heads[0]) - base_x(&heads[1]);
        let head_length = projection.tile * ARROW_LENGTH * MIDWAY_HEAD_SCALE;
        assert!(gap >= head_length * 0.5 - 1e-6, "bases {gap:.1} px apart (head {head_length:.1} px)");
        assert!(heads[0].tip.x > base_x(&heads[0]) && heads[1].tip.x < base_x(&heads[1]), "pointing apart");
    }

    #[test]
    fn two_way_heads_take_two_runs_when_one_is_too_short() {
        let projection = Projection::new(56.0);
        let head_length = projection.tile * ARROW_LENGTH * MIDWAY_HEAD_SCALE;
        let run = head_length * (MIN_RUN_HEADS + 0.2);
        let path = EdgePath::Polyline(vec![
            ScreenPoint::new(0.0, 0.0),
            ScreenPoint::new(run, 0.0),
            ScreenPoint::new(run, run),
        ]);
        let ends = [head_at(ScreenPoint::new(run, run + 10.0)), head_at(ScreenPoint::new(-10.0, 0.0))];
        let heads = midway_heads(&path, &ends, &projection);
        assert_eq!(heads.len(), 2);
        let middle = |head: &ArrowHead| head.tip.midpoint(head.base[0].midpoint(head.base[1]));
        assert!((middle(&heads[1]).y).abs() < 1.0, "the source head on the first run: {:?}", middle(&heads[1]));
        assert!((middle(&heads[0]).x - run).abs() < 1.0, "the target head on the second run: {:?}", middle(&heads[0]));
        assert_eq!(head_rect(&heads[0]).overlap_area(&head_rect(&heads[1]), 0.0), 0.0);
    }

    #[test]
    fn a_short_two_way_rail_keeps_its_end_heads() {
        let projection = Projection::new(56.0);
        let head_length = projection.tile * ARROW_LENGTH * MIDWAY_HEAD_SCALE;
        let path = EdgePath::Polyline(vec![ScreenPoint::new(0.0, 0.0), ScreenPoint::new(head_length * 3.0, 0.0)]);
        let ends = [head_at(ScreenPoint::new(head_length * 3.0 + 10.0, 0.0)), head_at(ScreenPoint::new(-10.0, 0.0))];
        assert!(midway_heads(&path, &ends, &projection).is_empty());
    }

    #[test]
    fn the_heads_of_a_two_way_edge_never_meet_in_any_rotation() {
        // T-414: `phone <-> ws` drew both heads base to base in the middle of a short rail.
        const SPEC: &str = "diorama: 1\ntheme: schematic\nnodes:\n  - { id: phone, icon: mobile }\n  \
                            - { id: ws, icon: loadbalancer }\n  - { id: chat, icon: server }\n\
                            edges:\n  - phone <-> ws\n  - ws -> chat\n";
        for rotation in dioramic_spec::Rotation::ALL {
            let options = crate::CompileOptions {
                rotation: Some(dioramic_spec::RotationSpec::Fixed(rotation)),
                ..crate::CompileOptions::default()
            };
            let diagram = crate::compile_source(SPEC, dioramic_spec::SourceFormat::Yaml, &options)
                .unwrap_or_else(|errors| panic!("{errors:?}"));
            let edge = &diagram.scene.edges[0];
            let painted: Vec<Rect> = edge.painted_heads().iter().map(head_rect).collect();
            assert_eq!(painted.len(), 2, "{rotation:?}");
            assert_eq!(painted[0].overlap_area(&painted[1], 0.0), 0.0, "{rotation:?}: {painted:?}");
            let lint = diagram.diagnostics.iter().any(|d| d.message.contains("arrow heads of edge"));
            assert!(!lint, "{rotation:?}");
        }
    }

    #[test]
    fn heads_of_one_edge_left_on_top_of_each_other_are_reported() {
        const SPEC: &str = "diorama: 1\nnodes:\n  - { id: a }\n  - { id: b }\nedges:\n  - a <-> b\n";
        let diagram = crate::compile_source(SPEC, dioramic_spec::SourceFormat::Yaml, &crate::CompileOptions::default())
            .unwrap_or_else(|errors| panic!("{errors:?}"));
        let model = dioramic_spec::load(SPEC, dioramic_spec::SourceFormat::Yaml)
            .unwrap_or_else(|errors| panic!("{errors:?}"))
            .model;
        let mut edges = diagram.scene.edges.clone();
        let head = floor_head(&Projection::new(56.0), ScreenPoint::new(0.0, 0.0), ScreenVector::new(1.0, 0.0));
        edges[0].midway = vec![head, head];
        let (mut report, mut diagnostics) = (CollisionReport::default(), Diagnostics::new());
        report_head_pairs(&mut report, &mut diagnostics, &model, &edges);
        assert!(report.score > 0.0 && report.overlaps.len() == 1);
        assert!(
            diagnostics.iter().any(|d| d.code == DiagnosticCode::LabelOverlap && d.path == "edges[0]"),
            "{diagnostics:?}"
        );
        edges[0].midway = Vec::new();
        let (mut report, mut diagnostics) = (CollisionReport::default(), Diagnostics::new());
        report_head_pairs(&mut report, &mut diagnostics, &model, &edges);
        assert_eq!(report.score, 0.0);
    }

    #[test]
    fn a_short_rail_keeps_its_end_head() {
        let path = EdgePath::Polyline(vec![ScreenPoint::new(0.0, 0.0), ScreenPoint::new(20.0, 0.0)]);
        let ends = [head_at(ScreenPoint::new(30.0, 0.0))];
        assert!(midway_heads(&path, &ends, &Projection::new(56.0)).is_empty());
    }

    #[test]
    fn an_edge_without_heads_gets_none() {
        let path = EdgePath::Polyline(vec![ScreenPoint::new(0.0, 0.0), ScreenPoint::new(400.0, 0.0)]);
        assert!(midway_heads(&path, &[], &Projection::new(56.0)).is_empty());
    }
}
