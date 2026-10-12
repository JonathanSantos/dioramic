//! Legibility defects the collision score used to miss (T-381), checked geometrically over every example
//! × 4 rotations × 2 densities: a pill or label over a badge (`v7`, `×3`), a leader line through a pill or
//! a zone label, an edge hugging another node's outline. Whatever the drawing shows must be in the score
//! (or the lint), and the `ml-pipeline` views the QA flagged must be free of them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use dioramic_core::canvas::ItemRole;
use dioramic_core::geometry::{PlanPoint, PlanVector, Projection, Rect, ScreenPoint};
use dioramic_core::icons::Footprint;
use dioramic_core::scene::{Overlap, Scene, SceneNode};
use dioramic_core::{CompileOptions, Diagram, compile_source};
use dioramic_spec::{Density, DiagnosticCode, Rotation, RotationSpec, SourceFormat};

/// Overlap (px²) below which two boxes only touch at an antialiased border.
const TOUCH_PX2: f64 = 2.0;
/// Inset (px) of the samples taken along a leader line, so ending on a box border is not a crossing.
const LEADER_END_INSET_PX: f64 = 2.0;
/// Clearance (px) the flagged `predictions` edge must keep from the Inference API outline.
const MIN_CLEARANCE_PX: f64 = 6.0;

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

/// Every fixed view plus `auto` in both densities.
fn every_view() -> Vec<(RotationSpec, Density)> {
    let fixed = Rotation::ALL.map(RotationSpec::Fixed);
    Density::ALL
        .into_iter()
        .flat_map(|density| fixed.into_iter().chain([RotationSpec::Auto]).map(move |rotation| (rotation, density)))
        .collect()
}

fn view_name(name: &str, scene: &Scene) -> String {
    format!("{name} r{} {}", scene.rotation.degrees(), scene.density.as_str())
}

/// Badge and replica pills drawn in a node body: `<rect … height="16" … rx="8">` in its markup.
fn badge_pills(node: &SceneNode) -> Vec<Rect> {
    node.markup
        .split('<')
        .filter(|tag| tag.starts_with("rect ") && tag.contains(" rx=\"8\"") && tag.contains(" height=\"16\""))
        .filter_map(|tag| {
            let number = |name: &str| -> Option<f64> {
                let start = tag.find(&format!(" {name}=\""))? + name.len() + 3;
                tag[start..].split('"').next()?.parse().ok()
            };
            let (x, y) = (number("x")?, number("y")?);
            Some(Rect::new(x, y, x + number("width")?, y + number("height")?))
        })
        .collect()
}

fn reported(overlaps: &[Overlap], role: ItemRole, id: &str) -> bool {
    overlaps.iter().any(|overlap| [&overlap.a, &overlap.b].iter().any(|item| item.role == role && item.id == id))
}

/// Pills and node label boxes of the scene, with the id of what they label.
fn label_boxes(scene: &Scene) -> Vec<(String, Rect)> {
    let pills = scene.edges.iter().filter_map(|edge| Some((edge.id.to_string(), edge.label.as_ref()?.rect)));
    let boxes = scene.nodes.iter().filter_map(|node| Some((node.id.to_string(), node.label.as_ref()?.rect)));
    pills.chain(boxes).collect()
}

/// Labels drawn over a badge pill: `(label id, node id)`.
fn labels_on_badges(scene: &Scene) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for node in &scene.nodes {
        for badge in badge_pills(node) {
            for (id, rect) in label_boxes(scene) {
                if rect.overlap_area(&badge, 0.0) > TOUCH_PX2 {
                    found.push((id, node.id.to_string()));
                }
            }
        }
    }
    found
}

/// Floor polygon of a zone label pill.
fn zone_label_polygon(scene: &Scene, index: usize) -> Option<[ScreenPoint; 4]> {
    let label = scene.zones[index].label.as_ref()?;
    let affine = label.transform?;
    let (width, height) = label.size;
    Some([affine.apply(0.0, 0.0), affine.apply(width, 0.0), affine.apply(width, height), affine.apply(0.0, height)])
}

fn inside_convex(polygon: &[ScreenPoint], point: ScreenPoint) -> bool {
    let sides: Vec<f64> = (0..polygon.len())
        .map(|index| {
            let (a, b) = (polygon[index], polygon[(index + 1) % polygon.len()]);
            (b.x - a.x) * (point.y - a.y) - (b.y - a.y) * (point.x - a.x)
        })
        .collect();
    sides.iter().all(|side| *side > 0.0) || sides.iter().all(|side| *side < 0.0)
}

/// Points along a leader line, 1 px apart, without its ends.
fn leader_samples(from: ScreenPoint, to: ScreenPoint) -> Vec<ScreenPoint> {
    let length = from.distance(to);
    let steps = length.floor() as usize;
    (0..=steps)
        .map(|step| step as f64)
        .filter(|along| *along > LEADER_END_INSET_PX && *along < length - LEADER_END_INSET_PX)
        .map(|along| from + (to - from) * (along / length))
        .collect()
}

