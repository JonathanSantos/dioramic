//! Where the reading starts (T-415): the hub layout puts the busiest places in the middle and lets the sources
//! fall where the stress puts them, sometimes at the end of the reading (a pipeline in U puts its clients on the
//! right, and the upload reads right to left). A mirror of the whole drawing keeps every edge length, crossing
//! and alignment: the hub layout keeps the mirror whose sources come first along `layout.flow` (on screen at
//! rotation 0, the reading order of the default flow), among the mirrors that cost about as much to draw. With
//! rails it reads the screen itself (sources up and to the left first) and counts the rails whose bends would
//! turn back on themselves (T-425).

use crate::geometry::{COS_30, PlanPoint, PlanRect, SIN_30};

/// How much more than the cheapest mirror a mirror may cost and still be chosen for its reading (share of the
/// cost): mirrors only change the size of the picture's screen box, a little.
pub(super) const MIRROR_COST_SLACK: f64 = 0.03;

/// A mirror of the plan: `(x, y)` → `(sign_x · x, sign_y · y)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Mirror {
    sign_x: f64,
    sign_y: f64,
}

impl Mirror {
    /// The four mirrors, the identity first (it wins ties).
    pub(super) const ALL: [Mirror; 4] = [
        Mirror { sign_x: 1.0, sign_y: 1.0 },
        Mirror { sign_x: -1.0, sign_y: 1.0 },
        Mirror { sign_x: 1.0, sign_y: -1.0 },
        Mirror { sign_x: -1.0, sign_y: -1.0 },
    ];

    /// `+1` when the mirror keeps the sign of `x · y` (identity, both axes flipped), `−1` when it flips it.
    pub(super) fn sign_product(self) -> f64 {
        self.sign_x * self.sign_y
    }

    /// `point` mirrored.
    pub(super) fn point(self, point: PlanPoint) -> PlanPoint {
        PlanPoint::new(self.sign_x * point.x, self.sign_y * point.y)
    }

    /// `rect` mirrored (corners renormalized).
    pub(super) fn rect(self, rect: PlanRect) -> PlanRect {
        let (low, high) = (self.point(PlanPoint::new(rect.x0, rect.y0)), self.point(PlanPoint::new(rect.x1, rect.y1)));
        PlanRect::new(low.x, low.y, high.x, high.y)
    }
}

/// Where the sources of a drawing stand in the reading order of the screen at rotation 0, from 0 (the centroid
/// of the sources is the first thing read: top left) to 1 (the last: bottom right). Reading runs along screen
/// `x + y`. `centers` are the plan centers of every node (rotation 0), `sources` the positions in it of the nodes
/// no edge enters. `None` without sources or when every node reads at once.
pub fn source_reading_rank(centers: &[PlanPoint], sources: &[usize]) -> Option<f64> {
    if sources.is_empty() || centers.is_empty() {
        return None;
    }
    let reading = |point: PlanPoint| (point.x - point.y) * COS_30 + (point.x + point.y) * SIN_30;
    let (first, last) = centers
        .iter()
        .map(|&center| reading(center))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), value| (low.min(value), high.max(value)));
    let span = last - first;
    if span <= f64::EPSILON {
        return None;
    }
    let centroid = sources.iter().map(|&source| reading(centers[source])).sum::<f64>() / sources.len() as f64;
    Some((centroid - first) / span)
}

/// Weight of where the sources stand across the flow next to where they stand along it: only breaks ties
/// between mirrors (the screen at rotation 0 reads a little from the larger flow-frame `y` first).
const ACROSS_TIE_WEIGHT: f64 = 0.05;

/// Where the sources stand in the reading of the flow frame, about 0 when their centroid is at the start of the
/// flow (the smallest `x` of any node) and 1 at its end, with [`ACROSS_TIE_WEIGHT`] for where they stand across
/// it (larger `y` first). `centers` are in the flow frame. `None` without sources.
pub(super) fn source_flow_rank(centers: &[PlanPoint], sources: &[usize]) -> Option<f64> {
    if sources.is_empty() || centers.is_empty() {
        return None;
    }
    let along = share(centers.iter().map(|center| center.x), sources.iter().map(|&source| centers[source].x));
    let across = share(centers.iter().map(|center| -center.y), sources.iter().map(|&source| -centers[source].y));
    Some(along + ACROSS_TIE_WEIGHT * across)
}

/// Where the mean of `picked` stands between the smallest and the largest of `all` (0 when they coincide).
fn share(all: impl Iterator<Item = f64>, picked: impl Iterator<Item = f64>) -> f64 {
    let (first, last) =
        all.fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), value| (low.min(value), high.max(value)));
    let (sum, count) = picked.fold((0.0, 0_u32), |(sum, count), value| (sum + value, count + 1));
    let span = last - first;
    if span <= f64::EPSILON || count == 0 { 0.0 } else { (sum / f64::from(count) - first) / span }
}

/// Positions of the nodes that no edge enters (self-loops aside), from `(from, to)` edges over `count` nodes.
/// Empty when every node is entered (a cycle) or none leaves.
pub(super) fn sources(edges: &[(usize, usize)], count: usize) -> Vec<usize> {
    let mut entered = vec![false; count];
    let mut leaves = vec![false; count];
    for &(from, to) in edges.iter().filter(|(from, to)| from != to) {
        entered[to] = true;
        leaves[from] = true;
    }
    (0..count).filter(|&node| !entered[node] && leaves[node]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_on_the_top_left_reads_first() {
        // Along plan x the screen reading coordinate (x + y on screen) grows: the first node reads first.
        let centers = [PlanPoint::new(0.0, 0.0), PlanPoint::new(4.0, 0.0), PlanPoint::new(8.0, 0.0)];
        assert_eq!(source_reading_rank(&centers, &[0]), Some(0.0));
        assert_eq!(source_reading_rank(&centers, &[2]), Some(1.0));
        let mirrored: Vec<PlanPoint> = centers.iter().map(|&center| Mirror::ALL[1].point(center)).collect();
        assert_eq!(source_reading_rank(&mirrored, &[2]), Some(0.0));
        assert_eq!(source_reading_rank(&centers, &[]), None);
    }

    #[test]
    fn the_flow_rank_reads_along_the_flow_frame() {
        let centers = [PlanPoint::new(6.0, 0.0), PlanPoint::new(0.0, 5.0), PlanPoint::new(3.0, -5.0)];
        assert_eq!(source_flow_rank(&centers, &[1]), Some(0.0));
        assert_eq!(source_flow_rank(&centers, &[0]), Some(1.0 + ACROSS_TIE_WEIGHT * 0.5));
        let mirrored: Vec<PlanPoint> = centers.iter().map(|&center| Mirror::ALL[2].point(center)).collect();
        let (straight, flipped) = (source_flow_rank(&centers, &[2]), source_flow_rank(&mirrored, &[2]));
        assert!(flipped < straight, "across the flow, larger y reads first: {flipped:?} vs {straight:?}");
    }

    #[test]
    fn sources_are_the_nodes_no_edge_enters() {
        assert_eq!(sources(&[(0, 1), (1, 2), (3, 2), (3, 3)], 4), vec![0, 3]);
        assert!(sources(&[(0, 1), (1, 0)], 2).is_empty());
    }

    #[test]
    fn mirrors_keep_rectangles_normalized() {
        let rect = Mirror::ALL[3].rect(PlanRect::new(1.0, 2.0, 3.0, 5.0));
        assert_eq!(rect, PlanRect::new(-3.0, -5.0, -1.0, -2.0));
    }
}
