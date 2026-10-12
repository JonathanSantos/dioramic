//! Scene lint: `W_EDGE_THROUGH_NODE` and `W_EDGE_CROSSING` (`docs/06-renderizacao.md#arestas`) and the edge
//! readability used by `rotation: auto` (edges through nodes, edges with too little visible stroke, fans
//! that cross).

use dioramic_spec::{Diagnostic, DiagnosticCode, Diagnostics, Model};

use crate::geometry::ScreenPoint;

use super::crossings::crossing_pairs;
use super::visibility::{Cover, MIN_VISIBLE_EDGE_PX, Trace, covered_length, grazing_length, visible_length};
use super::{SceneEdge, SceneNode};

/// Stroke length (px) inside another node's silhouette for the edge to be reported.
pub(super) const THROUGH_NODE_MIN_PX: f64 = 10.0;
/// Stroke length (px) hugging another node's outline for the edge to be reported: longer than a stroke
/// that just passes a corner, shorter than one that follows a base or a top (T-381).
pub(super) const ALONG_NODE_MIN_PX: f64 = 30.0;

/// How many edges a reader would misread in a scene. Lower is better; used after the collision score
/// to choose between rotations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct EdgeReadability {
    /// Edge × node pairs where the stroke runs through the node.
    pub through_node: usize,
    /// Edges whose visible stroke (outside nodes and its own pill) is shorter than [`MIN_VISIBLE_EDGE_PX`].
    pub hidden: usize,
    /// Edge × node pairs where the stroke runs along the node's outline without entering it.
    pub along_node: usize,
    /// Pairs of edges that share a node and cross each other (T-400).
    pub crossing_shared_node: usize,
}

impl EdgeReadability {
    /// Total number of problems.
    pub fn problems(&self) -> usize {
        self.through_node + self.hidden + self.along_node + self.crossing_shared_node
    }
}

/// Lint findings and readability of the scene edges.
pub fn lint_edges(
    model: &Model,
    edges: &[SceneEdge],
    nodes: &[SceneNode],
    silhouettes: &[Vec<ScreenPoint>],
) -> (Diagnostics, EdgeReadability) {
    let covers: Vec<Cover<'_>> = silhouettes.iter().map(|silhouette| Cover::new(silhouette)).collect();
    let mut diagnostics = Diagnostics::new();
    let mut readability = EdgeReadability::default();
    for edge in edges {
        let trace = Trace::of(&edge.path);
        let pill: Vec<_> = edge.label.iter().map(|label| label.rect).collect();
        if visible_length(&trace, &covers, &pill) < MIN_VISIBLE_EDGE_PX {
            readability.hidden += 1;
        }
        let others = || nodes.iter().zip(&covers).filter(|(node, _)| node.id != edge.from && node.id != edge.to);
        let crossed: Vec<&str> = others()
            .filter(|(_, cover)| covered_length(&trace, cover) >= THROUGH_NODE_MIN_PX)
            .map(|(node, _)| node.id.as_str())
            .collect();
        let hugged: Vec<&str> = others()
            .filter(|(node, _)| !crossed.contains(&node.id.as_str()))
            .filter(|(_, cover)| grazing_length(&trace, cover) >= ALONG_NODE_MIN_PX)
            .map(|(node, _)| node.id.as_str())
            .collect();
        if crossed.is_empty() && hugged.is_empty() {
            continue;
        }
        readability.through_node += crossed.len();
        readability.along_node += hugged.len();
        let path =
            model.edges.iter().find(|spec| spec.id == edge.id).map_or_else(String::new, |spec| spec.path.clone());
        diagnostics.push(
            Diagnostic::new(
                DiagnosticCode::EdgeThroughNode,
                path,
                through_node_message(edge.id.as_str(), &crossed, &hugged),
            )
            .with_hint("try route: orthogonal, view.rotation: auto, or move the nodes with at:"),
        );
    }
    lint_shared_node_crossings(model, edges, &mut diagnostics, &mut readability);
    (diagnostics, readability)
}

