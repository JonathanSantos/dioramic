//! Flow ranks of the edge graph: which nodes come first along `layout.flow`.
//!
//! Cycles are broken by a depth-first search in spec order (edges that close a cycle are ignored), then:
//! - `rank` is the longest path from a source (contract C-004: `0` = sources), used by the `build` animation;
//! - `slot` is the same layering pulled "as late as possible" towards the consumers (sinks keep their rank),
//!   so a client that only calls the serving tier sits next to it instead of at the far start of the diagram;
//! - `across` is a barycenter hint that orders siblings across the flow to reduce edge crossings.

use std::collections::BTreeMap;

use dioramic_spec::Model;

/// Flow ranks of every node, indexed like `model.nodes`.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct FlowRanks {
    /// Longest path from a source (0 = sources).
    pub rank: Vec<u32>,
    /// Placement layer: `rank` pulled towards the consumers; never smaller than `rank`.
    pub slot: Vec<u32>,
    /// Position hint across the flow, centered on 0 within each slot.
    pub across: Vec<f64>,
}

/// The acyclic part of the edge graph (self loops, unknown endpoints and cycle-closing edges dropped).
struct FlowGraph {
    successors: Vec<Vec<usize>>,
    predecessors: Vec<Vec<usize>>,
    /// Reverse depth-first finishing order: a topological order of the acyclic graph.
    topological: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Visit {
    New,
    Open,
    Done,
}

/// Computes ranks, slots and the across hint deterministically.
pub(super) fn flow_ranks(model: &Model) -> FlowRanks {
    let graph = acyclic_graph(model);
    let rank = longest_path_ranks(&graph);
    let slot = pulled_slots(&graph, &rank);
    let across = barycenter_hint(&graph, &slot);
    FlowRanks { rank, slot, across }
}

fn edge_lists(model: &Model) -> Vec<Vec<usize>> {
    let index_of: BTreeMap<&str, usize> =
        model.nodes.iter().enumerate().map(|(index, node)| (node.id.as_str(), index)).collect();
    let mut successors = vec![Vec::new(); model.nodes.len()];
    for edge in &model.edges {
        let (Some(&from), Some(&to)) = (index_of.get(edge.from.as_str()), index_of.get(edge.to.as_str())) else {
            continue;
        };
        if from != to && !successors[from].contains(&to) {
            successors[from].push(to);
        }
    }
    successors
}

/// Iterative depth-first search from every node in spec order; an edge into an open node closes a cycle.
fn acyclic_graph(model: &Model) -> FlowGraph {
    let all_successors = edge_lists(model);
    let count = all_successors.len();
    let mut visit = vec![Visit::New; count];
    let mut successors = vec![Vec::new(); count];
    let mut finished = Vec::with_capacity(count);
    for root in 0..count {
        if visit[root] != Visit::New {
            continue;
        }
        visit[root] = Visit::Open;
        let mut stack = vec![(root, 0_usize)];
        while let Some((node, next_edge)) = stack.last_mut() {
            let node = *node;
            if let Some(&target) = all_successors[node].get(*next_edge) {
                *next_edge += 1;
                match visit[target] {
                    Visit::Open => {}
                    Visit::Done => successors[node].push(target),
                    Visit::New => {
                        successors[node].push(target);
                        visit[target] = Visit::Open;
                        stack.push((target, 0));
                    }
                }
            } else {
                visit[node] = Visit::Done;
                finished.push(node);
                stack.pop();
            }
        }
    }
    let mut predecessors = vec![Vec::new(); count];
    for (from, targets) in successors.iter().enumerate() {
        for &to in targets {
            predecessors[to].push(from);
        }
    }
    finished.reverse();
    FlowGraph { successors, predecessors, topological: finished }
}

fn longest_path_ranks(graph: &FlowGraph) -> Vec<u32> {
    let mut rank = vec![0_u32; graph.successors.len()];
    for &node in &graph.topological {
        rank[node] = graph.predecessors[node].iter().map(|&from| rank[from] + 1).max().unwrap_or(0);
    }
    rank
}

/// As-late-as-possible pass in reverse topological order: a node moves to just before its earliest consumer.
fn pulled_slots(graph: &FlowGraph, rank: &[u32]) -> Vec<u32> {
    let mut slot = rank.to_vec();
    for &node in graph.topological.iter().rev() {
        if let Some(earliest) = graph.successors[node].iter().map(|&to| slot[to]).min() {
            slot[node] = earliest.saturating_sub(1).max(rank[node]);
        }
    }
    slot
}

/// Within each slot, orders nodes by the mean across position of their predecessors (sources first, then
/// spec order), and records the centered position as the hint.
fn barycenter_hint(graph: &FlowGraph, slot: &[u32]) -> Vec<f64> {
    let mut by_slot: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (node, &layer) in slot.iter().enumerate() {
        by_slot.entry(layer).or_default().push(node);
    }
    let mut across = vec![0.0; slot.len()];
    for members in by_slot.values() {
        let mut keyed: Vec<(f64, usize)> = members
            .iter()
            .map(|&node| {
                let predecessors = &graph.predecessors[node];
                let key = if predecessors.is_empty() {
                    f64::NEG_INFINITY
                } else {
                    predecessors.iter().map(|&from| across[from]).sum::<f64>() / predecessors.len() as f64
                };
                (key, node)
            })
            .collect();
        keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let center = (keyed.len() as f64 - 1.0) / 2.0;
        for (position, (_, node)) in keyed.into_iter().enumerate() {
            across[node] = position as f64 - center;
        }
    }
    across
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::fixtures::ModelBuilder;

    fn ranks_of(builder: ModelBuilder) -> FlowRanks {
        flow_ranks(&builder.build())
    }

    #[test]
    fn chain_ranks_increase_by_one() {
        let ranks = ranks_of(ModelBuilder::new().nodes(&["a", "b", "c"]).edges(&["a -> b", "b -> c"]));
        assert_eq!(ranks.rank, vec![0, 1, 2]);
        assert_eq!(ranks.slot, vec![0, 1, 2]);
    }

    #[test]
    fn fan_out_puts_siblings_on_the_same_rank_in_spec_order() {
        let ranks = ranks_of(ModelBuilder::new().nodes(&["lb", "api1", "api2", "api3"]).edges(&[
            "lb -> api1",
            "lb -> api2",
            "lb -> api3",
        ]));
        assert_eq!(ranks.rank, vec![0, 1, 1, 1]);
        assert_eq!(ranks.across, vec![0.0, -1.0, 0.0, 1.0]);
    }

    #[test]
    fn cycle_is_broken_at_the_edge_that_closes_it() {
        let ranks = ranks_of(ModelBuilder::new().nodes(&["a", "b", "c"]).edges(&["a -> b", "b -> c", "c -> a"]));
        assert_eq!(ranks.rank, vec![0, 1, 2]);
    }

    #[test]
    fn cycle_breaking_follows_spec_order_of_nodes() {
        let ranks = ranks_of(ModelBuilder::new().nodes(&["c", "b", "a"]).edges(&["a -> b", "b -> c", "c -> a"]));
        // DFS starts at `c`: c → a → b, and b → c closes the cycle.
        assert_eq!(ranks.rank, vec![0, 2, 1]);
    }

    #[test]
    fn self_loops_and_unknown_endpoints_are_ignored() {
        let ranks = ranks_of(ModelBuilder::new().nodes(&["a", "b"]).edges(&["a -> a", "a -> b", "a -> ghost"]));
        assert_eq!(ranks.rank, vec![0, 1]);
    }

    #[test]
    fn late_source_is_pulled_next_to_its_consumer() {
        let ranks =
            ranks_of(ModelBuilder::new().nodes(&["a", "b", "c", "client"]).edges(&["a -> b", "b -> c", "client -> c"]));
        assert_eq!(ranks.rank[3], 0, "rank keeps sources at 0");
        assert_eq!(ranks.slot[3], 1, "slot pulls the client next to c");
    }

    #[test]
    fn barycenter_orders_children_like_their_parents() {
        let ranks =
            ranks_of(ModelBuilder::new().nodes(&["root", "left", "right", "under_right", "under_left"]).edges(&[
                "root -> left",
                "root -> right",
                "right -> under_right",
                "left -> under_left",
            ]));
        assert!(ranks.across[4] < ranks.across[3], "under_left goes before under_right");
    }

    #[test]
    fn isolated_nodes_are_rank_zero() {
        let ranks = ranks_of(ModelBuilder::new().nodes(&["a", "b"]));
        assert_eq!(ranks.rank, vec![0, 0]);
        assert_eq!(ranks.slot, vec![0, 0]);
    }
}
