//! End-to-end scene checks over every example × 4 rotations × 2 densities (T-104):
//! arrows aim at their targets and arrive tangent (< 1°), no head is hidden behind another node, heads at
//! the same node do not overlap, collision-free fixtures stay at score 0, SVG output is deterministic.
//!
//! These are local equivalents of the `test-support` asserts (C-007), kept here because `dioramic-core`
//! cannot depend on `test-support` (it depends on `dioramic-core`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::time::Instant;

use dioramic_core::geometry::{PlanPoint, PlanVector, Projection, ScreenPoint, ScreenVector};
use dioramic_core::icons::Footprint;
use dioramic_core::scene::{ARROW_ANGLE_TOLERANCE_DEG, ArrowHead, Scene, SceneEdge, SceneNode};
use dioramic_core::svg::{SvgMode, SvgOptions, TextRendering};
use dioramic_core::{CompileOptions, Diagram, compile_source, render_svg};
use dioramic_spec::{Density, DiagnosticCode, Rotation, RotationSpec, SourceFormat, ThemeName};

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

fn source(name: &str) -> String {
    std::fs::read_to_string(examples_dir().join(format!("{name}.diorama.yaml"))).unwrap()
}

fn compile(name: &str, rotation: RotationSpec, density: Density) -> Diagram {
    let options = CompileOptions { rotation: Some(rotation), density: Some(density), ..CompileOptions::default() };
    compile_source(&source(name), SourceFormat::Yaml, &options)
        .unwrap_or_else(|errors| panic!("{name}: {:?}", errors.iter().map(|d| &d.message).collect::<Vec<_>>()))
}

fn every_view() -> Vec<(Rotation, Density)> {
    Density::ALL.into_iter().flat_map(|density| Rotation::ALL.map(|rotation| (rotation, density))).collect()
}

fn angle(a: ScreenVector, b: ScreenVector) -> f64 {
    a.angle_between_degrees(b).abs()
}

/// Heads at the end of the path aim along `end_direction`; heads at the start along the reversed start.
fn tangent_error(edge: &SceneEdge, head: &ArrowHead) -> f64 {
    let at_end = head.tip.distance(end_point(edge)) <= head.tip.distance(start_point(edge));
    if at_end {
        angle(head.direction(), edge.path.end_direction())
    } else {
        angle(head.direction(), edge.path.start_direction() * -1.0)
    }
}

fn start_point(edge: &SceneEdge) -> ScreenPoint {
    edge.path.point_at_fraction(0.0)
}

fn end_point(edge: &SceneEdge) -> ScreenPoint {
    edge.path.point_at_fraction(1.0)
}

