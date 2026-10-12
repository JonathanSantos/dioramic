//! Test helpers shared by every crate (`docs/work/README.md#mini-engine-para-ui-e-qa`).
//! Contract: C-007. Implementation: T-106.
//!
//! Test code may panic: every helper here panics with a readable message instead of returning `Result`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use dioramic_core::scene::{ARROW_ANGLE_TOLERANCE_DEG, CollisionReport, ItemRef, Overlap, Scene};
use dioramic_core::svg::SvgOptions;
use dioramic_core::{CompileOptions, Diagram};
use dioramic_spec::{Density, Model, Rotation, RotationSpec, SourceFormat, ThemeName};

pub mod arrows;
pub mod golden;
pub mod layout_metrics;
pub mod quality;
pub mod route_shape;

pub use arrows::{ArrowMeasurement, measure_arrows};
pub use layout_metrics::LayoutMetrics;
pub use quality::SceneQuality;

/// Themes covered by default by the gallery and by [`View::gallery_matrix`] (`--themes all` adds the rest).
pub const GALLERY_THEMES: [ThemeName; 2] = [ThemeName::Blueprint, ThemeName::Midnight];
/// Collision score (px²) under which a scene counts as collision free (float noise only).
pub const COLLISION_SCORE_EPSILON: f64 = 1e-6;

/// One camera/theme combination of a fixture (`blueprint-r90-compact`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct View {
    /// Theme.
    pub theme: ThemeName,
    /// Fixed rotation.
    pub rotation: Rotation,
    /// Density.
    pub density: Density,
}

impl View {
    /// New view.
    pub fn new(theme: ThemeName, rotation: Rotation, density: Density) -> Self {
        Self { theme, rotation, density }
    }

    /// Every view of the gallery: [`GALLERY_THEMES`] × 4 rotations × 2 densities (16), in a stable order.
    pub fn gallery_matrix() -> Vec<View> {
        Self::matrix(&GALLERY_THEMES)
    }

    /// `themes` × 4 rotations × 2 densities, theme-major then density then rotation (the gallery order).
    pub fn matrix(themes: &[ThemeName]) -> Vec<View> {
        themes
            .iter()
            .copied()
            .flat_map(|theme| Density::ALL.into_iter().map(move |density| (theme, density)))
            .flat_map(|(theme, density)| {
                Rotation::ALL.into_iter().map(move |rotation| View::new(theme, rotation, density))
            })
            .collect()
    }

    /// File stem used by the gallery: `<theme>-r<degrees>-<density>`.
    pub fn label(&self) -> String {
        format!("{}-r{}-{}", self.theme.as_str(), self.rotation.degrees(), self.density.as_str())
    }

    /// Inverse of [`View::label`] (`midnight-r90-compact`); `None` for anything else.
    pub fn parse_label(label: &str) -> Option<View> {
        let mut parts = label.splitn(3, '-');
        let (theme, rotation, density) = (parts.next()?, parts.next()?, parts.next()?);
        let theme = ThemeName::ALL.into_iter().find(|candidate| candidate.as_str() == theme)?;
        let degrees: u16 = rotation.strip_prefix('r')?.parse().ok()?;
        let rotation = Rotation::ALL.into_iter().find(|candidate| candidate.degrees() == degrees)?;
        let density = Density::ALL.into_iter().find(|candidate| candidate.as_str() == density)?;
        Some(View::new(theme, rotation, density))
    }

    /// Compile overrides that force this view.
    pub fn compile_options(&self) -> CompileOptions {
        CompileOptions {
            theme: Some(self.theme),
            rotation: Some(RotationSpec::Fixed(self.rotation)),
            density: Some(self.density),
            ..CompileOptions::default()
        }
    }
}

/// Workspace root (two levels above this crate).
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap_or_else(|_| PathBuf::from("."))
}

/// An example spec from `examples/<name>.diorama.yaml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fixture {
    /// Name (`checkout`).
    pub name: String,
    /// Absolute path.
    pub path: PathBuf,
    /// File contents.
    pub source: String,
}

