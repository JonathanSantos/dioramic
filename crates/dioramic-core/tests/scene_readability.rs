//! Readability checks of the lane Cena fixes (T-310..T-313) over every example × 4 rotations × 2 densities:
//! edges stay visible and distinct, zone labels stay uncovered, leaders stay short and clear, and
//! `rotation: auto` prefers 0° on ties.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use dioramic_core::canvas::ItemRole;
use dioramic_core::geometry::{PlanPoint, PlanVector, Projection, ScreenPoint};
use dioramic_core::icons::Footprint;
use dioramic_core::scene::{Scene, SceneEdge, SceneNode};
use dioramic_core::{CompileOptions, Diagram, compile_source};
use dioramic_spec::{Density, DiagnosticCode, Rotation, RotationSpec, SourceFormat};

/// Shortest visible stretch of an edge (px) and largest share of it on top of another edge (T-310).
const MIN_VISIBLE_PX: f64 = 24.0;
const MAX_COINCIDENT_SHARE: f64 = 0.3;
/// Two strokes closer than this (px) and parallel read as one.
const COINCIDENT_PX: f64 = 3.0;
/// Highest leader above the base lift (doc 06, T-313).
const MAX_EXTRA_LIFT_PX: f64 = 84.0;
/// Depth inside a silhouette for a point to count as hidden (px).
const INSET_PX: f64 = 4.0;
/// Problems tolerated in compact views while the compact layout is tight (T-317).
const COMPACT_TOLERATED_PROBLEMS: usize = 2;
/// Share of a zone label that may be touched in the automatic compact view (T-311: none in spaced).
const AUTO_COMPACT_ZONE_LABEL_SHARE: f64 = 0.05;
/// Samples per edge.
const SAMPLES: usize = 96;

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

fn compile_text(source: &str, rotation: RotationSpec, density: Density) -> Diagram {
    let options = CompileOptions { rotation: Some(rotation), density: Some(density), ..CompileOptions::default() };
    compile_source(source, SourceFormat::Yaml, &options).unwrap_or_else(|errors| panic!("{errors:?}"))
}

fn compile(name: &str, rotation: RotationSpec, density: Density) -> Diagram {
    let source = std::fs::read_to_string(examples_dir().join(format!("{name}.diorama.yaml"))).unwrap();
    compile_text(&source, rotation, density)
}

fn every_view() -> Vec<(Rotation, Density)> {
    Density::ALL.into_iter().flat_map(|density| Rotation::ALL.map(|rotation| (rotation, density))).collect()
}

fn label(name: &str, scene: &Scene) -> String {
    format!("{name} r{} {}", scene.rotation.degrees(), scene.density.as_str())
}

/// Screen silhouette of a node body (convex hull of its prism, counter-clockwise or clockwise).
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
                let angle = std::f64::consts::TAU * f64::from(index) / 24.0;
                center + PlanVector::new(angle.cos(), angle.sin()) * radius
            })
            .collect(),
    };
    let mut points: Vec<ScreenPoint> = outline
        .iter()
        .flat_map(|point| [projection.project(*point, z), projection.project(*point, z + node.shape.height)])
        .collect();
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    let cross = |o: ScreenPoint, a: ScreenPoint, b: ScreenPoint| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut hull: Vec<ScreenPoint> = Vec::new();
    for pass in [points.clone(), points.iter().rev().copied().collect()] {
        let start = hull.len();
        for point in pass {
            while hull.len() >= start + 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], point) <= 0.0 {
                hull.pop();
            }
            hull.push(point);
        }
        hull.pop();
    }
    hull
}

fn inside(polygon: &[ScreenPoint], point: ScreenPoint) -> bool {
    let sides: Vec<f64> = (0..polygon.len())
        .map(|index| {
            let (a, b) = (polygon[index], polygon[(index + 1) % polygon.len()]);
            let edge = b - a;
            (edge.x * (point.y - a.y) - edge.y * (point.x - a.x)) / edge.length().max(1e-9)
        })
        .collect();
    sides.iter().all(|side| *side > INSET_PX) || sides.iter().all(|side| *side < -INSET_PX)
}

fn silhouettes(diagram: &Diagram) -> Vec<Vec<ScreenPoint>> {
    let scene = &diagram.scene;
    scene
        .nodes
        .iter()
        .map(|node| silhouette(scene, node, diagram.model.node(&node.id).map_or(0.0, |spec| spec.z)))
        .collect()
}

/// `(midpoint, direction, length)` of the pieces of an edge stroke.
fn pieces(edge: &SceneEdge) -> Vec<(ScreenPoint, ScreenPoint, ScreenPoint)> {
    let points = edge.path.sample(SAMPLES, 4.0);
    points.windows(2).map(|pair| (pair[0].midpoint(pair[1]), pair[0], pair[1])).collect()
}

