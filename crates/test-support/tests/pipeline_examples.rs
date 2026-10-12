//! QA (T-301): cross-crate integration over every `examples/*.diorama.yaml`:
//! spec (parse/normalize/validate) → core (layout/scene/SVG) → render (`Engine::render`).
//!
//! Every property here must hold for all examples, so a new example is covered automatically.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use dioramic_core::geometry::PlanRect;
use dioramic_core::icons::Footprint;
use dioramic_core::layout::{self, PlanLayout};
use dioramic_render::{Engine, OutputFormat, PreviewMode, RenderRequest};
use dioramic_spec::{Density, Model, Rotation, RotationSpec, SourceFormat, ThemeName};
use test_support::{
    View, assert_arrows_aim_at_targets, assert_deterministic, assert_no_collisions, fixture, fixture_names, golden,
    measure_arrows,
};

/// Footprints are inflated by this margin (tiles) before the overlap check (C-004 / T-103 acceptance).
const FOOTPRINT_MARGIN_TILES: f64 = 0.2;
/// Float slack when comparing plan coordinates.
const PLAN_EPSILON: f64 = 1e-6;

fn auto_view_options(density: Density) -> dioramic_core::CompileOptions {
    dioramic_core::CompileOptions {
        rotation: Some(RotationSpec::Auto),
        density: Some(density),
        ..dioramic_core::CompileOptions::default()
    }
}

#[test]
fn there_are_examples_to_test() {
    let names = fixture_names();
    assert!(names.len() >= 6, "expected the 6 shipped examples, found {names:?}");
}

#[test]
fn every_example_loads_without_errors() {
    for name in fixture_names() {
        let fixture = fixture(&name);
        let validated = dioramic_spec::load(&fixture.source, SourceFormat::Yaml)
            .unwrap_or_else(|diagnostics| panic!("{name}: {diagnostics}"));
        assert!(!validated.model.nodes.is_empty(), "{name}: no nodes");
        // Warnings are allowed, but every diagnostic must carry a stable code and a path.
        for diagnostic in validated.warnings.iter() {
            assert!(diagnostic.code.as_str().starts_with(['E', 'W', 'H']), "{name}: {diagnostic:?}");
            assert!(!diagnostic.path.is_empty(), "{name}: diagnostic without path: {diagnostic:?}");
        }
    }
}

#[test]
fn arrows_aim_at_targets_in_every_example_and_view() {
    for name in fixture_names() {
        let fixture = fixture(&name);
        for view in View::gallery_matrix() {
            let scene = fixture.scene(view);
            assert!(!measure_arrows(&scene).is_empty() || scene.edges.is_empty(), "{name} {}", view.label());
            assert_arrows_aim_at_targets(&scene);
        }
    }
}

#[test]
fn arrows_aim_at_targets_with_auto_rotation() {
    for name in fixture_names() {
        for density in Density::ALL {
            let diagram =
                dioramic_core::compile_source(&fixture(&name).source, SourceFormat::Yaml, &auto_view_options(density))
                    .unwrap();
            assert_arrows_aim_at_targets(&diagram.scene);
        }
    }
}

#[test]
fn every_head_belongs_to_an_edge_end_that_asks_for_it() {
    for name in fixture_names() {
        let fixture = fixture(&name);
        let scene = fixture.scene(View::default());
        let model = fixture.model();
        for edge in &scene.edges {
            let spec_edge = model.edges.iter().find(|candidate| candidate.id == edge.id).unwrap();
            let expected_heads = match spec_edge.arrow {
                dioramic_spec::ArrowEnds::None => 0,
                dioramic_spec::ArrowEnds::End | dioramic_spec::ArrowEnds::Start => 1,
                dioramic_spec::ArrowEnds::Both => 2,
            };
            assert_eq!(edge.heads.len(), expected_heads, "{name}: edge {}", edge.id);
        }
    }
}