impl Fixture {
    /// Parsed, normalized and validated model (panics with the diagnostics on error).
    pub fn model(&self) -> Model {
        match dioramic_spec::load(&self.source, dioramic_spec::SourceFormat::Yaml) {
            Ok(validated) => validated.model,
            Err(diagnostics) => panic!("fixture '{}' is invalid:\n{diagnostics}", self.name),
        }
    }

    /// Compiled diagram for `view` (panics with the diagnostics on error).
    ///
    /// ```ignore
    /// let scene = fixture("checkout").diagram(View::default()).scene;
    /// ```
    pub fn diagram(&self, view: View) -> Diagram {
        dioramic_core::compile_source(&self.source, SourceFormat::Yaml, &view.compile_options()).unwrap_or_else(
            |diagnostics| panic!("fixture '{}' ({}) does not compile:\n{diagnostics}", self.name, view.label()),
        )
    }

    /// Scene for `view`.
    pub fn scene(&self, view: View) -> Scene {
        self.diagram(view).scene
    }

    /// Static SVG for `view` (default [`SvgOptions`]).
    pub fn svg(&self, view: View) -> String {
        dioramic_core::render_svg(&self.diagram(view), &SvgOptions::default())
    }
}

/// Loads `examples/<name>.diorama.yaml` (panics if missing).
pub fn fixture(name: &str) -> Fixture {
    let path = workspace_root().join("examples").join(format!("{name}.diorama.yaml"));
    let source = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("fixture {}: {error}", path.display()));
    Fixture { name: name.to_owned(), path, source }
}

/// Loads `examples/schematic/<name>.diorama.yaml`, the gallery of system designs drawn in the `schematic` theme
/// (T-410; panics if missing).
pub fn schematic_fixture(name: &str) -> Fixture {
    let path = workspace_root().join("examples").join("schematic").join(format!("{name}.diorama.yaml"));
    let source = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("fixture {}: {error}", path.display()));
    Fixture { name: format!("schematic/{name}"), path, source }
}

/// Names of every `examples/schematic/*.diorama.yaml`, sorted (empty when the folder is missing).
pub fn schematic_fixture_names() -> Vec<String> {
    spec_names_in(&workspace_root().join("examples").join("schematic"))
}

/// Names of every `examples/*.diorama.yaml`, sorted.
pub fn fixture_names() -> Vec<String> {
    spec_names_in(&workspace_root().join("examples"))
}

