//! Label attribution (T-394), checked geometrically from the scene, independently of the score: in every
//! view `rotation: auto` picks for an example, each pill is nearest its own edge (within 1.5 pill heights of
//! it, no other stroke nearer, no node label clearly nearer) and no node leader runs behind another node's
//! label. (The scene scores and reports the misattributions it cannot avoid: unit tests in
//! `scene/labels/attribution.rs` and `scene/collision.rs`.)
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use dioramic_core::geometry::{Rect, ScreenPoint};
use dioramic_core::scene::{EdgePath, Scene};
use dioramic_core::{CompileOptions, Diagram, compile_source};
use dioramic_spec::{Density, RotationSpec, SourceFormat};

/// Largest gap between a pill and its own stroke, in pill heights.
const MAX_GAP_SHARE: f64 = 1.5;
/// A pill this close to its own stroke (pill heights) sits on it.
const ON_EDGE_SHARE: f64 = 0.25;
/// A node label must be nearer than the own stroke by this much (pill heights) to take a pill over.
const LABEL_SLACK_SHARE: f64 = 0.5;
/// Float noise of the geometric checks (px).
const EPSILON_PX: f64 = 0.5;

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

/// Dense polyline of an edge stroke.
fn stroke(path: &EdgePath) -> Vec<ScreenPoint> {
    path.sample(256, 2.0)
}

fn gap_to_point(rect: &Rect, point: ScreenPoint) -> f64 {
    let dx = (rect.x0 - point.x).max(point.x - rect.x1).max(0.0);
    let dy = (rect.y0 - point.y).max(point.y - rect.y1).max(0.0);
    dx.hypot(dy)
}

/// Gap between a box and a densely sampled stroke (sampling noise ≤ 1 px).
fn gap_to_stroke(rect: &Rect, points: &[ScreenPoint]) -> f64 {
    points.iter().map(|point| gap_to_point(rect, *point)).fold(f64::INFINITY, f64::min)
}

fn gap_between(a: &Rect, b: &Rect) -> f64 {
    let dx = (b.x0 - a.x1).max(a.x0 - b.x1).max(0.0);
    let dy = (b.y0 - a.y1).max(a.y0 - b.y1).max(0.0);
    dx.hypot(dy)
}

/// Every label of `scene` that reads as another item's, described for the failure message.
fn misattributions(scene: &Scene) -> Vec<String> {
    let strokes: Vec<Vec<ScreenPoint>> = scene.edges.iter().map(|edge| stroke(&edge.path)).collect();
    let node_labels: Vec<(&str, Rect)> =
        scene.nodes.iter().filter_map(|node| Some((node.id.as_str(), node.label.as_ref()?.rect))).collect();
    let mut found = Vec::new();
    for (index, edge) in scene.edges.iter().enumerate() {
        let Some(pill) = &edge.label else { continue };
        let (rect, height) = (pill.rect, pill.rect.height());
        let own = gap_to_stroke(&rect, &strokes[index]);
        if own <= ON_EDGE_SHARE * height + EPSILON_PX {
            continue;
        }
        if own > MAX_GAP_SHARE * height + EPSILON_PX {
            found.push(format!("pill '{}' is {own:.0} px from its edge", edge.id));
        }
        for (other, points) in scene.edges.iter().zip(&strokes) {
            let gap = gap_to_stroke(&rect, points);
            if other.id != edge.id && gap + EPSILON_PX < own {
                found.push(format!(
                    "pill '{}' is nearer edge '{}' ({gap:.0} px) than its own ({own:.0} px)",
                    edge.id, other.id
                ));
            }
        }
        for (node, label) in &node_labels {
            let gap = gap_between(&rect, label);
            if gap + EPSILON_PX < own - LABEL_SLACK_SHARE * height {
                found.push(format!(
                    "pill '{}' sits on the label of '{node}' ({gap:.0} px, own edge {own:.0} px)",
                    edge.id
                ));
            }
        }
    }
    for node in &scene.nodes {
        let Some((anchor, end)) = node.leader else { continue };
        for (other, label) in &node_labels {
            let inside_x = anchor.x > label.x0 + 3.0 && anchor.x < label.x1 - 3.0;
            let along = anchor.y.max(end.y).min(label.y1) - anchor.y.min(end.y).max(label.y0);
            if *other != node.id.as_str() && inside_x && along > 4.0 {
                found.push(format!("leader of '{}' runs behind the label of '{other}'", node.id));
            }
        }
    }
    found
}

#[test]
fn auto_views_attribute_every_label_to_its_own_item() {
    for name in example_names() {
        for density in Density::ALL {
            let diagram = compile(&name, RotationSpec::Auto, density);
            let found = misattributions(&diagram.scene);
            assert!(found.is_empty(), "{name} {density:?} r{}: {found:#?}", diagram.scene.rotation.degrees());
        }
    }
}

#[test]
fn the_reviewed_cases_are_attributed_right() {
    // UI-final (T-394): bank-branch AP leader behind "Teller Workstation 1", event-driven `ShipmentCreated`
    // next to `archiver -> lake`, aws-serverless `OrderPlaced` stacked on the SQS label.
    for name in ["bank-branch", "event-driven", "aws-serverless"] {
        let diagram = compile(name, RotationSpec::Auto, Density::Spaced);
        let found = misattributions(&diagram.scene);
        assert!(found.is_empty(), "{name}: {found:#?}");
        assert_eq!(diagram.scene.collision.score, 0.0, "{name}: {:?}", diagram.scene.collision.overlaps);
    }
}