#[test]
fn static_svg_is_deterministic_for_every_example() {
    for name in fixture_names() {
        let fixture = fixture(&name);
        assert_deterministic(|| fixture.svg(View::default()));
        let compact = View::new(ThemeName::Midnight, Rotation::R90, Density::Compact);
        assert_deterministic(|| fixture.svg(compact));
    }
}

#[test]
fn auto_rotation_reports_four_scores_and_picks_the_minimum() {
    for name in fixture_names() {
        for density in Density::ALL {
            let diagram =
                dioramic_core::compile_source(&fixture(&name).source, SourceFormat::Yaml, &auto_view_options(density))
                    .unwrap();
            let scores = diagram.scene.rotation_scores.as_ref().unwrap_or_else(|| panic!("{name}: no rotation_scores"));
            let minimum = scores.scores.iter().map(|(_, score)| *score).fold(f64::INFINITY, f64::min);
            assert!(
                (diagram.scene.collision.score - minimum).abs() < 1e-9,
                "{name} {density:?}: chose {:?} with {} but minimum is {minimum} ({:?})",
                diagram.scene.rotation,
                diagram.scene.collision.score,
                scores.scores
            );
            let chosen = scores.scores.iter().find(|(rotation, _)| *rotation == diagram.scene.rotation).unwrap();
            assert!((chosen.1 - diagram.scene.collision.score).abs() < 1e-9, "{name}: chosen score mismatch");
        }
    }
}

#[test]
fn bank_branch_auto_rotation_scores_are_not_all_zero() {
    // Regression for the "rotation_scores all 0.0" report: bank-branch spaced has residual overlaps at some angles.
    let diagram = dioramic_core::compile_source(
        &fixture("bank-branch").source,
        SourceFormat::Yaml,
        &auto_view_options(Density::Spaced),
    )
    .unwrap();
    let scores = diagram.scene.rotation_scores.unwrap();
    assert!(scores.scores.iter().any(|(_, score)| *score > 0.0), "{:?}", scores.scores);
}

#[test]
fn checkout_spaced_front_view_is_collision_free() {
    assert_no_collisions(&fixture("checkout").scene(View::new(ThemeName::Blueprint, Rotation::R0, Density::Spaced)));
}

#[test]
fn bank_branch_auto_rotation_is_collision_free() {
    let diagram = dioramic_core::compile_source(
        &fixture("bank-branch").source,
        SourceFormat::Yaml,
        &auto_view_options(Density::Spaced),
    )
    .unwrap();
    assert_no_collisions(&diagram.scene);
}

#[test]
fn spaced_auto_rotation_is_collision_free_for_every_example() {
    for name in fixture_names() {
        let diagram = dioramic_core::compile_source(
            &fixture(&name).source,
            SourceFormat::Yaml,
            &auto_view_options(Density::Spaced),
        )
        .unwrap();
        assert_no_collisions(&diagram.scene);
    }
}

#[test]
fn auto_views_have_no_label_read_as_another_items() {
    // T-394: the collision score counts labels that read as another item's (a pill nearer another edge or
    // stacked on a node label, a leader behind another node's label); none may be left in the `auto` views.
    let misread = ["reads as its label", "from its edge (at most", "nearer the label of node", "runs behind the label"];
    for name in fixture_names() {
        for density in Density::ALL {
            let diagram =
                dioramic_core::compile_source(&fixture(&name).source, SourceFormat::Yaml, &auto_view_options(density))
                    .unwrap();
            let found: Vec<&str> = diagram
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_str())
                .filter(|message| misread.iter().any(|marker| message.contains(marker)))
                .collect();
            assert!(found.is_empty(), "{name} {density:?}: {found:#?}");
        }
    }
}

#[test]
fn residual_collisions_are_reported_as_label_overlap_warnings() {
    for name in fixture_names() {
        for view in View::gallery_matrix() {
            let diagram = fixture(&name).diagram(view);
            let has_warning =
                diagram.diagnostics.iter().any(|diagnostic| diagnostic.code.as_str() == "W_LABEL_OVERLAP");
            if diagram.scene.collision.score > test_support::COLLISION_SCORE_EPSILON {
                assert!(
                    has_warning,
                    "{name} {}: score {} without W_LABEL_OVERLAP",
                    view.label(),
                    diagram.scene.collision.score
                );
            }
        }
    }
}