/// Leader lines drawn through a pill or a zone label: `(node id, crossed role, crossed id)`.
fn leaders_through_text(scene: &Scene) -> Vec<(String, ItemRole, String)> {
    let mut found = Vec::new();
    for node in &scene.nodes {
        let Some((from, to)) = node.leader else { continue };
        let samples = leader_samples(from, to);
        for edge in &scene.edges {
            let Some(pill) = edge.label.as_ref() else { continue };
            if samples.iter().any(|point| pill.rect.contains(*point)) {
                found.push((node.id.to_string(), ItemRole::EdgeLabel, edge.id.to_string()));
            }
        }
        for index in 0..scene.zones.len() {
            let Some(polygon) = zone_label_polygon(scene, index) else { continue };
            if samples.iter().any(|point| inside_convex(&polygon, *point)) {
                found.push((node.id.to_string(), ItemRole::ZoneLabel, scene.zones[index].id.to_string()));
            }
        }
    }
    found
}

#[test]
fn labels_over_badges_are_always_scored() {
    let mut missed = Vec::new();
    for name in example_names() {
        for (rotation, density) in every_view() {
            let diagram = compile(&name, rotation, density);
            let scene = &diagram.scene;
            for (label, node) in labels_on_badges(scene) {
                if !reported(&scene.collision.overlaps, ItemRole::Badge, &node) {
                    missed.push(format!("{}: '{label}' covers the badge of '{node}'", view_name(&name, scene)));
                }
            }
        }
    }
    assert!(missed.is_empty(), "{missed:#?}");
}

#[test]
fn leaders_through_pills_and_zone_labels_are_always_scored() {
    let mut missed = Vec::new();
    for name in example_names() {
        for (rotation, density) in every_view() {
            let diagram = compile(&name, rotation, density);
            let scene = &diagram.scene;
            for (node, role, crossed) in leaders_through_text(scene) {
                let leader_overlaps: Vec<Overlap> = scene
                    .collision
                    .overlaps
                    .iter()
                    .filter(|overlap| [&overlap.a, &overlap.b].iter().any(|item| item.id == node))
                    .cloned()
                    .collect();
                if !reported(&leader_overlaps, role, &crossed) {
                    missed.push(format!(
                        "{}: leader of '{node}' crosses {} '{crossed}'",
                        view_name(&name, scene),
                        role.as_str()
                    ));
                }
            }
        }
    }
    assert!(missed.is_empty(), "{missed:#?}");
}

/// Screen silhouette of a node body, as the scene builds it.
fn silhouette(scene: &Scene, node: &SceneNode) -> Vec<ScreenPoint> {
    let projection = Projection::new(scene.tile_px);
    let center = projection.unproject(node.floor_center, 0.0);
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
    outline
        .iter()
        .flat_map(|point| [projection.project(*point, 0.0), projection.project(*point, node.shape.height)])
        .collect()
}

/// Distance from `point` to the convex hull of `points` (0 inside): distance to the nearest hull side.
fn distance_to_points_hull(points: &[ScreenPoint], point: ScreenPoint) -> f64 {
    let hull = convex_hull(points.to_vec());
    if inside_convex(&hull, point) {
        return 0.0;
    }
    (0..hull.len())
        .map(|index| distance_to_segment(point, hull[index], hull[(index + 1) % hull.len()]))
        .fold(f64::INFINITY, f64::min)
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

fn convex_hull(mut points: Vec<ScreenPoint>) -> Vec<ScreenPoint> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    let cross = |o: ScreenPoint, a: ScreenPoint, b: ScreenPoint| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut hull: Vec<ScreenPoint> = Vec::new();
    for pass in 0..2 {
        let start = hull.len();
        let ordered: Vec<ScreenPoint> = if pass == 0 { points.clone() } else { points.iter().rev().copied().collect() };
        for point in ordered {
            while hull.len() >= start + 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], point) <= 0.0 {
                hull.pop();
            }
            hull.push(point);
        }
        hull.pop();
    }
    hull
}

/// The QA findings (`docs/work/reviews/QA-entrega.md`, P1-1): in the default view (0°) and in `auto`, no
/// pill or label covers the `v7` or `×3` badges, no leader runs through a pill or a zone label, and the
/// dashed `predictions` edge keeps clear of the Inference API instead of running under its base.
#[test]
fn ml_pipeline_default_and_auto_views_are_free_of_the_qa_findings() {
    for rotation in [RotationSpec::Fixed(Rotation::R0), RotationSpec::Auto] {
        let diagram = compile("ml-pipeline", rotation, Density::Spaced);
        let scene = &diagram.scene;
        let view = view_name("ml-pipeline", scene);
        assert!(labels_on_badges(scene).is_empty(), "{view}: {:?}", labels_on_badges(scene));
        assert!(leaders_through_text(scene).is_empty(), "{view}: {:?}", leaders_through_text(scene));
        assert!(
            diagram.diagnostics.iter().all(|finding| finding.code != DiagnosticCode::EdgeThroughNode),
            "{view}: {:?}",
            diagram.diagnostics
        );
        let gateway = scene.nodes.iter().find(|node| node.id.as_str() == "gateway").unwrap();
        let outline = silhouette(scene, gateway);
        let predictions = scene.edges.iter().find(|edge| edge.id.as_str() == "model_srv->drift").unwrap();
        let closest = predictions
            .path
            .sample(96, 4.0)
            .into_iter()
            .map(|point| distance_to_points_hull(&outline, point))
            .fold(f64::INFINITY, f64::min);
        assert!(closest >= MIN_CLEARANCE_PX, "{view}: predictions passes {closest:.1} px from the Inference API");
    }
}
