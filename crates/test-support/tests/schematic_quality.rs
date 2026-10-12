//! The `schematic` theme as shipped (T-413): `layout.algorithm: hub` and `routing: rails` by default. In every
//! example × rotation × density, and in the default view of every `examples/schematic/` spec (T-410), the routes
//! read at a glance: few bends and none above what the end faces need (no step just before a port), no detour
//! (measured between the ports and between the node centers), no edge crossing another edge of the same node or
//! another edge twice, no stroke glued to another (in `spaced`), no edge going around a whole zone or past its
//! nodes, and no stroke under the label of a node it does not connect (T-421, T-422). The shape of each route
//! ([`test_support::route_shape`], T-425) is tallied too: steps ("S"), U-turns and hooks at a port stay under
//! a budget, and the three default views the final engine of the T-415/T-421 round made worse (`web-platform`,
//! `09-file-sync-like-google-drive`, `10-ai-assistant-like-claude`) must have none, no label over a stroke, and
//! no rail that passes its target to come back into it from behind.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use dioramic_core::scene::{Scene, crossing_pairs};
use dioramic_spec::{Density, DiagnosticCode, Diagnostics, Rotation, ThemeName};
use test_support::quality::{
    edge_bends, edge_center_detour, edge_detour, edge_excess_bends, edge_overshoot, edge_stray,
    edges_under_node_labels, glued_pairs, overlap_length, source_reading_rank, zone_wraps,
};
use test_support::route_shape::{RouteShape, edge_arrives_behind, edge_route_shape};
use test_support::{
    View, assert_arrows_aim_at_targets, fixture, fixture_names, schematic_fixture, schematic_fixture_names,
};

/// Mean bends per edge over every view.
const BENDS_MEAN_GOAL: f64 = 1.3;
/// Most bends of one edge.
const BENDS_MAX: usize = 3;
/// Bends of one edge above the fewest its two end faces allow (T-421: two more is a step or a walk round).
const EXCESS_BENDS_MAX: usize = 1;
/// Edges allowed over [`EXCESS_BENDS_MAX`] in one suite: a rail that must pass a node standing right between
/// its ends and lands a lane over, where the clean alternative (another face of the target) is taken by a
/// sibling (`kubernetes-platform` r180 spaced `metrics->orders`: Prometheus, the storefront and the orders pods
/// stand in one column). Was 14 views before T-421.
const EXCESS_BENDS_EDGES: usize = 1;
/// Longest route over the Manhattan distance between its ends.
const DETOUR_MAX: f64 = 1.4;
/// Longest route over the Manhattan distance between the centers of its nodes (T-421: sees a U to a neighbour).
const CENTER_DETOUR_MAX: f64 = 1.6;
/// Longest stretch (tiles) of a route past its two nodes, along an axis on which they stand apart (T-421).
const STRAY_MAX: f64 = 0.0;
/// Stroke glued to another edge (tiles) in `spaced`; `compact` puts nodes about a tile apart and allows a little.
const GLUED_SPACED: f64 = 0.0;
const GLUED_COMPACT: f64 = 1.0;
/// How far (tiles) a route may stray outside the rectangle spanned by its ends: a rail arriving by a face
/// behind its node needs about the node's depth; more is a route that overshoots and turns back around
/// something (the T-408 `web-platform` drew its data edges past the data zone and back).
const OVERSHOOT_MAX: f64 = 1.5;
/// Edges running under the label of a node they do not connect, over every view of the examples (T-422). The
/// labels are placed after the rails and move aside when they can. Was 5 before T-421, 6 after it; none since
/// the hub layout keeps rails off the cells next to them (T-425).
const UNDER_FOREIGN_LABEL_MAX: usize = 0;
/// The same over the default views of the gallery of system designs (was 1 before T-425).
const UNDER_FOREIGN_LABEL_GALLERY_MAX: usize = 0;
/// Shape defects (steps, U-turns, hooks at a port: [`RouteShape`]) over every view of the examples (T-425: were
/// 322 with the engine of T-424; the ones left are in the densest hubs, mostly in `compact`).
const SHAPE_DEFECTS_MAX: usize = 20;
/// The same over the default views of the gallery (were 39 with the engine of T-424; one step left in
/// `05-rate-limiter`, where six rails share the small API gateway).
const SHAPE_DEFECTS_GALLERY_MAX: usize = 1;
/// The default views made worse by the final engine of the T-415/T-421 round (T-424): they read clean.
const CLEAN_VIEWS: [(&str, Fixtures); 3] = [
    ("web-platform", Fixtures::Examples),
    ("09-file-sync-like-google-drive", Fixtures::Gallery),
    ("10-ai-assistant-like-claude", Fixtures::Gallery),
];

