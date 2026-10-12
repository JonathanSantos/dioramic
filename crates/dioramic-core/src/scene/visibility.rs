//! How much of an edge the reader actually sees (T-310, T-312).
//!
//! Edges are painted before nodes, so every stretch of a stroke that falls inside a node silhouette is
//! hidden, whether the node stands in front of it or the stroke runs "behind" it. On top of that two
//! strokes that run on top of each other read as one. These measures drive the route choice
//! (`edges.rs`), the pill rule (`labels.rs`) and the edge quality used by `rotation: auto`.

use crate::geometry::{Rect, ScreenPoint, ScreenVector};

use super::EdgePath;
use super::nodes::{inside_convex, outside_distance};

/// Segments sampled along a curve for the measures (uniform in `t`).
pub const MEASURE_SAMPLES: usize = 28;
/// Longest polyline piece between two samples (px).
pub const MEASURE_MAX_STEP_PX: f64 = 8.0;
/// Depth inside a silhouette (px) for a sample to count as covered, so grazing a corner is not reported.
pub const COVER_INSET_PX: f64 = 4.0;
/// Two strokes closer than this (px) and nearly parallel read as one.
pub const COINCIDENT_DISTANCE_PX: f64 = 4.0;
/// Cosine above which two stroke pieces count as parallel (≈ 25°): a crossing is not a coincidence.
const PARALLEL_COSINE: f64 = 0.9;
/// Shortest visible stretch (px) for an edge to read as a line between two nodes.
pub const MIN_VISIBLE_EDGE_PX: f64 = 24.0;
/// A visible stroke closer than this (px) to a node silhouette runs along its outline: with the outline,
/// the soft shadow and the stroke width it reads as passing under the node's base or behind it (T-381).
pub const GRAZE_CLEARANCE_PX: f64 = 10.0;
/// Crossings this close (px) to an end of either stroke are not counted ([`crossing_count`]).
pub const CROSSING_END_CLEARANCE_PX: f64 = 2.0;

/// A sampled stroke and its bounds, measured many times while routes are compared.
#[derive(Debug, Clone, PartialEq)]
pub struct Trace {
    /// Points along the stroke.
    pub points: Vec<ScreenPoint>,
    /// Bounding box of the points.
    pub bounds: Rect,
    /// Bounds of each piece grown by [`COINCIDENT_DISTANCE_PX`] (cheap reject in [`coincident_length`]).
    reach: Vec<Rect>,
    /// Bounds of each run of [`TRACE_BLOCK`] pieces, so a measure can skip a whole run far from what it
    /// looks for (block `i` covers pieces `i·B .. (i+1)·B`).
    blocks: Vec<Rect>,
}

/// Pieces per block of [`Trace::blocks`].
pub(crate) const TRACE_BLOCK: usize = 8;

impl Trace {
    /// Samples `path`.
    pub fn of(path: &EdgePath) -> Self {
        Self::from_points(path.sample(MEASURE_SAMPLES, MEASURE_MAX_STEP_PX))
    }

    /// A straight segment cut into pieces no longer than [`MEASURE_MAX_STEP_PX`] (leader lines).
    pub fn segment(from: ScreenPoint, to: ScreenPoint) -> Self {
        let pieces = (from.distance(to) / MEASURE_MAX_STEP_PX).ceil().max(1.0) as usize;
        Self::from_points((0..=pieces).map(|index| from + (to - from) * (index as f64 / pieces as f64)).collect())
    }

    fn from_points(points: Vec<ScreenPoint>) -> Self {
        let bounds = Rect::from_points(points.iter().copied());
        let reach = points
            .windows(2)
            .map(|pair| Rect::from_points([pair[0], pair[1]]).inflate(COINCIDENT_DISTANCE_PX))
            .collect();
        let pieces = points.len().saturating_sub(1);
        let blocks = (0..pieces.div_ceil(TRACE_BLOCK))
            .map(|block| {
                let last = ((block + 1) * TRACE_BLOCK).min(pieces);
                Rect::from_points(points[block * TRACE_BLOCK..=last].iter().copied())
            })
            .collect();
        Self { points, bounds, reach, blocks }
    }

    /// Bounds of each run of [`TRACE_BLOCK`] pieces.
    pub(crate) fn blocks(&self) -> &[Rect] {
        &self.blocks
    }

