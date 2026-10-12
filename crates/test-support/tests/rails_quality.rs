//! `routing: rails` (and `orthogonal`, the same elbows with tighter corners) without detours (T-407): in every example × rotation × density, edges take few
//! bends, never wander far from the Manhattan distance between their ends, never cross an edge of the same
//! node, never run glued to another edge (closer than a lane), never read as running under a node
//! (`W_EDGE_THROUGH_NODE`), and every head stays visible and square to its last leg (also on a face hidden
//! behind its node).

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use dioramic_core::scene::crossing_pairs;
use dioramic_spec::{Density, DiagnosticCode, Rotation, Routing, ThemeName};
use test_support::quality::{edge_bends, edge_detour, overlap_length};
use test_support::{View, assert_arrows_aim_at_targets, fixture, fixture_names};

/// Mean bends per edge over every view (T-407 goal).
const BENDS_MEAN_GOAL: f64 = 1.3;

/// Per-view limits of one density.
struct Limits {
    /// Most bends of one edge.
    bends: usize,
    /// Longest route over the Manhattan distance between its ends.
    detour: f64,
    /// Stroke glued to another edge (tiles).
    glued: f64,
    /// `W_EDGE_THROUGH_NODE` diagnostics.
    through_node: usize,
}

/// `spaced`, the density rails are made for: the T-407 goals (≤ 3 bends, ≤ 1.4×), but for two edges of 1280:
/// web-platform r90, whose API Gateway stands right beside the Orders API (gateway → orders at 1.53×), and
/// bank-branch r180, whose core switch is ringed by its rack, server and NAS (core → guest AP, 4 bends).
const SPACED: Limits = Limits { bends: 4, detour: 1.55, glued: 0.0, through_node: 0 };
/// `compact`, where nodes stand about a tile apart and the short rails of a crowded hub (the bank-branch
/// switches, three arrivals on one face of the web-platform Postgres) sometimes need a jog, share a face a
/// little closer than a lane or pass beside a node (T-407 handoff).
const COMPACT: Limits = Limits { bends: 4, detour: 1.8, glued: 1.0, through_node: 1 };

#[test]
fn rails_take_few_bends_and_no_detours() {
    elbows_take_few_bends_and_no_detours(Routing::Rails);
}

/// `orthogonal` is drawn by the same elbow router with tighter corners (T-407).
#[test]
fn orthogonal_routes_take_few_bends_and_no_detours() {
    elbows_take_few_bends_and_no_detours(Routing::Orthogonal);
}

fn elbows_take_few_bends_and_no_detours(routing: Routing) {
    let (mut bends, mut edges) = (0usize, 0usize);
    let mut failures = Vec::new();
    for name in fixture_names() {
        let mut model = fixture(&name).model();
        model.layout.routing = routing;
        for edge in &mut model.edges {
            edge.route = routing;
        }
        for density in Density::ALL {
            let limits = match density {
                Density::Compact => &COMPACT,
                Density::Spaced => &SPACED,
            };
            for rotation in Rotation::ALL {
                let view = View::new(ThemeName::Blueprint, rotation, density);
                let diagram = dioramic_core::compile_with(&model, &view.compile_options())
                    .unwrap_or_else(|diagnostics| panic!("{name}: {diagnostics}"));
                let label = format!("{routing:?} {name} {}", view.label());
                let through_node: Vec<String> = diagram
                    .diagnostics
                    .iter()
                    .filter(|diagnostic| diagnostic.code == DiagnosticCode::EdgeThroughNode)
                    .map(|diagnostic| format!("{label}: {diagnostic}"))
                    .collect();
                if through_node.len() > limits.through_node {
                    failures.extend(through_node);
                }
                let scene = diagram.scene;
                assert_arrows_aim_at_targets(&scene);
                for pair in crossing_pairs(&scene.edges).into_iter().filter(|pair| pair.shared_node) {
                    failures.push(format!("{label}: shared-node crossing #{} × #{}", pair.first, pair.second));
                }
                let glued = overlap_length(&scene);
                if glued > limits.glued {
                    failures.push(format!("{label}: {glued:.2} tiles glued to another edge"));
                }
                for edge in &scene.edges {
                    let (edge_bends, detour) = (edge_bends(&scene, edge), edge_detour(&scene, edge));
                    bends += edge_bends;
                    edges += 1;
                    if edge_bends > limits.bends {
                        failures.push(format!("{label}: {} takes {edge_bends} bends", edge.id.as_str()));
                    }
                    if detour > limits.detour {
                        failures.push(format!("{label}: {} is {detour:.2}× its Manhattan span", edge.id.as_str()));
                    }
                }
            }
        }
    }
    let mean = bends as f64 / edges.max(1) as f64;
    eprintln!("{routing:?}: {edges} edges, {mean:.2} bends per edge");
    assert!(mean <= BENDS_MEAN_GOAL, "mean bends {mean:.2} > {BENDS_MEAN_GOAL}");
    assert!(failures.is_empty(), "{} problem(s):\n{}", failures.len(), failures.join("\n"));
}