/// Names (without `.diorama.yaml`) of the specs directly inside `directory`, sorted.
fn spec_names_in(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(directory)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|entry| entry.file_name().to_str()?.strip_suffix(".diorama.yaml").map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// Panics listing every residual overlap when `scene.collision.score > 0`
/// (labels, pills, zone labels and node bodies).
///
/// Message: one line per overlap, e.g. `node-label 'ws1' × edge-label 'teller_sw->ws2': 42 px²`.
pub fn assert_no_collisions(scene: &Scene) {
    if let Some(report) = describe_collisions(&scene.collision) {
        panic!("{report}");
    }
}

/// Readable description of the residual collisions, or `None` when the scene is collision free.
pub fn describe_collisions(collision: &CollisionReport) -> Option<String> {
    let has_overlaps = collision.overlaps.iter().any(|overlap| overlap.area > COLLISION_SCORE_EPSILON);
    if collision.score <= COLLISION_SCORE_EPSILON && !has_overlaps {
        return None;
    }
    let mut report = format!(
        "scene has residual collisions: score {:.1} px², {} overlap(s)",
        collision.score,
        collision.overlaps.len()
    );
    for overlap in &collision.overlaps {
        let _ = write!(report, "\n  {}", describe_overlap(overlap));
    }
    Some(report)
}

/// One overlap as `node-label 'ws1' × edge-label 'teller_sw->ws2': 42 px²`.
pub fn describe_overlap(overlap: &Overlap) -> String {
    format!("{} × {}: {:.0} px²", item_label(&overlap.a), item_label(&overlap.b), overlap.area)
}

fn item_label(item: &ItemRef) -> String {
    format!("{} '{}'", item.role.as_str(), item.id)
}

/// Panics unless every arrow head points at its target's center and the path arrives tangent to it,
/// within [`dioramic_core::scene::ARROW_ANGLE_TOLERANCE_DEG`], and no head is hidden behind a node body
/// painted after it (`docs/06-renderizacao.md#setas-e-portas`).
///
/// Message: one line per bad head with the edge id, the end and both errors in degrees.
pub fn assert_arrows_aim_at_targets(scene: &Scene) {
    let failures: Vec<String> = measure_arrows(scene)
        .iter()
        .filter(|measurement| !measurement.is_ok(ARROW_ANGLE_TOLERANCE_DEG))
        .map(describe_arrow)
        .collect();
    if !failures.is_empty() {
        panic!(
            "{} arrow head(s) off target (tolerance {ARROW_ANGLE_TOLERANCE_DEG}°):\n  {}",
            failures.len(),
            failures.join("\n  ")
        );
    }
}

/// One arrow head as `edge 'a->b' → 'b': center error 0.12°, tangent error 0.40°[, tip hidden behind node 'x']`.
pub fn describe_arrow(measurement: &ArrowMeasurement) -> String {
    let mut line = format!(
        "edge '{}' → '{}': center error {:.2}°, tangent error {:.2}°",
        measurement.edge, measurement.target, measurement.center_error_deg, measurement.tangent_error_deg
    );
    if let Some(node) = &measurement.hidden_by {
        let _ = write!(line, ", tip hidden behind node '{node}'");
    }
    line
}

/// Runs `produce` twice and panics with a diff summary if the outputs differ (byte-identical output).
pub fn assert_deterministic<T, F>(produce: F)
where
    T: PartialEq + std::fmt::Debug,
    F: Fn() -> T,
{
    let first = produce();
    let second = produce();
    assert_eq!(first, second, "output is not deterministic");
}

/// Compares `actual` with `tests/golden/<name>` (text diff for `.svg`, tolerant pixel diff for `.png`).
/// With `DIORAMIC_UPDATE_GOLDEN=1` (set by `cargo xtask golden --update`) it rewrites the file instead.
///
/// ```ignore
/// golden!("checkout-blueprint-r0.svg", svg);
/// ```
#[macro_export]
macro_rules! golden {
    ($name:expr, $actual:expr) => {
        $crate::golden::check($name, ::std::convert::AsRef::<[u8]>::as_ref(&$actual))
    };
}

#[cfg(test)]
mod tests {
    use dioramic_core::canvas::ItemRole;
    use dioramic_core::color::Color;
    use dioramic_core::geometry::{CubicBezier, PlanPoint, PlanVector, Projection, Rect, ScreenPoint, ScreenVector};
    use dioramic_core::icons::{Footprint, NodeShape};
    use dioramic_core::scene::{ArrowHead, EDGE_Z, EdgePath, SceneEdge, SceneGrid, SceneNode};
    use dioramic_core::theme::Theme;
    use dioramic_spec::{Diagnostics, FlowMode, GridOptions};

    use super::*;

    const TILE: f64 = 56.0;
    const TALL: f64 = 1.0;
    const LOW: f64 = 0.2;
    /// The prototype pushes the tip this far (tiles) outside the port.
    const TIP_GAP: f64 = 0.08;
    const HEAD_LENGTH: f64 = 0.3;
    const HEAD_HALF_WIDTH: f64 = 0.13;

    fn projection() -> Projection {
        Projection::new(TILE)
    }

    /// Floor point (tiles, relative to the target at the origin) at edge height, in screen space.
    fn on_floor(x: f64, y: f64) -> ScreenPoint {
        projection().project(PlanPoint::new(x, y), EDGE_Z)
    }

    fn node(id: &str, center: PlanPoint, height: f64) -> SceneNode {
        let floor_center = projection().project(center, 0.0);
        SceneNode {
            id: id.into(),
            plan_center: center,
            floor_center,
            shape: NodeShape {
                footprint: Footprint::Rect { half_width: 0.4, half_depth: 0.4 },
                height,
                screen_bbox: Rect::centered(floor_center, 80.0, 120.0),
            },
            markup: String::new(),
            label: None,
            leader: None,
            rank: 0,
        }
    }

    /// An edge arriving at the target (origin) from plan direction `from_direction`, built like the
    /// prototype: port on the footprint edge, tip slightly outside, head aimed at the center, curve
    /// arriving tangent. `twist_deg` rotates the head; `bend` pushes the last control point sideways.
    fn edge_into_origin(from_direction: PlanVector, twist_deg: f64, bend: f64) -> SceneEdge {
        let outward = from_direction.normalized();
        let aim = outward * -1.0;
        let port = 0.4 / outward.x.abs().max(outward.y.abs());
        let tip_plan = outward * (port + TIP_GAP);
        let base_plan = tip_plan - aim * HEAD_LENGTH;
        let side = aim.perpendicular() * HEAD_HALF_WIDTH;
        let tip = on_floor(tip_plan.x, tip_plan.y);
        let base_middle = on_floor(base_plan.x, base_plan.y);
        let twisted_tip = base_middle + rotate(tip - base_middle, twist_deg);
        let base = [
            on_floor(base_plan.x + side.x, base_plan.y + side.y),
            on_floor(base_plan.x - side.x, base_plan.y - side.y),
        ];
        let start_plan = outward * 4.0;
        let control_plan = base_plan - aim * 1.0;
        let control2 = on_floor(control_plan.x, control_plan.y) + ScreenVector::new(0.0, bend);
        SceneEdge {
            id: "api->db".into(),
            from: "api".into(),
            to: "db".into(),
            path: EdgePath::Curve(CubicBezier::new(
                on_floor(start_plan.x, start_plan.y),
                on_floor(start_plan.x * 0.6, start_plan.y * 0.6),
                control2,
                base_middle,
            )),
            heads: vec![ArrowHead { tip: twisted_tip, base, target_center: on_floor(0.0, 0.0) }],
            midway: Vec::new(),
            kind: dioramic_spec::EdgeKind::Service,
            color: Color::BLACK,
            width: 2.0,
            dashed: false,
            flow: FlowMode::Off,
            label: None,
        }
    }

    fn rotate(vector: ScreenVector, degrees: f64) -> ScreenVector {
        let (sin, cos) = degrees.to_radians().sin_cos();
        ScreenVector::new(vector.x * cos - vector.y * sin, vector.x * sin + vector.y * cos)
    }

    fn scene(nodes: Vec<SceneNode>, edges: Vec<SceneEdge>, collision: CollisionReport) -> Scene {
        Scene {
            theme: Theme::blueprint(),
            rotation: Rotation::R0,
            density: Density::Spaced,
            tile_px: TILE,
            viewbox: Rect::new(-300.0, -300.0, 300.0, 300.0),
            title: None,
            zones: Vec::new(),
            nodes,
            edges,
            defs: Vec::new(),
            collision,
            rotation_scores: None,
            diagnostics: Diagnostics::new(),
            grid: SceneGrid::of(&GridOptions::default(), &Theme::blueprint()),
            legend: None,
        }
    }

    fn arrow_scene(target_height: f64, edge: SceneEdge) -> Scene {
        let nodes =
            vec![node("db", PlanPoint::new(0.0, 0.0), target_height), node("api", PlanPoint::new(0.0, 4.0), TALL)];
        scene(nodes, vec![edge], CollisionReport::default())
    }

    fn front_left() -> PlanVector {
        PlanVector::new(0.0, 1.0)
    }

    fn behind() -> PlanVector {
        PlanVector::new(0.0, -1.0)
    }

    fn overlap(a: (ItemRole, &str), b: (ItemRole, &str), area: f64) -> Overlap {
        Overlap { a: ItemRef { role: a.0, id: a.1.into() }, b: ItemRef { role: b.0, id: b.1.into() }, area }
    }

    #[test]
    fn collision_free_scene_passes() {
        assert_no_collisions(&scene(Vec::new(), Vec::new(), CollisionReport::default()));
    }

    #[test]
    #[should_panic(expected = "node-label 'ws1' × edge-label 'teller_sw->ws2': 42 px²")]
    fn collisions_are_listed_with_roles_ids_and_area() {
        let collision = CollisionReport {
            score: 42.0,
            overlaps: vec![overlap((ItemRole::NodeLabel, "ws1"), (ItemRole::EdgeLabel, "teller_sw->ws2"), 42.0)],
        };
        assert_no_collisions(&scene(Vec::new(), Vec::new(), collision));
    }

    #[test]
    #[should_panic(expected = "score 7.0 px²")]
    fn positive_score_without_overlaps_still_fails() {
        assert_no_collisions(&scene(Vec::new(), Vec::new(), CollisionReport { score: 7.0, overlaps: Vec::new() }));
    }

    #[test]
    fn well_aimed_arrow_passes() {
        let scene = arrow_scene(TALL, edge_into_origin(front_left(), 0.0, 0.0));
        let measurements = measure_arrows(&scene);
        assert_eq!(measurements.len(), 1);
        assert!(measurements[0].max_error_deg() < 1e-6, "{measurements:?}");
        assert_arrows_aim_at_targets(&scene);
    }

    #[test]
    fn diagonal_arrival_passes() {
        assert_arrows_aim_at_targets(&arrow_scene(TALL, edge_into_origin(PlanVector::new(1.0, 0.6), 0.0, 0.0)));
    }

    #[test]
    #[should_panic(expected = "edge 'api->db' → 'db': center error")]
    fn twisted_head_fails_with_edge_id_and_degrees() {
        let scene = arrow_scene(TALL, edge_into_origin(front_left(), 3.0, 0.0));
        let measurement = &measure_arrows(&scene)[0];
        // Twisting about the base moves the tip, so the center error grows a bit beyond the twist.
        assert!((measurement.tangent_error_deg - 3.0).abs() < 1e-6, "{measurement:?}");
        assert!(measurement.center_error_deg > ARROW_ANGLE_TOLERANCE_DEG, "{measurement:?}");
        assert_arrows_aim_at_targets(&scene);
    }

    #[test]
    #[should_panic(expected = "tangent error")]
    fn curve_arriving_at_an_angle_fails() {
        let scene = arrow_scene(TALL, edge_into_origin(front_left(), 0.0, 15.0));
        let measurement = &measure_arrows(&scene)[0];
        assert!(measurement.center_error_deg < 1e-6 && measurement.tangent_error_deg > 1.0, "{measurement:?}");
        assert_arrows_aim_at_targets(&scene);
    }

    #[test]
    #[should_panic(expected = "tip hidden behind node 'db'")]
    fn tip_behind_tall_target_fails() {
        assert_arrows_aim_at_targets(&arrow_scene(TALL, edge_into_origin(behind(), 0.0, 0.0)));
    }

    #[test]
    fn tip_behind_low_target_is_accepted() {
        assert_arrows_aim_at_targets(&arrow_scene(LOW, edge_into_origin(behind(), 0.0, 0.0)));
    }

    #[test]
    #[should_panic(expected = "tip hidden behind node 'rack'")]
    fn tip_covered_by_another_node_fails() {
        let edge = edge_into_origin(front_left(), 0.0, 0.0);
        let mut scene = arrow_scene(TALL, edge);
        scene.nodes.push(node("rack", PlanPoint::new(0.3, 0.9), TALL));
        assert_arrows_aim_at_targets(&scene);
    }

    #[test]
    fn gallery_matrix_has_sixteen_unique_views() {
        let views = View::gallery_matrix();
        assert_eq!(views.len(), 16);
        let labels: std::collections::BTreeSet<String> = views.iter().map(View::label).collect();
        assert_eq!(labels.len(), 16);
        assert!(labels.contains("midnight-r270-compact"));
    }

    #[test]
    fn matrix_of_every_theme_has_eight_views_per_theme() {
        let views = View::matrix(&ThemeName::ALL);
        assert_eq!(views.len(), ThemeName::ALL.len() * 8);
        assert_eq!(views[..16], View::gallery_matrix()[..]);
    }

    #[test]
    fn view_labels_round_trip() {
        for view in View::matrix(&ThemeName::ALL) {
            assert_eq!(View::parse_label(&view.label()), Some(view));
        }
        for bad in ["midnight-r45-compact", "neon-r0-spaced", "paper-0-spaced", "mono-r90", "mono-r90-compact-x"] {
            assert_eq!(View::parse_label(bad), None, "{bad}");
        }
    }

    #[test]
    fn deterministic_helper_accepts_pure_functions() {
        assert_deterministic(|| format!("{:.1}", 1.25_f64));
    }

    #[test]
    fn workspace_root_has_cargo_toml() {
        assert!(workspace_root().join("Cargo.toml").is_file());
    }
}