    /// Points of the pieces of block `block` (one more than the pieces).
    pub(crate) fn block_points(&self, block: usize) -> &[ScreenPoint] {
        let pieces = self.points.len().saturating_sub(1);
        let last = ((block + 1) * TRACE_BLOCK).min(pieces);
        &self.points[block * TRACE_BLOCK..=last]
    }

    /// Pieces of the stroke: `(midpoint, direction, length)`.
    pub(crate) fn pieces(&self) -> impl Iterator<Item = (ScreenPoint, ScreenVector, f64)> + '_ {
        self.points.windows(2).map(|pair| (pair[0].midpoint(pair[1]), pair[1] - pair[0], pair[0].distance(pair[1])))
    }
}

/// A node silhouette with its bounds (pre-filter for the convex test).
#[derive(Debug, Clone, Copy)]
pub struct Cover<'a> {
    /// Convex polygon.
    pub polygon: &'a [ScreenPoint],
    /// Its bounding box.
    pub bounds: Rect,
}

impl<'a> Cover<'a> {
    /// Cover of a silhouette.
    pub fn new(polygon: &'a [ScreenPoint]) -> Self {
        Self { polygon, bounds: Rect::from_points(polygon.iter().copied()) }
    }

    /// True when `point` is inside the silhouette (deeper than [`COVER_INSET_PX`]).
    pub(crate) fn hides(&self, point: ScreenPoint) -> bool {
        self.bounds.contains(point) && inside_convex(self.polygon, point, COVER_INSET_PX)
    }

    /// [`Cover::grazes`] for a point already known not to be hidden by this cover.
    pub(crate) fn hugs(&self, point: ScreenPoint) -> bool {
        self.bounds.inflate(GRAZE_CLEARANCE_PX).contains(point)
            && outside_distance(self.polygon, point) < GRAZE_CLEARANCE_PX
    }

    /// True when `point` is not hidden but lies within [`GRAZE_CLEARANCE_PX`] of the silhouette.
    pub(crate) fn grazes(&self, point: ScreenPoint) -> bool {
        self.bounds.inflate(GRAZE_CLEARANCE_PX).contains(point)
            && outside_distance(self.polygon, point) < GRAZE_CLEARANCE_PX
            && !self.hides(point)
    }
}

/// Length (px) of the stroke hidden by `cover`.
pub fn covered_length(trace: &Trace, cover: &Cover<'_>) -> f64 {
    if !touches(&trace.bounds, &cover.bounds) {
        return 0.0;
    }
    trace.pieces().filter(|(middle, _, _)| cover.hides(*middle)).map(|(_, _, length)| length).sum()
}

/// Length (px) of the stroke running along `cover` without entering it ([`Cover::grazes`]).
pub fn grazing_length(trace: &Trace, cover: &Cover<'_>) -> f64 {
    if !touches(&trace.bounds, &cover.bounds.inflate(GRAZE_CLEARANCE_PX)) {
        return 0.0;
    }
    trace.pieces().filter(|(middle, _, _)| cover.grazes(*middle)).map(|(_, _, length)| length).sum()
}

