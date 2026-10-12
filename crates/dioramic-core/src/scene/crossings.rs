//! Edge × edge crossings (T-400, `docs/06-renderizacao.md#arestas`).
//!
//! Two edges that cross read worse than two that do not; two edges that share a node and cross read as an
//! "X" where a reader expects a fan. The router charges both (the second much more), the lint reports the
//! second (`W_EDGE_CROSSING`) and the gallery writes both counts per view.

use super::SceneEdge;
use super::visibility::{Trace, crossing_count};

/// Crossings between the edges of a scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EdgeCrossings {
    /// Crossing points between any two edges.
    pub total: usize,
    /// The part of [`EdgeCrossings::total`] between edges that share a node (should be 0).
    pub shared_node: usize,
}

/// Two edges that cross, by index in the scene edges (`first < second`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrossingPair {
    /// Index of the first edge.
    pub first: usize,
    /// Index of the second edge.
    pub second: usize,
    /// Number of crossing points between them.
    pub count: usize,
    /// True when the two edges share an end node.
    pub shared_node: bool,
}

/// Every pair of crossing edges, in index order.
pub fn crossing_pairs(edges: &[SceneEdge]) -> Vec<CrossingPair> {
    let traces: Vec<Trace> = edges.iter().map(|edge| Trace::of(&edge.path)).collect();
    let mut pairs = Vec::new();
    for first in 0..edges.len() {
        for second in first + 1..edges.len() {
            let count = crossing_count(&traces[first], &traces[second]);
            if count > 0 {
                let shared_node = share_node(&edges[first], &edges[second]);
                pairs.push(CrossingPair { first, second, count, shared_node });
            }
        }
    }
    pairs
}

/// Crossing counts of a scene's edges.
pub fn edge_crossings(edges: &[SceneEdge]) -> EdgeCrossings {
    crossing_pairs(edges).iter().fold(EdgeCrossings::default(), |mut totals, pair| {
        totals.total += pair.count;
        if pair.shared_node {
            totals.shared_node += pair.count;
        }
        totals
    })
}

/// True when the two edges have an end node in common.
fn share_node(a: &SceneEdge, b: &SceneEdge) -> bool {
    [&a.from, &a.to].into_iter().any(|node| *node == b.from || *node == b.to)
}

#[cfg(test)]
mod tests {
    use dioramic_spec::SourceFormat;

    use super::*;
    use crate::geometry::ScreenPoint;
    use crate::scene::EdgePath;
    use crate::{CompileOptions, compile_source};

    const SPEC: &str = "diorama: 1\nnodes:\n  - { id: a }\n  - { id: b }\n  - { id: c }\n  - { id: d }\n\
                        edges:\n  - a -> b\n  - a -> c\n  - c -> d\n";

    fn edges_with(paths: [[(f64, f64); 2]; 3]) -> Vec<SceneEdge> {
        let diagram = compile_source(SPEC, SourceFormat::Yaml, &CompileOptions::default())
            .unwrap_or_else(|errors| panic!("{errors:?}"));
        let mut edges = diagram.scene.edges.clone();
        for (edge, [from, to]) in edges.iter_mut().zip(paths) {
            edge.path = EdgePath::Polyline(vec![ScreenPoint::new(from.0, from.1), ScreenPoint::new(to.0, to.1)]);
        }
        edges
    }

    #[test]
    fn a_fan_that_reads_as_an_x_counts_as_a_shared_node_crossing() {
        // a→b and a→c leave a and swap sides; c→d crosses a→b far from a.
        let edges =
            edges_with([[(0.0, 0.0), (100.0, 100.0)], [(0.0, 100.0), (100.0, 0.0)], [(50.0, -50.0), (60.0, 200.0)]]);
        let pairs = crossing_pairs(&edges);
        assert_eq!(pairs.len(), 3, "{pairs:?}");
        assert!(pairs.iter().all(|pair| pair.count == 1));
        // a→b × a→c share a; a→c × c→d share c; a→b × c→d share nothing.
        assert_eq!(edge_crossings(&edges), EdgeCrossings { total: 3, shared_node: 2 });
    }

    #[test]
    fn two_strokes_plaited_together_cross_twice() {
        // T-422: two dotted edges that cross, run side by side and cross back read as one plait.
        let mut edges = edges_with([
            [(0.0, 0.0), (300.0, 0.0)],
            [(500.0, 500.0), (600.0, 500.0)],
            [(700.0, 700.0), (800.0, 700.0)],
        ]);
        edges[1].path = EdgePath::Polyline(vec![
            ScreenPoint::new(0.0, -50.0),
            ScreenPoint::new(100.0, 50.0),
            ScreenPoint::new(200.0, 50.0),
            ScreenPoint::new(250.0, -50.0),
        ]);
        let pairs = crossing_pairs(&edges);
        assert_eq!(pairs.len(), 1, "{pairs:?}");
        assert_eq!((pairs[0].first, pairs[0].second, pairs[0].count), (0, 1, 2));
    }

    #[test]
    fn edges_meeting_only_at_their_ends_do_not_cross() {
        let edges =
            edges_with([[(0.0, 0.0), (100.0, 0.0)], [(0.0, 0.0), (100.0, 50.0)], [(100.0, 50.0), (200.0, 50.0)]]);
        assert_eq!(edge_crossings(&edges), EdgeCrossings::default());
    }
}