/// Screen silhouette of a node body (convex hull of its prism), rebuilt from the scene.
fn silhouette(scene: &Scene, node: &SceneNode, z: f64) -> Vec<ScreenPoint> {
    let projection = Projection::new(scene.tile_px);
    let center = projection.unproject(node.floor_center, z);
    let outline: Vec<PlanPoint> = match node.shape.footprint {
        Footprint::Rect { half_width, half_depth } => [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .iter()
            .map(|(sx, sy)| center + PlanVector::new(sx * half_width, sy * half_depth))
            .collect(),
        Footprint::Circle { radius } => (0..24)
            .map(|index| {
                let a = std::f64::consts::TAU * f64::from(index) / 24.0;
                center + PlanVector::new(a.cos(), a.sin()) * radius
            })
            .collect(),
    };
    let mut points: Vec<ScreenPoint> = outline
        .iter()
        .flat_map(|point| [projection.project(*point, z), projection.project(*point, z + node.shape.height)])
        .collect();
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    hull(points)
}

fn hull(points: Vec<ScreenPoint>) -> Vec<ScreenPoint> {
    let cross = |o: ScreenPoint, a: ScreenPoint, b: ScreenPoint| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut lower: Vec<ScreenPoint> = Vec::new();
    for &p in &points {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<ScreenPoint> = Vec::new();
    for &p in points.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

fn strictly_inside(polygon: &[ScreenPoint], point: ScreenPoint) -> bool {
    let inset = 0.5;
    let sides: Vec<f64> = (0..polygon.len())
        .map(|i| {
            let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
            let edge = b - a;
            (edge.x * (point.y - a.y) - edge.y * (point.x - a.x)) / edge.length().max(1e-9)
        })
        .collect();
    sides.iter().all(|side| *side > inset) || sides.iter().all(|side| *side < -inset)
}

/// Asserts the arrow invariants of C-005 and returns the worst angle error (degrees).
fn assert_arrows(name: &str, diagram: &Diagram) -> f64 {
    let scene = &diagram.scene;
    let mut worst: f64 = 0.0;
    for edge in &scene.edges {
        for head in &edge.heads {
            let to_center = head.target_center - head.tip;
            let aim = angle(head.direction(), to_center);
            let tangent = tangent_error(edge, head);
            worst = worst.max(aim).max(tangent);
            assert!(aim < ARROW_ANGLE_TOLERANCE_DEG, "{name} {}: head aims {aim:.3}° off the target center", edge.id);
            assert!(
                tangent < ARROW_ANGLE_TOLERANCE_DEG,
                "{name} {}: curve arrives {tangent:.3}° off the head",
                edge.id
            );
            for node in &scene.nodes {
                // A head lies against the body of its own node (either end of the edge), never another one.
                let own_node = node.id == edge.from || node.id == edge.to;
                if own_node {
                    continue;
                }
                let z = diagram.model.node(&node.id).map_or(0.0, |spec| spec.z);
                let polygon = silhouette(scene, node, z);
                for point in [head.tip, head.base[0], head.base[1]] {
                    assert!(
                        !strictly_inside(&polygon, point),
                        "{name} {}: head hidden behind node {} at r{} {}",
                        edge.id,
                        node.id,
                        scene.rotation.degrees(),
                        scene.density.as_str()
                    );
                }
            }
        }
    }
    assert_heads_apart(name, scene);
    worst
}

fn assert_heads_apart(name: &str, scene: &Scene) {
    let heads: Vec<(&str, &ArrowHead)> =
        scene.edges.iter().flat_map(|edge| edge.heads.iter().map(move |head| (edge.id.as_str(), head))).collect();
    for (index, (id, head)) in heads.iter().enumerate() {
        for (other_id, other) in &heads[index + 1..] {
            assert!(
                head.tip.distance(other.tip) > 3.0,
                "{name}: heads of {id} and {other_id} overlap at r{} {}",
                scene.rotation.degrees(),
                scene.density.as_str()
            );
        }
    }
}

#[test]
fn arrows_aim_at_targets_in_every_example_rotation_and_density() {
    let mut worst: f64 = 0.0;
    for name in example_names() {
        for (rotation, density) in every_view() {
            let diagram = compile(&name, RotationSpec::Fixed(rotation), density);
            worst = worst.max(assert_arrows(&format!("{name} r{} {}", rotation.degrees(), density.as_str()), &diagram));
        }
    }
    assert!(worst < ARROW_ANGLE_TOLERANCE_DEG, "worst {worst}");
}

/// `rails` routing (T-403) keeps the arrow rules: heads on the target center, tangent to the rail, visible,
/// and every rail is a polyline that leaves and arrives along its lead.
#[test]
fn arrows_aim_at_targets_on_rails() {
    let mut worst: f64 = 0.0;
    for name in example_names() {
        let mut spec = dioramic_spec::parse(&source(&name), SourceFormat::Yaml).unwrap();
        spec.layout.get_or_insert_with(Default::default).routing = Some(dioramic_spec::Routing::Rails);
        let rails = serde_json::to_string(&spec).unwrap();
        for (rotation, density) in every_view() {
            let options = CompileOptions {
                rotation: Some(RotationSpec::Fixed(rotation)),
                density: Some(density),
                ..CompileOptions::default()
            };
            let diagram = compile_source(&rails, SourceFormat::Json, &options).unwrap();
            let label = format!("{name} rails r{} {}", rotation.degrees(), density.as_str());
            assert!(
                diagram.scene.edges.iter().all(|edge| matches!(edge.path, dioramic_core::scene::EdgePath::Polyline(_))),
                "{label}: every rail is a polyline"
            );
            worst = worst.max(assert_arrows(&label, &diagram));
        }
    }
    assert!(worst < ARROW_ANGLE_TOLERANCE_DEG, "worst {worst}");
}

#[test]
fn checkout_spaced_front_view_is_collision_free() {
    let diagram = compile("checkout", RotationSpec::Fixed(Rotation::R0), Density::Spaced);
    assert_eq!(diagram.scene.collision.score, 0.0, "{:#?}", diagram.scene.collision.overlaps);
    assert!(diagram.diagnostics.iter().all(|d| d.code != DiagnosticCode::LabelOverlap));
}

#[test]
fn bank_branch_auto_rotation_is_collision_free() {
    let diagram = compile("bank-branch", RotationSpec::Auto, Density::Spaced);
    let scores = diagram.scene.rotation_scores.expect("auto fills the scores");
    assert_eq!(scores.scores.map(|(rotation, _)| rotation), Rotation::ALL);
    assert_eq!(diagram.scene.collision.score, 0.0, "{:#?}", diagram.scene.collision.overlaps);
}

#[test]
fn auto_picks_the_lowest_score() {
    for name in example_names() {
        for density in Density::ALL {
            let diagram = compile(&name, RotationSpec::Auto, density);
            let scores = diagram.scene.rotation_scores.unwrap();
            let best = scores.scores.iter().map(|(_, score)| *score).fold(f64::INFINITY, f64::min);
            assert_eq!(diagram.scene.collision.score, best, "{name} {}", density.as_str());
        }
    }
}

#[test]
fn residual_overlaps_are_reported_with_ids() {
    for name in example_names() {
        for (rotation, density) in every_view() {
            let diagram = compile(&name, RotationSpec::Fixed(rotation), density);
            let reported = diagram.diagnostics.iter().filter(|d| d.code == DiagnosticCode::LabelOverlap).count();
            if diagram.scene.collision.score > 0.0 {
                assert!(reported > 0 && !diagram.scene.collision.overlaps.is_empty());
                assert!(diagram.scene.collision.overlaps.iter().all(|overlap| !overlap.a.id.is_empty()));
            } else {
                assert_eq!(reported, 0);
            }
        }
    }
}

#[test]
fn density_changes_the_leader_length() {
    let leader = |density| {
        let diagram = compile("minimal", RotationSpec::Fixed(Rotation::R0), density);
        let lengths: Vec<f64> =
            diagram.scene.nodes.iter().filter_map(|node| node.leader).map(|(from, to)| from.y - to.y).collect();
        lengths.into_iter().fold(f64::INFINITY, f64::min)
    };
    assert!((leader(Density::Spaced) - 52.0).abs() < 0.01 || leader(Density::Spaced) >= 52.0);
    assert!(leader(Density::Compact) < leader(Density::Spaced));
}

#[test]
fn svg_output_is_deterministic_and_well_formed() {
    for name in example_names() {
        let first = compile(&name, RotationSpec::Auto, Density::Compact);
        let second = compile(&name, RotationSpec::Auto, Density::Compact);
        for mode in [SvgMode::Static, SvgMode::Frame { t: 0.4 }, SvgMode::Animated] {
            let options = SvgOptions { mode, ..SvgOptions::default() };
            let svg = render_svg(&first, &options);
            assert_eq!(svg, render_svg(&second, &options), "{name}");
            assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
            assert!(svg.contains("<metadata>dioramic "));
            assert!(!svg.contains("<marker"), "heads are polygons, never markers");
            for bad in ["NaN", "\"inf", "-inf", ",inf"] {
                assert!(!svg.contains(bad), "{name}: non-finite number ({bad})");
            }
            assert_eq!(svg.matches("<g").count(), svg.matches("</g>").count(), "{name}: balanced groups");
        }
    }
}

#[test]
fn svg_tags_items_for_hit_testing() {
    let diagram = compile("checkout", RotationSpec::Fixed(Rotation::R0), Density::Spaced);
    let svg = render_svg(&diagram, &SvgOptions::default());
    for role in ["zone", "zone-label", "edge", "node", "node-label", "edge-label", "leader"] {
        assert!(svg.contains(&format!("data-role=\"{role}\"")), "missing {role}");
    }
    assert!(svg.contains("data-id=\"api1\" data-role=\"node\""));
    assert!(svg.contains("data-id=\"lb\" data-role=\"node-label\""));
}

#[test]
fn paint_order_follows_the_doc() {
    let diagram = compile("checkout", RotationSpec::Fixed(Rotation::R0), Density::Spaced);
    let svg = render_svg(&diagram, &SvgOptions::default());
    let first = |role: &str| svg.find(&format!("data-role=\"{role}\"")).unwrap();
    let order = ["grid", "zone", "zone-label", "edge", "node", "leader", "node-label", "edge-label", "title"];
    for pair in order.windows(2) {
        assert!(first(pair[0]) < first(pair[1]), "{} must come before {}", pair[0], pair[1]);
    }
}

#[test]
fn frames_loop_seamlessly() {
    let diagram = compile("checkout", RotationSpec::Fixed(Rotation::R0), Density::Spaced);
    assert!(diagram.animation.is_some(), "checkout declares animation: flow");
    let at = |t| render_svg(&diagram, &SvgOptions { mode: SvgMode::Frame { t }, ..SvgOptions::default() });
    assert_eq!(at(0.0), at(1.0));
    assert_ne!(at(0.0), at(0.25));
    assert!(at(0.25).contains("data-role=\"packet\""));
    assert!(at(0.25).contains("stroke-dashoffset"));
}

#[test]
fn animated_svg_uses_css_and_smil() {
    let diagram = compile("checkout", RotationSpec::Fixed(Rotation::R0), Density::Spaced);
    let svg = render_svg(&diagram, &SvgOptions { mode: SvgMode::Animated, ..SvgOptions::default() });
    assert!(svg.contains("@keyframes dioramic-flow"));
    assert!(svg.contains("class=\"flow\""));
    assert_eq!(svg.matches("<animateMotion").count(), diagram.scene.edges.len());
}

#[test]
fn outlines_mode_has_no_text_elements() {
    let diagram = compile("checkout", RotationSpec::Fixed(Rotation::R0), Density::Spaced);
    let svg = render_svg(&diagram, &SvgOptions { text: TextRendering::Outlines, ..SvgOptions::default() });
    assert!(!svg.contains("<text"));
    let with_text = render_svg(&diagram, &SvgOptions::default());
    assert!(with_text.contains("<text"));
}

#[test]
fn every_example_compiles_in_every_theme() {
    for name in example_names() {
        for theme in [ThemeName::Blueprint, ThemeName::Midnight, ThemeName::Paper, ThemeName::Mono] {
            let options = CompileOptions { theme: Some(theme), ..CompileOptions::default() };
            let diagram = compile_source(&source(&name), SourceFormat::Yaml, &options).unwrap();
            assert!(!render_svg(&diagram, &SvgOptions::default()).is_empty());
        }
    }
}

/// Budget of `rotation: auto` on the largest example (4 full scenes), release builds (T-312).
const AUTO_ROTATION_BUDGET_MS: f64 = 20.0;

/// Timing is only meaningful with optimizations: a debug build is ~20× slower and its time says nothing
/// about the product. It runs in release builds (`cargo test --release -p dioramic-core --test scene_examples`);
/// in debug it is ignored instead of passing against a loose bound.
#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "timing needs a release build: cargo test --release -p dioramic-core --test scene_examples"
)]
fn auto_rotation_is_fast_enough() {
    let source = source("bank-branch");
    let options = CompileOptions { rotation: Some(RotationSpec::Auto), ..CompileOptions::default() };
    let _warm = compile_source(&source, SourceFormat::Yaml, &options).unwrap();
    // The fastest of several runs: other tests of this binary and other builds share the machine, and that
    // noise only ever adds time; a real regression slows every run, the fastest included.
    let fastest = (0..9)
        .map(|_| {
            let start = Instant::now();
            let _ = compile_source(&source, SourceFormat::Yaml, &options).unwrap();
            start.elapsed().as_secs_f64() * 1000.0
        })
        .fold(f64::INFINITY, f64::min);
    assert!(fastest < AUTO_ROTATION_BUDGET_MS, "bank-branch auto took {fastest:.1} ms at best");
}

#[test]
fn checkout_static_snapshot() {
    let diagram = compile("checkout", RotationSpec::Fixed(Rotation::R0), Density::Spaced);
    insta::assert_snapshot!("checkout_blueprint_r0_spaced", render_svg(&diagram, &SvgOptions::default()));
}

#[test]
fn checkout_frame_snapshot() {
    let diagram = compile("checkout", RotationSpec::Fixed(Rotation::R0), Density::Spaced);
    let options = SvgOptions { mode: SvgMode::Frame { t: 0.25 }, ..SvgOptions::default() };
    insta::assert_snapshot!("checkout_blueprint_r0_spaced_t025", render_svg(&diagram, &options));
}

#[test]
fn build_preset_drops_nodes_in_and_holds_the_end() {
    let options = CompileOptions {
        rotation: Some(RotationSpec::Fixed(Rotation::R0)),
        animation: dioramic_core::animation::AnimationOverrides {
            preset: Some(dioramic_spec::AnimationPreset::Build),
            ..Default::default()
        },
        ..CompileOptions::default()
    };
    let diagram = compile_source(&source("checkout"), SourceFormat::Yaml, &options).unwrap();
    let plan = diagram.animation.as_ref().unwrap();
    assert!(!plan.looped, "build holds the last frame");
    let at = |t| render_svg(&diagram, &SvgOptions { mode: SvgMode::Frame { t }, ..SvgOptions::default() });
    assert!(at(0.3).contains("transform=\"translate(0,"));
    assert!(!at(0.3).contains("data-role=\"packet\""));
    let end = at(0.999);
    assert!(end.contains("translate(0,0)\" opacity=\"1\""));
    let animated = render_svg(&diagram, &SvgOptions { mode: SvgMode::Animated, ..SvgOptions::default() });
    assert!(animated.contains("@keyframes dioramic-drop") && animated.contains("class=\"build-node rank-0\""));
}