/// Length (px) of the stroke outside every cover and every extra box (labels drawn over it).
pub fn visible_length(trace: &Trace, covers: &[Cover<'_>], boxes: &[Rect]) -> f64 {
    trace
        .pieces()
        .filter(|(middle, _, _)| {
            covers.iter().all(|cover| !cover.hides(*middle)) && boxes.iter().all(|rect| !rect.contains(*middle))
        })
        .map(|(_, _, length)| length)
        .sum()
}

/// Length (px) of `trace` that runs on top of `other`: closer than [`COINCIDENT_DISTANCE_PX`] and parallel.
pub fn coincident_length(trace: &Trace, other: &Trace) -> f64 {
    if !touches(&trace.bounds.inflate(COINCIDENT_DISTANCE_PX), &other.bounds) {
        return 0.0;
    }
    trace
        .pieces()
        .filter(|(middle, direction, _)| runs_along(*middle, *direction, other))
        .map(|(_, _, length)| length)
        .sum()
}

/// Number of points where `trace` crosses `other` (T-400). Crossings closer than [`CROSSING_END_CLEARANCE_PX`]
/// to an end of either stroke are not counted: two strokes leaving the same port area meet there by design.
pub fn crossing_count(trace: &Trace, other: &Trace) -> usize {
    if !touches(&trace.bounds, &other.bounds) {
        return 0;
    }
    let ends = [trace.points.first(), trace.points.last(), other.points.first(), other.points.last()];
    let near_an_end =
        |point: ScreenPoint| ends.iter().flatten().any(|end| end.distance(point) < CROSSING_END_CLEARANCE_PX);
    let mut count = 0;
    // Blocks of pieces far apart are skipped whole; the pieces of two blocks that meet are compared as before
    // (a count does not depend on the order the pairs are visited in).
    let grown = |rect: &Rect| rect.inflate(COINCIDENT_DISTANCE_PX);
    for (block, block_bounds) in trace.blocks.iter().enumerate() {
        if !touches(&grown(block_bounds), &grown(&other.bounds)) {
            continue;
        }
        let first = block * TRACE_BLOCK;
        for (other_block, other_bounds) in other.blocks.iter().enumerate() {
            if !touches(&grown(block_bounds), &grown(other_bounds)) {
                continue;
            }
            let other_first = other_block * TRACE_BLOCK;
            let pieces = trace.block_points(block).windows(2).zip(&trace.reach[first..]);
            for (pair, reach) in pieces {
                let other_pieces = other.block_points(other_block).windows(2).zip(&other.reach[other_first..]);
                for (other_pair, other_reach) in other_pieces {
                    if !touches(reach, other_reach) {
                        continue;
                    }
                    if let Some(point) = segment_crossing(pair[0], pair[1], other_pair[0], other_pair[1])
                        && !near_an_end(point)
                    {
                        count += 1;
                    }
                }
            }
        }
    }
    count
}

/// Crossing point of segments `a0`–`a1` and `b0`–`b1`, half-open at the far end of each (`t, u ∈ [0, 1)`),
/// so a crossing exactly at a shared vertex of a polyline is counted once. Parallel segments never cross.
fn segment_crossing(a0: ScreenPoint, a1: ScreenPoint, b0: ScreenPoint, b1: ScreenPoint) -> Option<ScreenPoint> {
    let along_a = a1 - a0;
    let along_b = b1 - b0;
    let denominator = along_a.x * along_b.y - along_a.y * along_b.x;
    if denominator.abs() <= f64::EPSILON {
        return None;
    }
    let offset = b0 - a0;
    let t = (offset.x * along_b.y - offset.y * along_b.x) / denominator;
    let u = (offset.x * along_a.y - offset.y * along_a.x) / denominator;
    ((0.0..1.0).contains(&t) && (0.0..1.0).contains(&u)).then(|| a0 + along_a * t)
}

/// True when a piece of `other` passes by `point`, parallel to `direction`.
fn runs_along(point: ScreenPoint, direction: ScreenVector, other: &Trace) -> bool {
    if !other.bounds.inflate(COINCIDENT_DISTANCE_PX).contains(point) {
        return false;
    }
    let direction = direction.normalized();
    let near = |block: &Rect| block.inflate(COINCIDENT_DISTANCE_PX).contains(point);
    let candidates = other
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, block)| near(block))
        .flat_map(|(block, _)| other.block_points(block).windows(2).zip(&other.reach[block * TRACE_BLOCK..]));
    candidates.into_iter().any(|(pair, reach)| {
        if !reach.contains(point) {
            return false;
        }
        let piece = pair[1] - pair[0];
        let length = piece.length();
        if length <= f64::EPSILON {
            return false;
        }
        let cosine = (piece.x * direction.x + piece.y * direction.y) / length;
        cosine.abs() >= PARALLEL_COSINE && distance_to_segment(point, pair[0], pair[1]) < COINCIDENT_DISTANCE_PX
    })
}

/// True when two boxes share a point, borders included. Unlike [`Rect::intersects`] it holds for flat
/// boxes too: the bounds of a horizontal stroke or a vertical leader have no area.
pub fn touches(a: &Rect, b: &Rect) -> bool {
    !a.is_empty() && !b.is_empty() && a.x0 <= b.x1 && b.x0 <= a.x1 && a.y0 <= b.y1 && b.y0 <= a.y1
}

/// Distance from `point` to the segment `a`–`b`.
pub fn distance_to_segment(point: ScreenPoint, a: ScreenPoint, b: ScreenPoint) -> f64 {
    squared_distance_to_segment(point, a, b).sqrt()
}

