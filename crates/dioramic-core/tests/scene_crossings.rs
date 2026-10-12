//! Edge fans (T-400): edges that share a node never cross each other, in every example × rotation
//! (fixed and `auto`) × density.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use dioramic_core::scene::{Scene, crossing_pairs, edge_crossings};
use dioramic_core::{CompileOptions, Diagram, compile_source};
use dioramic_spec::{Density, DiagnosticCode, Rotation, RotationSpec, SourceFormat};

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn example_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(examples_dir())
        .unwrap()
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.strip_suffix(".diorama.yaml").map(str::to_owned))
        .collect();
    names.sort();
    names
}

fn compile(name: &str, rotation: RotationSpec, density: Density) -> Diagram {
    let source = std::fs::read_to_string(examples_dir().join(format!("{name}.diorama.yaml"))).unwrap();
    let options = CompileOptions { rotation: Some(rotation), density: Some(density), ..CompileOptions::default() };
    compile_source(&source, SourceFormat::Yaml, &options).unwrap_or_else(|errors| panic!("{name}: {errors:?}"))
}

fn every_view() -> Vec<(RotationSpec, Density)> {
    let rotations = Rotation::ALL.into_iter().map(RotationSpec::Fixed).chain([RotationSpec::Auto]);
    rotations.flat_map(|rotation| [Density::Compact, Density::Spaced].map(|density| (rotation, density))).collect()
}

/// `checkout r90 spaced: lb->api1 × lb->api2 (1)` for every shared-node crossing of a scene.
fn shared_node_crossings(label: &str, scene: &Scene) -> Vec<String> {
    crossing_pairs(&scene.edges)
        .into_iter()
        .filter(|pair| pair.shared_node)
        .map(|pair| {
            let (first, second) = (&scene.edges[pair.first], &scene.edges[pair.second]);
            format!("{label}: {} × {} ({})", first.id.as_str(), second.id.as_str(), pair.count)
        })
        .collect()
}

#[test]
fn edges_sharing_a_node_never_cross() {
    let mut failures = Vec::new();
    let mut total = 0;
    for name in example_names() {
        for (rotation, density) in every_view() {
            let diagram = compile(&name, rotation, density);
            let label = format!("{name} {rotation:?} {density:?}");
            total += edge_crossings(&diagram.scene.edges).total;
            failures.extend(shared_node_crossings(&label, &diagram.scene));
        }
    }
    eprintln!("crossings (any two edges) over every view: {total}");
    assert!(failures.is_empty(), "{} shared-node crossing(s):\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn a_shared_node_crossing_is_reported_as_w_edge_crossing() {
    for name in example_names() {
        let diagram = compile(&name, RotationSpec::Auto, Density::Spaced);
        let reported =
            diagram.diagnostics.iter().filter(|diagnostic| diagnostic.code == DiagnosticCode::EdgeCrossing).count();
        let pairs = crossing_pairs(&diagram.scene.edges).into_iter().filter(|pair| pair.shared_node).count();
        assert_eq!(reported, pairs, "{name}: one W_EDGE_CROSSING per crossing pair that shares a node");
    }
}