/// Where a spec of [`CLEAN_VIEWS`] lives.
#[derive(Debug, Clone, Copy)]
enum Fixtures {
    /// `examples/`.
    Examples,
    /// `examples/schematic/`.
    Gallery,
}

/// Where the sources may stand in the reading of the screen at rotation 0 (0 = read first, 1 = last; T-415).
/// The goal is the first third; specs whose sources sit at both ends of the flow (a viewer that only reads
/// from the CDN at the end of the `07-video-upload` pipeline, the feature store and the drift monitor of
/// `ml-pipeline`) cannot all come first, and land a little past it.
const SOURCE_READING_RANK_MAX: f64 = 0.42;
/// Specs allowed past [`SOURCE_READING_RANK_MAX`]: `ml-pipeline` reads its clickstream first and its product apps
/// last, at the serving end of the pipeline; every mirror of its drawing puts the pair half way (0.47 to 0.53)
/// since the rails layout keeps its training and drift edges straight (T-425; it read at 0.37 with the hooks).
const SOURCE_READING_RANK_EXCEPTIONS: [(&str, f64); 1] = [("ml-pipeline", 0.5)];

/// Counts kept over every view.
#[derive(Default)]
struct Tally {
    bends: usize,
    edges: usize,
    excess_bends: Vec<String>,
    under_foreign_labels: usize,
    shape_defects: Vec<String>,
    failures: Vec<String>,
}

impl Tally {
    /// Checks one compiled view.
    fn check(&mut self, label: &str, scene: &Scene, diagnostics: &Diagnostics, glued_limit: f64) {
        for diagnostic in diagnostics.iter().filter(|d| d.code == DiagnosticCode::EdgeThroughNode) {
            self.failures.push(format!("{label}: {diagnostic}"));
        }
        assert_arrows_aim_at_targets(scene);
        for pair in crossing_pairs(&scene.edges) {
            let (first, second) = (scene.edges[pair.first].id.as_str(), scene.edges[pair.second].id.as_str());
            if pair.shared_node {
                self.failures.push(format!("{label}: shared-node crossing {first} × {second}"));
            } else if pair.count >= 2 {
                self.failures.push(format!("{label}: {first} and {second} cross {} times", pair.count));
            }
        }
        let glued = overlap_length(scene);
        if glued > glued_limit {
            let pairs: Vec<String> = glued_pairs(scene)
                .into_iter()
                .map(|(a, b, tiles)| {
                    format!("{} × {} {tiles:.2}", scene.edges[a].id.as_str(), scene.edges[b].id.as_str())
                })
                .collect();
            self.failures.push(format!("{label}: {glued:.2} tiles glued to another edge ({})", pairs.join(", ")));
        }
        for (edge, zone) in zone_wraps(scene) {
            self.failures.push(format!(
                "{label}: {} goes around zone {}",
                scene.edges[edge].id.as_str(),
                scene.zones[zone].id.as_str()
            ));
        }
        for (edge, node) in edges_under_node_labels(scene) {
            let (edge, node) = (&scene.edges[edge], &scene.nodes[node]);
            if edge.from != node.id && edge.to != node.id {
                self.under_foreign_labels += 1;
                eprintln!("  {label}: {} runs under the label of {}", edge.id.as_str(), node.id.as_str());
            }
        }
        for edge in &scene.edges {
            self.check_edge(label, scene, edge);
        }
        let rank_max = SOURCE_READING_RANK_EXCEPTIONS
            .iter()
            .find(|(name, _)| label.split(' ').next() == Some(name))
            .map_or(SOURCE_READING_RANK_MAX, |(_, max)| *max);
        if scene.rotation == Rotation::R0
            && let Some(rank) = source_reading_rank(scene)
            && rank > rank_max
        {
            self.failures.push(format!("{label}: the sources read at {rank:.2} of the picture (≤ {rank_max})"));
        }
    }