/// One `W_EDGE_CROSSING` per pair of edges that share a node and cross (a fan that reads as an "X").
fn lint_shared_node_crossings(
    model: &Model,
    edges: &[SceneEdge],
    diagnostics: &mut Diagnostics,
    readability: &mut EdgeReadability,
) {
    for pair in crossing_pairs(edges).into_iter().filter(|pair| pair.shared_node) {
        readability.crossing_shared_node += 1;
        let (first, second) = (&edges[pair.first], &edges[pair.second]);
        let path =
            model.edges.iter().find(|spec| spec.id == second.id).map_or_else(String::new, |spec| spec.path.clone());
        diagnostics.push(
            Diagnostic::new(
                DiagnosticCode::EdgeCrossing,
                path,
                format!("edges '{}' and '{}' share a node and cross each other", first.id.as_str(), second.id.as_str()),
            )
            .with_hint("try view.rotation: auto, or move the nodes with at:"),
        );
    }
}

/// `edge 'a->b' passes through node(s) 'x' and runs along node(s) 'y'` (either part may be missing).
fn through_node_message(edge: &str, crossed: &[&str], hugged: &[&str]) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !crossed.is_empty() {
        parts.push(format!("passes through node(s) {}", quoted(crossed)));
    }
    if !hugged.is_empty() {
        parts.push(format!("runs along the outline of node(s) {}, as if under or behind it", quoted(hugged)));
    }
    format!("edge '{edge}' {}", parts.join(" and "))
}

fn quoted(ids: &[&str]) -> String {
    ids.iter().map(|id| format!("'{id}'")).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use dioramic_spec::SourceFormat;

    use super::*;
    use crate::scene::EdgePath;
    use crate::{CompileOptions, compile_source};

    const SPEC: &str = "diorama: 1\nnodes:\n  - { id: a }\n  - { id: b }\n  - { id: c }\nedges:\n  - a -> b\n";

    fn square(x0: f64, y0: f64, size: f64) -> Vec<ScreenPoint> {
        vec![
            ScreenPoint::new(x0, y0),
            ScreenPoint::new(x0 + size, y0),
            ScreenPoint::new(x0 + size, y0 + size),
            ScreenPoint::new(x0, y0 + size),
        ]
    }

    /// Lints edge `a -> b` drawn as a horizontal line at `y`, with `c` a 60 px square at (100, 0) and the
    /// ends far away.
    fn lint_line_at(y: f64) -> (Diagnostics, EdgeReadability) {
        let diagram = compile_source(SPEC, SourceFormat::Yaml, &CompileOptions::default())
            .unwrap_or_else(|errors| panic!("{errors:?}"));
        let mut edges = diagram.scene.edges.clone();
        edges[0].path = EdgePath::Polyline(vec![ScreenPoint::new(-200.0, y), ScreenPoint::new(400.0, y)]);
        edges[0].label = None;
        let nodes = &diagram.scene.nodes;
        let silhouettes: Vec<Vec<ScreenPoint>> = nodes
            .iter()
            .map(|node| match node.id.as_str() {
                "a" => square(-300.0, y - 30.0, 60.0),
                "b" => square(440.0, y - 30.0, 60.0),
                _ => square(100.0, 0.0, 60.0),
            })
            .collect();
        lint_edges(&diagram.model, &edges, nodes, &silhouettes)
    }

    #[test]
    fn an_edge_hugging_a_node_outline_is_reported() {
        let (diagnostics, readability) = lint_line_at(64.0);
        assert_eq!(readability.along_node, 1);
        assert_eq!(readability.through_node, 0);
        let [finding] = &diagnostics.iter().collect::<Vec<_>>()[..] else { panic!("{diagnostics:?}") };
        assert_eq!(finding.code, DiagnosticCode::EdgeThroughNode);
        assert!(finding.message.contains("runs along the outline of node(s) 'c'"), "{}", finding.message);
    }

    #[test]
    fn an_edge_through_a_node_is_reported_once_and_a_clear_one_not_at_all() {
        let (diagnostics, readability) = lint_line_at(30.0);
        assert_eq!((readability.through_node, readability.along_node), (1, 0));
        assert!(diagnostics.iter().all(|finding| finding.message.contains("passes through node(s) 'c'")));
        let (clear, readability) = lint_line_at(60.0 + 20.0);
        assert!(clear.is_empty(), "{clear:?}");
        assert_eq!(readability.problems(), 0);
    }
}