fn footprint_rect(center: dioramic_core::geometry::PlanPoint, footprint: &Footprint, margin: f64) -> PlanRect {
    let (half_width, half_depth) = footprint.half_extents();
    PlanRect::around(center, half_width + margin, half_depth + margin)
}

fn plan_overlap(a: &PlanRect, b: &PlanRect) -> bool {
    a.x0 < b.x1 - PLAN_EPSILON && b.x0 < a.x1 - PLAN_EPSILON && a.y0 < b.y1 - PLAN_EPSILON && b.y0 < a.y1 - PLAN_EPSILON
}

fn plan_contains(outer: &PlanRect, inner: &PlanRect) -> bool {
    inner.x0 >= outer.x0 - PLAN_EPSILON
        && inner.y0 >= outer.y0 - PLAN_EPSILON
        && inner.x1 <= outer.x1 + PLAN_EPSILON
        && inner.y1 <= outer.y1 + PLAN_EPSILON
}

fn assert_layout_is_clean(name: &str, model: &Model, plan: &PlanLayout) {
    let label = format!("{name} {:?} r{}", plan.density, plan.rotation.degrees());
    let inflated: Vec<PlanRect> = plan
        .nodes
        .iter()
        .map(|node| footprint_rect(node.center, &node.metrics.footprint, FOOTPRINT_MARGIN_TILES / 2.0))
        .collect();
    for (first, a) in inflated.iter().enumerate() {
        for (offset, b) in inflated[first + 1..].iter().enumerate() {
            let second = first + 1 + offset;
            assert!(
                !plan_overlap(a, b),
                "{label}: footprints of '{}' and '{}' overlap",
                plan.nodes[first].id,
                plan.nodes[second].id
            );
        }
    }
    for node in &plan.nodes {
        let Some(zone_id) = &model.nodes[node.index].zone else { continue };
        let zone = plan.zones.iter().find(|zone| &zone.id == zone_id).unwrap();
        let body = footprint_rect(node.center, &node.metrics.footprint, 0.0);
        assert!(plan_contains(&zone.rect, &body), "{label}: node '{}' outside zone '{zone_id}'", node.id);
    }
    for zone in &plan.zones {
        let spec_zone = &model.zones[zone.index];
        if let Some(parent_id) = &spec_zone.parent {
            let parent = plan.zones.iter().find(|candidate| &candidate.id == parent_id).unwrap();
            assert!(
                plan_contains(&parent.rect, &zone.rect),
                "{label}: zone '{}' outside parent '{parent_id}'",
                zone.id
            );
            assert_eq!(zone.depth, parent.depth + 1, "{label}: depth of '{}'", zone.id);
        }
    }
    let siblings: Vec<_> = plan.zones.iter().collect();
    for (first, a) in siblings.iter().enumerate() {
        for b in &siblings[first + 1..] {
            let same_parent = model.zones[a.index].parent == model.zones[b.index].parent;
            if same_parent {
                assert!(!plan_overlap(&a.rect, &b.rect), "{label}: sibling zones '{}' and '{}' overlap", a.id, b.id);
            }
        }
    }
}

#[test]
fn layout_has_no_overlaps_and_respects_zones_in_every_example() {
    for name in fixture_names() {
        let model = fixture(&name).model();
        for density in Density::ALL {
            let base = layout::layout(&model, density).unwrap();
            for rotation in Rotation::ALL {
                assert_layout_is_clean(&name, &model, &layout::rotate(&base, rotation));
            }
        }
    }
}

#[test]
fn layout_is_deterministic_for_every_example() {
    for name in fixture_names() {
        let model = fixture(&name).model();
        for density in Density::ALL {
            assert_deterministic(|| format!("{:?}", layout::layout(&model, density).unwrap()));
        }
    }
}