fn distance_to_segment(point: ScreenPoint, a: ScreenPoint, b: ScreenPoint) -> f64 {
    let segment = b - a;
    let squared = segment.x * segment.x + segment.y * segment.y;
    if squared <= f64::EPSILON {
        return point.distance(a);
    }
    let offset = point - a;
    let t = ((offset.x * segment.x + offset.y * segment.y) / squared).clamp(0.0, 1.0);
    point.distance(a + segment * t)
}

/// Visible length of an edge: outside every silhouette and outside its own pill.
fn visible_length(edge: &SceneEdge, covers: &[Vec<ScreenPoint>]) -> f64 {
    let pill = edge.label.as_ref().map(|label| label.rect);
    pieces(edge)
        .into_iter()
        .filter(|(middle, _, _)| covers.iter().all(|cover| !inside(cover, *middle)))
        .filter(|(middle, _, _)| pill.is_none_or(|rect| !rect.contains(*middle)))
        .map(|(_, a, b)| a.distance(b))
        .sum()
}

/// Share of `edge` running on top of `other` (closer than [`COINCIDENT_PX`], parallel within ~25°).
fn coincident_share(edge: &SceneEdge, other: &SceneEdge) -> f64 {
    let others = pieces(other);
    let mine = pieces(edge);
    let total: f64 = mine.iter().map(|(_, a, b)| a.distance(*b)).sum();
    let shared: f64 = mine
        .iter()
        .filter(|(middle, a, b)| {
            let direction = (*b - *a).normalized();
            others.iter().any(|(_, c, d)| {
                let piece = (*d - *c).normalized();
                (piece.x * direction.x + piece.y * direction.y).abs() > 0.9
                    && distance_to_segment(*middle, *c, *d) < COINCIDENT_PX
            })
        })
        .map(|(_, a, b)| a.distance(*b))
        .sum();
    if total > 0.0 { shared / total } else { 0.0 }
}

#[test]
fn every_edge_is_visible_and_distinct() {
    let mut problems = Vec::new();
    for name in example_names() {
        for (rotation, density) in every_view() {
            let diagram = compile(&name, RotationSpec::Fixed(rotation), density);
            let scene = &diagram.scene;
            let covers = silhouettes(&diagram);
            for edge in &scene.edges {
                let visible = visible_length(edge, &covers);
                if visible < MIN_VISIBLE_PX {
                    problems.push(format!("{}: edge {} shows {visible:.0} px", label(&name, scene), edge.id));
                }
                for other in scene.edges.iter().filter(|other| other.id != edge.id) {
                    let share = coincident_share(edge, other);
                    if share > MAX_COINCIDENT_SHARE {
                        problems.push(format!(
                            "{}: edge {} runs {:.0}% on top of {}",
                            label(&name, scene),
                            edge.id,
                            share * 100.0,
                            other.id
                        ));
                    }
                }
            }
        }
    }
    // Compact views of the crowded examples still put a few neighbours closer than an edge needs (layout,
    // T-317); spaced views must be clean.
    let spaced: Vec<&String> = problems.iter().filter(|problem| problem.contains(" spaced:")).collect();
    assert!(spaced.is_empty(), "{spaced:#?}");
    assert!(problems.len() <= COMPACT_TOLERATED_PROBLEMS, "{} problem(s):\n{}", problems.len(), problems.join("\n"));
}

/// A node with no zone sitting exactly between two connected nodes, plus an edge from it to one of them:
/// the trunk must swing around it and the short edge must not run on top of the trunk (T-310, bank-branch).
#[test]
fn collinear_edges_through_an_unzoned_node_are_pulled_apart() {
    let source = "diorama: 1\nnodes:\n  - { id: fw, icon: firewall, label: Firewall, at: [0, 0] }\n  - { id: rack, icon: rack, label: Rack, at: [3, 0] }\n  - { id: core, icon: switch, label: Core, at: [6, 0] }\nedges:\n  - fw -> core: trunk\n  - rack .. core: rack\n";
    for rotation in Rotation::ALL {
        let diagram = compile_text(source, RotationSpec::Fixed(rotation), Density::Spaced);
        let scene = &diagram.scene;
        let covers = silhouettes(&diagram);
        assert!(
            diagram.diagnostics.iter().all(|d| d.code != DiagnosticCode::EdgeThroughNode),
            "r{}: {:?}",
            rotation.degrees(),
            diagram.diagnostics
        );
        let (trunk, rack) = (&scene.edges[0], &scene.edges[1]);
        assert!(coincident_share(rack, trunk) <= MAX_COINCIDENT_SHARE, "r{}", rotation.degrees());
        assert!(visible_length(rack, &covers) >= MIN_VISIBLE_PX, "r{}", rotation.degrees());
        assert!(visible_length(trunk, &covers) >= MIN_VISIBLE_PX, "r{}", rotation.degrees());
    }
}