    fn check_edge(&mut self, label: &str, scene: &Scene, edge: &dioramic_core::scene::SceneEdge) {
        let id = edge.id.as_str();
        let shape = edge_route_shape(scene, edge);
        if !shape.is_clean() {
            self.shape_defects.push(format!("{label}: {id} {}", describe_shape(shape)));
        }
        let bends = edge_bends(scene, edge);
        self.bends += bends;
        self.edges += 1;
        if bends > BENDS_MAX {
            self.failures.push(format!("{label}: {id} takes {bends} bends"));
        }
        let excess = edge_excess_bends(scene, edge);
        if excess > EXCESS_BENDS_MAX {
            self.excess_bends.push(format!("{label}: {id} takes {excess} bends more than its faces need"));
        }
        let overshoot = edge_overshoot(scene, edge);
        if overshoot > OVERSHOOT_MAX {
            self.failures.push(format!("{label}: {id} strays {overshoot:.2} tiles past its ends"));
        }
        let stray = edge_stray(scene, edge);
        if stray > STRAY_MAX + 1e-6 {
            self.failures.push(format!("{label}: {id} runs {stray:.2} tiles past its nodes"));
        }
        let detour = edge_detour(scene, edge);
        if detour > DETOUR_MAX {
            self.failures.push(format!("{label}: {id} is {detour:.2}× its Manhattan span"));
        }
        let center_detour = edge_center_detour(scene, edge);
        if center_detour > CENTER_DETOUR_MAX {
            self.failures.push(format!("{label}: {id} is {center_detour:.2}× the span between its nodes"));
        }
    }

    fn assert_clean(&self, scope: &str) {
        let mean = self.bends as f64 / self.edges.max(1) as f64;
        eprintln!(
            "{scope}: {} edges, {mean:.2} bends per edge, {} under foreign labels, {} problem(s)",
            self.edges,
            self.under_foreign_labels,
            self.failures.len()
        );
        for failure in self.failures.iter().chain(&self.excess_bends).chain(&self.shape_defects) {
            eprintln!("  {failure}");
        }
        assert!(mean <= BENDS_MEAN_GOAL, "mean bends {mean:.2} > {BENDS_MEAN_GOAL}");
        assert!(self.failures.is_empty(), "{} problem(s):\n{}", self.failures.len(), self.failures.join("\n"));
        assert!(
            self.excess_bends.len() <= EXCESS_BENDS_EDGES,
            "{} edge(s) with steps (≤ {EXCESS_BENDS_EDGES}):\n{}",
            self.excess_bends.len(),
            self.excess_bends.join("\n")
        );
    }

    /// Fails when the shape defects pass `max`.
    fn assert_shapes_within(&self, max: usize) {
        assert!(
            self.shape_defects.len() <= max,
            "{} route(s) with steps, U-turns or hooks (≤ {max}):\n{}",
            self.shape_defects.len(),
            self.shape_defects.join("\n")
        );
    }
}

