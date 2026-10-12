//! The hub layout on every example, on hand-built graphs and on random models.

use std::time::Instant;

use dioramic_spec::{Density, FlowAxis, LayoutAlgorithm, Model, PlanCoord};
use proptest::prelude::*;

use super::LATTICE_STEP;
use crate::geometry::PlanPoint;
use crate::layout::fixtures::{ModelBuilder, example, example_names};
use crate::layout::tests::{assert_invariants, dump, random_model};
use crate::layout::{PlanLayout, layout};

fn hub(model: &Model) -> Model {
    let mut model = model.clone();
    model.layout.algorithm = LayoutAlgorithm::Hub;
    model
}

fn plan_of(model: &Model, density: Density) -> PlanLayout {
    layout(model, density).unwrap_or_else(|errors| panic!("layout failed: {errors:?}"))
}

fn on_lattice(value: f64) -> bool {
    let steps = value / LATTICE_STEP;
    (steps - steps.round()).abs() < 1e-9
}

fn center(plan: &PlanLayout, id: &str) -> PlanPoint {
    plan.nodes.iter().find(|node| node.id.as_str() == id).map(|node| node.center).unwrap_or_else(|| panic!("node {id}"))
}

/// Mean distance of the nodes to the middle of the bounds, weighted by degree.
fn weighted_offset(model: &Model, plan: &PlanLayout) -> f64 {
    let middle = plan.bounds.center();
    let (mut sum, mut weight) = (0.0, 0.0);
    for node in &plan.nodes {
        let degree = model.edges.iter().filter(|edge| edge.from == node.id || edge.to == node.id).count() as f64;
        sum += degree * node.center.distance(middle);
        weight += degree;
    }
    if weight == 0.0 { 0.0 } else { sum / weight }
}

#[test]
fn every_example_satisfies_the_invariants_on_the_lattice() {
    for name in example_names() {
        let model = hub(&example(&name));
        for density in Density::ALL {
            let plan = plan_of(&model, density);
            assert_invariants(&model, &plan);
            for node in &plan.nodes {
                assert!(
                    on_lattice(node.center.x) && on_lattice(node.center.y),
                    "{name}: {} at {:?}",
                    node.id,
                    node.center
                );
            }
            dump::write(&format!("hub-{name}-{}", density.as_str()), &model, &plan);
        }
    }
}

fn edge_length(model: &Model, plan: &PlanLayout) -> f64 {
    model
        .edges
        .iter()
        .filter_map(|edge| Some(plan.node(&edge.from)?.center.distance(plan.node(&edge.to)?.center)))
        .sum()
}

#[test]
fn hub_shortens_the_edges_of_the_examples() {
    for density in Density::ALL {
        let (mut flow_total, mut hub_total) = (0.0, 0.0);
        for name in example_names() {
            let flow = example(&name);
            let (before, after) =
                (edge_length(&flow, &plan_of(&flow, density)), edge_length(&flow, &plan_of(&hub(&flow), density)));
            if name == "web-platform" || name == "kubernetes-platform" {
                assert!(after < before, "{name} {}: hub {after:.1} vs flow {before:.1}", density.as_str());
            }
            flow_total += before;
            hub_total += after;
        }
        assert!(hub_total < flow_total, "{}: hub {hub_total:.1} vs flow {flow_total:.1}", density.as_str());
    }
}

#[test]
fn the_hub_of_a_star_sits_in_the_middle() {
    let model = ModelBuilder::new()
        .nodes(&["a", "b", "c", "hub", "d", "e"])
        .edges(&["a -> hub", "b -> hub", "hub -> c", "hub -> d", "hub -> e"])
        .algorithm(LayoutAlgorithm::Hub)
        .build();
    let plan = plan_of(&model, Density::Spaced);
    let middle = plan.bounds.center();
    let hub_offset = center(&plan, "hub").distance(middle);
    for node in &plan.nodes {
        assert!(node.center.distance(middle) >= hub_offset - 1e-9, "{} is more central than the hub", node.id);
    }
}

#[test]
fn busy_places_end_closer_to_the_middle_than_with_flow() {
    for name in ["web-platform", "kubernetes-platform", "checkout"] {
        let flow = example(name);
        let before = weighted_offset(&flow, &plan_of(&flow, Density::Spaced));
        let after = weighted_offset(&flow, &plan_of(&hub(&flow), Density::Spaced));
        assert!(after <= before + 1e-9, "{name}: hub offset {after:.2} vs flow {before:.2}");
    }
}