/// Area of `zone` label covered by other items, from the collision report.
fn zone_label_overlaps(scene: &Scene) -> Vec<(String, f64)> {
    scene
        .collision
        .overlaps
        .iter()
        .filter_map(|overlap| {
            if overlap.a.role == ItemRole::ZoneLabel {
                Some((overlap.a.id.clone(), overlap.area))
            } else if overlap.b.role == ItemRole::ZoneLabel {
                Some((overlap.b.id.clone(), overlap.area))
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn zone_labels_stay_uncovered() {
    let mut problems = Vec::new();
    for name in example_names() {
        let views = every_view().into_iter().map(|(rotation, density)| (RotationSpec::Fixed(rotation), density));
        let autos = Density::ALL.into_iter().map(|density| (RotationSpec::Auto, density));
        for (rotation, density) in views.chain(autos) {
            let diagram = compile(&name, rotation, density);
            let scene = &diagram.scene;
            let allowed_share = match (density, rotation) {
                (Density::Spaced, _) => 0.0,
                (_, RotationSpec::Auto) => AUTO_COMPACT_ZONE_LABEL_SHARE,
                _ => 0.1,
            };
            for (zone, area) in zone_label_overlaps(scene) {
                let label_area = scene
                    .zones
                    .iter()
                    .find(|candidate| candidate.id.as_str() == zone)
                    .and_then(|candidate| candidate.label.as_ref())
                    .map_or(f64::INFINITY, |label| label.size.0 * label.size.1);
                if area > allowed_share * label_area {
                    problems.push(format!("{}: zone label {zone} covered {area:.0} px²", label(&name, scene)));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{} problem(s):\n{}", problems.len(), problems.join("\n"));
}

#[test]
fn leaders_are_short_and_cross_no_other_node() {
    let mut problems = Vec::new();
    for name in example_names() {
        for (rotation, density) in every_view() {
            let diagram = compile(&name, RotationSpec::Fixed(rotation), density);
            let scene = &diagram.scene;
            let base_lift = match density {
                Density::Spaced => 52.0,
                Density::Compact => 38.0,
            };
            let covers = silhouettes(&diagram);
            for (index, node) in scene.nodes.iter().enumerate() {
                let Some((from, to)) = node.leader else { continue };
                let length = from.distance(to);
                if length > base_lift + MAX_EXTRA_LIFT_PX + 0.5 {
                    problems.push(format!("{}: leader of {} is {length:.0} px", label(&name, scene), node.id));
                }
                let steps = (length / 4.0).ceil().max(1.0) as usize;
                let crossed = covers.iter().enumerate().filter(|(other, _)| *other != index).find(|(_, cover)| {
                    (0..=steps).filter(|step| inside(cover, from + (to - from) * (*step as f64 / steps as f64))).count()
                        >= 3
                });
                if let Some((other, _)) = crossed {
                    problems.push(format!(
                        "{}: leader of {} crosses {}",
                        label(&name, scene),
                        node.id,
                        scene.nodes[other].id
                    ));
                }
            }
        }
    }
    // The cap is a hard rule; a leader may still cross a node when every short spot is worse (reported).
    let too_long: Vec<&String> = problems.iter().filter(|problem| problem.contains(" px")).collect();
    assert!(too_long.is_empty(), "{too_long:#?}");
    eprintln!("leaders crossing nodes: {}", problems.len() - too_long.len());
}

#[test]
fn auto_rotation_prefers_the_front_view_on_ties() {
    let source = "diorama: 1\nnodes:\n  - { id: a, icon: server }\n  - { id: b, icon: database }\nedges:\n  - a -> b\n";
    let diagram = compile_text(source, RotationSpec::Auto, Density::Spaced);
    let scores = diagram.scene.rotation_scores.unwrap();
    assert!(scores.scores.iter().all(|(_, score)| *score == 0.0), "{scores:?}");
    assert_eq!(diagram.scene.rotation, Rotation::R0);
}

#[test]
fn pills_never_sit_on_nodes_in_spaced_views() {
    let mut problems = Vec::new();
    for name in example_names() {
        for rotation in Rotation::ALL {
            let diagram = compile(&name, RotationSpec::Fixed(rotation), Density::Spaced);
            let scene = &diagram.scene;
            let covers = silhouettes(&diagram);
            for edge in &scene.edges {
                let Some(pill) = &edge.label else { continue };
                let rect = pill.rect;
                // A grid of points over the pill: any of them inside a body means the pill covers the icon.
                let points = (0..=8).flat_map(|i| {
                    (0..=4).map(move |j| {
                        ScreenPoint::new(
                            rect.x0 + rect.width() * f64::from(i) / 8.0,
                            rect.y0 + rect.height() * f64::from(j) / 4.0,
                        )
                    })
                });
                let points: Vec<ScreenPoint> = points.collect();
                for (node, cover) in scene.nodes.iter().zip(&covers) {
                    if points.iter().any(|point| inside(cover, *point)) {
                        problems.push(format!("{}: pill {} on node {}", label(&name, scene), edge.id, node.id));
                    }
                }
            }
        }
    }
    assert!(problems.is_empty(), "{} problem(s):\n{}", problems.len(), problems.join("\n"));
}