/// `step`, `U`, `hook` counts of a route shape, the non-zero ones.
fn describe_shape(shape: RouteShape) -> String {
    [("step", shape.steps), ("U-turn", shape.u_turns), ("hook", shape.port_hooks)]
        .iter()
        .filter(|(_, count)| *count > 0)
        .map(|(name, count)| format!("{count} {name}(s)"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[test]
fn schematic_routes_read_at_a_glance() {
    let mut tally = Tally::default();
    for name in fixture_names() {
        let model = fixture(&name).model();
        for density in Density::ALL {
            let glued_limit = match density {
                Density::Spaced => GLUED_SPACED,
                Density::Compact => GLUED_COMPACT,
            };
            for rotation in Rotation::ALL {
                let view = View::new(ThemeName::Schematic, rotation, density);
                let diagram = dioramic_core::compile_with(&model, &view.compile_options())
                    .unwrap_or_else(|diagnostics| panic!("{name}: {diagnostics}"));
                tally.check(&format!("{name} {}", view.label()), &diagram.scene, &diagram.diagnostics, glued_limit);
            }
        }
    }
    tally.assert_clean("schematic");
    tally.assert_shapes_within(SHAPE_DEFECTS_MAX);
    assert!(
        tally.under_foreign_labels == UNDER_FOREIGN_LABEL_MAX,
        "{} edges under the label of a node they do not connect (= {UNDER_FOREIGN_LABEL_MAX})",
        tally.under_foreign_labels
    );
}

/// The gallery of system designs (`examples/schematic/`, T-410) as each spec is written: its own theme,
/// rotation and density.
#[test]
fn schematic_gallery_reads_at_a_glance() {
    let mut tally = Tally::default();
    for name in schematic_fixture_names() {
        let example = schematic_fixture(&name);
        let diagram = dioramic_core::compile_source(
            &example.source,
            dioramic_spec::SourceFormat::Yaml,
            &dioramic_core::CompileOptions::default(),
        )
        .unwrap_or_else(|diagnostics| panic!("{name}: {diagnostics}"));
        let glued_limit = match diagram.scene.density {
            Density::Spaced => GLUED_SPACED,
            Density::Compact => GLUED_COMPACT,
        };
        tally.check(&example.name, &diagram.scene, &diagram.diagnostics, glued_limit);
    }
    tally.assert_clean("examples/schematic");
    tally.assert_shapes_within(SHAPE_DEFECTS_GALLERY_MAX);
    assert!(
        tally.under_foreign_labels == UNDER_FOREIGN_LABEL_GALLERY_MAX,
        "{} edges under the label of a node they do not connect (= {UNDER_FOREIGN_LABEL_GALLERY_MAX})",
        tally.under_foreign_labels
    );
}

/// T-425: the default views the final engine of the T-415/T-421 round made worse than their assets
/// (`docs/work/reviews/UI-fechamento-ajustes.md`) read clean: no step, U-turn or hook in any route, no label
/// over a stroke (`W_LABEL_OVERLAP`), and no rail that passes its target and comes back into it from behind when
/// an L or a Z reaches a front face.
#[test]
fn the_views_the_final_engine_made_worse_read_clean() {
    let mut problems = Vec::new();
    for (name, fixtures) in CLEAN_VIEWS {
        let example = match fixtures {
            Fixtures::Examples => fixture(name),
            Fixtures::Gallery => schematic_fixture(name),
        };
        let diagram = dioramic_core::compile_source(
            &example.source,
            dioramic_spec::SourceFormat::Yaml,
            &dioramic_core::CompileOptions::default(),
        )
        .unwrap_or_else(|diagnostics| panic!("{name}: {diagnostics}"));
        let scene = &diagram.scene;
        for edge in &scene.edges {
            let shape = edge_route_shape(scene, edge);
            if !shape.is_clean() {
                problems.push(format!("{name}: {} {}", edge.id.as_str(), describe_shape(shape)));
            }
            if edge_arrives_behind(scene, edge) {
                problems.push(format!("{name}: {} passes its target and arrives from behind", edge.id.as_str()));
            }
        }
        for diagnostic in diagram.diagnostics.iter().filter(|d| d.code == DiagnosticCode::LabelOverlap) {
            problems.push(format!("{name}: {diagnostic}"));
        }
    }
    assert!(problems.is_empty(), "{} problem(s):\n{}", problems.len(), problems.join("\n"));
}