fn engine_request(name: &str, format: OutputFormat) -> RenderRequest {
    let mut request = RenderRequest::from_text(fixture(name).source);
    request.format = format;
    request.preview = PreviewMode::None;
    request
}

#[test]
fn engine_renders_png_for_every_example_deterministically() {
    for name in fixture_names() {
        let request = engine_request(&name, OutputFormat::Png);
        let first = Engine::shared().render(&request).unwrap_or_else(|error| panic!("{name}: {error}"));
        let decoder = png::Decoder::new(std::io::Cursor::new(&first.output)).read_info().unwrap();
        assert_eq!(
            (decoder.info().width, decoder.info().height),
            (first.summary.width, first.summary.height),
            "{name}"
        );
        assert_eq!(first.summary.bytes, first.output.len() as u64, "{name}");
        let second = Engine::shared().render(&request).unwrap();
        assert!(first.output == second.output, "{name}: PNG is not deterministic");
    }
}

#[test]
fn engine_svg_has_no_text_elements_by_default_for_every_example() {
    for name in fixture_names() {
        let result = Engine::shared().render(&engine_request(&name, OutputFormat::Svg)).unwrap();
        let svg = String::from_utf8(result.output).unwrap();
        assert!(svg.starts_with("<svg") || svg.starts_with("<?xml"), "{name}: not an SVG");
        assert!(!svg.contains("<text"), "{name}: outlines mode still emits <text>");
    }
}

#[test]
fn engine_summary_matches_core_auto_rotation() {
    for name in fixture_names() {
        for density in Density::ALL {
            let mut request = engine_request(&name, OutputFormat::Svg);
            request.rotation = Some(RotationSpec::Auto);
            request.density = Some(density);
            let summary = Engine::shared().render(&request).unwrap().summary;
            let diagram =
                dioramic_core::compile_source(&fixture(&name).source, SourceFormat::Yaml, &auto_view_options(density))
                    .unwrap();
            assert_eq!(summary.view.rotation, diagram.scene.rotation, "{name} {density:?}");
            let scores =
                summary.view.rotation_scores.unwrap_or_else(|| panic!("{name}: summary without rotation_scores"));
            assert_eq!(scores.len(), 4, "{name}: {scores:?}");
            for (rotation, score) in diagram.scene.rotation_scores.unwrap().scores {
                let reported = scores
                    .get(&rotation.degrees().to_string())
                    .copied()
                    .unwrap_or_else(|| panic!("{name}: no score for {rotation:?} in {scores:?}"));
                assert!(
                    (reported - score).abs() < 1e-6,
                    "{name} {density:?}: {rotation:?} core {score} vs engine {reported}"
                );
            }
        }
    }
}

#[test]
fn engine_gif_of_minimal_example_is_well_framed() {
    let mut request = engine_request("minimal", OutputFormat::Gif);
    request.width_px = Some(320);
    let result = Engine::shared().render(&request).unwrap();
    assert!(result.output.starts_with(b"GIF89a"), "missing GIF89a header");
    assert_eq!(result.output.last(), Some(&0x3B), "missing GIF trailer");
    assert!(result.summary.frames.is_some_and(|frames| frames > 1), "{:?}", result.summary.frames);
}

#[test]
fn golden_checkout_blueprint_r0_spaced_svg() {
    let svg = fixture("checkout").svg(View::new(ThemeName::Blueprint, Rotation::R0, Density::Spaced));
    golden::check("checkout-blueprint-r0-spaced.svg", svg.as_bytes());
}

#[test]
fn golden_bank_branch_midnight_r90_compact_svg() {
    let svg = fixture("bank-branch").svg(View::new(ThemeName::Midnight, Rotation::R90, Density::Compact));
    golden::check("bank-branch-midnight-r90-compact.svg", svg.as_bytes());
}

#[test]
fn golden_minimal_png() {
    let result = Engine::shared().render(&engine_request("minimal", OutputFormat::Png)).unwrap();
    golden::check("minimal.png", &result.output);
}
