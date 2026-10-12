//! The edge graph as the hub layout needs it: undirected neighbours, degrees, pairs two hops apart and the
//! edges that point forward along the flow.

use std::collections::BTreeMap;

use dioramic_spec::Model;

/// Undirected view of the edges (self loops and unknown endpoints dropped).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct HubGraph {
    /// Distinct neighbours of each node, ascending.
    pub neighbours: Vec<Vec<usize>>,
    /// Edges at each node, parallel edges counted (how busy a place is).
    pub degree: Vec<usize>,
    /// Nodes exactly two hops away from each node, ascending.
    pub second_neighbours: Vec<Vec<usize>>,
    /// `(from, to)` of every edge whose ends are on increasing flow slots: they should keep pointing forward.
    pub forward: Vec<(usize, usize)>,
    /// Edges as `(from, to)` node indices, spec order (parallel edges kept).
    pub edges: Vec<(usize, usize)>,
}

impl HubGraph {
    /// Builds the graph of `model`; `slots` orders the nodes along the flow.
    pub fn new(model: &Model, slots: &[u32]) -> Self {
        let index_of: BTreeMap<&str, usize> =
            model.nodes.iter().enumerate().map(|(index, node)| (node.id.as_str(), index)).collect();
        let count = model.nodes.len();
        let edges: Vec<(usize, usize)> = model
            .edges
            .iter()
            .filter_map(|edge| Some((*index_of.get(edge.from.as_str())?, *index_of.get(edge.to.as_str())?)))
            .filter(|(from, to)| from != to)
            .collect();
        let mut neighbours = vec![Vec::new(); count];
        let mut degree = vec![0; count];
        for &(from, to) in &edges {
            degree[from] += 1;
            degree[to] += 1;
            neighbours[from].push(to);
            neighbours[to].push(from);
        }
        for list in &mut neighbours {
            list.sort_unstable();
            list.dedup();
        }
        let second_neighbours = (0..count).map(|node| two_hops(&neighbours, node)).collect();
        let mut forward: Vec<(usize, usize)> =
            edges.iter().copied().filter(|&(from, to)| slots[from] < slots[to]).collect();
        forward.sort_unstable();
        forward.dedup();
        Self { neighbours, degree, second_neighbours, forward, edges }
    }

    /// Largest degree (at least 1, so it can divide).
    pub fn max_degree(&self) -> usize {
        self.degree.iter().copied().max().unwrap_or(0).max(1)
    }
}

fn two_hops(neighbours: &[Vec<usize>], node: usize) -> Vec<usize> {
    let mut result: Vec<usize> = neighbours[node]
        .iter()
        .flat_map(|&middle| neighbours[middle].iter().copied())
        .filter(|&other| other != node && neighbours[node].binary_search(&other).is_err())
        .collect();
    result.sort_unstable();
    result.dedup();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::fixtures::ModelBuilder;

    #[test]
    fn neighbours_degrees_and_two_hops() {
        let model = ModelBuilder::new()
            .nodes(&["a", "b", "c", "d"])
            .edges(&["a -> b", "a -> b", "b -> c", "c -> c", "c -> ghost"])
            .build();
        let graph = HubGraph::new(&model, &[0, 1, 2, 0]);
        assert_eq!(graph.neighbours, vec![vec![1], vec![0, 2], vec![1], vec![]]);
        assert_eq!(graph.degree, vec![2, 3, 1, 0]);
        assert_eq!(graph.second_neighbours, vec![vec![2], vec![], vec![0], vec![]]);
        assert_eq!(graph.forward, vec![(0, 1), (1, 2)]);
        assert_eq!(graph.max_degree(), 3);
    }
}