/// Square of [`distance_to_segment`] (its root is exactly that distance).
pub fn squared_distance_to_segment(point: ScreenPoint, a: ScreenPoint, b: ScreenPoint) -> f64 {
    let segment = b - a;
    let squared = segment.x * segment.x + segment.y * segment.y;
    let offset = point - a;
    let nearest = if squared <= f64::EPSILON {
        a
    } else {
        let t = ((offset.x * segment.x + offset.y * segment.y) / squared).clamp(0.0, 1.0);
        a + segment * t
    };
    let gap = point - nearest;
    gap.x * gap.x + gap.y * gap.y
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::CubicBezier;

    fn segment(from: (f64, f64), to: (f64, f64)) -> Trace {
        let (a, b) = (ScreenPoint::new(from.0, from.1), ScreenPoint::new(to.0, to.1));
        Trace::of(&EdgePath::Curve(CubicBezier::new(a, a + (b - a) * (1.0 / 3.0), a + (b - a) * (2.0 / 3.0), b)))
    }

    fn square(x0: f64, y0: f64, size: f64) -> Vec<ScreenPoint> {
        vec![
            ScreenPoint::new(x0, y0),
            ScreenPoint::new(x0 + size, y0),
            ScreenPoint::new(x0 + size, y0 + size),
            ScreenPoint::new(x0, y0 + size),
        ]
    }

    #[test]
    fn collinear_strokes_coincide_and_crossings_do_not() {
        let trunk = segment((0.0, 0.0), (200.0, 0.0));
        let tail = segment((100.0, 1.0), (200.0, 1.0));
        assert!((coincident_length(&tail, &trunk) - 100.0).abs() < 3.0);
        let crossing = segment((150.0, -50.0), (150.0, 50.0));
        assert!(coincident_length(&crossing, &trunk) < COINCIDENT_DISTANCE_PX * 2.0);
        let apart = segment((0.0, 20.0), (200.0, 20.0));
        assert_eq!(coincident_length(&apart, &trunk), 0.0);
    }

    #[test]
    fn visible_length_skips_silhouettes_and_boxes() {
        let edge = segment((0.0, 50.0), (200.0, 50.0));
        let node = square(0.0, 0.0, 100.0);
        let covers = [Cover::new(&node)];
        let visible = visible_length(&edge, &covers, &[]);
        // Pieces within the cover inset of the border still count as visible.
        assert!((visible - 100.0).abs() < 16.0, "{visible}");
        let pill = Rect::new(120.0, 40.0, 180.0, 60.0);
        let under_pill = visible_length(&edge, &covers, &[pill]);
        assert!((visible - under_pill - 60.0).abs() < 8.0, "{under_pill}");
        assert!((covered_length(&edge, &covers[0]) - 100.0).abs() < 16.0);
    }

    #[test]
    fn grazing_counts_strokes_hugging_a_silhouette_from_outside() {
        let node = square(0.0, 0.0, 100.0);
        let cover = Cover::new(&node);
        let hugging = segment((0.0, 104.0), (100.0, 104.0));
        assert!((grazing_length(&hugging, &cover) - 100.0).abs() < 8.0);
        let clear = segment((0.0, 100.0 + GRAZE_CLEARANCE_PX + 2.0), (100.0, 100.0 + GRAZE_CLEARANCE_PX + 2.0));
        assert_eq!(grazing_length(&clear, &cover), 0.0);
        let through = segment((-50.0, 50.0), (150.0, 50.0));
        let entering = grazing_length(&through, &cover);
        assert!(entering < 2.0 * (GRAZE_CLEARANCE_PX + COVER_INSET_PX) + 2.0 * MEASURE_MAX_STEP_PX, "{entering}");
    }

    #[test]
    fn crossings_count_each_meeting_once_and_ignore_shared_ends() {
        let horizontal = segment((0.0, 0.0), (200.0, 0.0));
        assert_eq!(crossing_count(&segment((100.0, -50.0), (100.0, 50.0)), &horizontal), 1);
        assert_eq!(crossing_count(&segment((0.0, 10.0), (200.0, 10.0)), &horizontal), 0);
        // Leaving from the same port is not a crossing; neither is touching the other stroke's end.
        assert_eq!(crossing_count(&segment((0.0, 0.0), (150.0, 80.0)), &horizontal), 0);
        assert_eq!(crossing_count(&segment((200.0, 0.0), (250.0, 80.0)), &horizontal), 0);
        // A wave across the line crosses it twice.
        let wave = Trace::from_points(vec![
            ScreenPoint::new(0.0, -20.0),
            ScreenPoint::new(60.0, 20.0),
            ScreenPoint::new(120.0, -20.0),
        ]);
        assert_eq!(crossing_count(&wave, &horizontal), 2);
        assert_eq!(crossing_count(&horizontal, &wave), 2);
    }
}