#[test]
fn sources_lean_towards_the_start_of_the_flow() {
    let model = hub(&example("web-platform"));
    let plan = plan_of(&model, Density::Spaced);
    assert!(center(&plan, "web").x < center(&plan, "db").x, "clients before data along the flow");
}

#[test]
fn flow_y_transposes_the_hub_layout() {
    let along_x = hub(&example("checkout"));
    let mut along_y = along_x.clone();
    along_y.layout.flow = FlowAxis::Y;
    let (x, y) = (plan_of(&along_x, Density::Spaced), plan_of(&along_y, Density::Spaced));
    for (a, b) in x.nodes.iter().zip(&y.nodes) {
        assert!((a.center.x - b.center.y).abs() < 1e-9 && (a.center.y - b.center.x).abs() < 1e-9);
    }
}

#[test]
fn pinned_models_fall_back_to_flow() {
    let mut flow = example("checkout");
    flow.nodes[0].at = Some(PlanCoord { x: 1.0, y: 2.0 });
    assert_eq!(plan_of(&flow, Density::Spaced), plan_of(&hub(&flow), Density::Spaced));
}

#[test]
fn hub_layout_is_deterministic() {
    for name in example_names() {
        let model = hub(&example(&name));
        assert_eq!(plan_of(&model, Density::Compact), plan_of(&model, Density::Compact), "{name}");
    }
}

/// 30 nodes in 5 zones of 6, each zone a star, the stars chained and fed by the first hub.
fn thirty_nodes_in_stars() -> Model {
    let mut builder = ModelBuilder::new().algorithm(LayoutAlgorithm::Hub);
    let mut edges = Vec::new();
    for zone in 0..5 {
        builder = builder.zone(crate::layout::fixtures::zone(&format!("z{zone}")));
        let ids: Vec<String> = (0..6).map(|node| format!("n{zone}_{node}")).collect();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        builder = builder.nodes_in(&format!("z{zone}"), &refs);
        for node in 1..6 {
            edges.push(format!("n{zone}_0 -> n{zone}_{node}"));
        }
        if zone > 0 {
            edges.push(format!("n{}_0 -> n{zone}_0", zone - 1));
            edges.push(format!("n0_0 -> n{zone}_3"));
        }
    }
    let refs: Vec<&str> = edges.iter().map(String::as_str).collect();
    builder.edges(&refs).build()
}

/// 30 nodes and 45 pseudo-random edges (a fixed linear congruential sequence), `zones` zones (0: none).
fn thirty_nodes_scrambled(zones: usize) -> Model {
    let mut builder = ModelBuilder::new().algorithm(LayoutAlgorithm::Hub);
    for zone in 0..zones {
        builder = builder.zone(crate::layout::fixtures::zone(&format!("z{zone}")));
    }
    for node in 0..30 {
        let id = format!("n{node}");
        builder =
            if zones == 0 { builder.nodes(&[&id]) } else { builder.nodes_in(&format!("z{}", node % zones), &[&id]) };
    }
    let mut state: u64 = 7;
    let mut next = |bound: u64| {
        state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % bound
    };
    let edges: Vec<String> = (0..45).map(|_| format!("n{} -> n{}", next(30), next(30))).collect();
    let refs: Vec<&str> = edges.iter().map(String::as_str).collect();
    builder.edges(&refs).build()
}

/// Best of three runs (ms): the budget is about the algorithm, not about other tests sharing the machine.
fn layout_ms(model: &Model, density: Density) -> f64 {
    (0..3)
        .map(|_| {
            let started = Instant::now();
            let plan = plan_of(model, density);
            let elapsed = started.elapsed().as_secs_f64() * 1000.0;
            assert_invariants(model, &plan);
            elapsed
        })
        .fold(f64::INFINITY, f64::min)
}

#[test]
fn thirty_nodes_lay_out_within_the_budget() {
    // The 20 ms budget is for release builds; debug builds are an order of magnitude slower.
    let budget = if cfg!(debug_assertions) { 2000.0 } else { 20.0 };
    let models =
        [thirty_nodes_in_stars(), thirty_nodes_scrambled(0), thirty_nodes_scrambled(4), thirty_nodes_scrambled(8)];
    for (index, model) in models.iter().enumerate() {
        for density in Density::ALL {
            let elapsed = layout_ms(model, density);
            assert!(
                elapsed < budget,
                "model {index} {}: hub layout of 30 nodes took {elapsed:.1} ms",
                density.as_str()
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn random_models_never_overlap_with_hub((model, density) in random_model()) {
        let model = hub(&model);
        let plan = plan_of(&model, density);
        assert_invariants(&model, &plan);
    }
}
