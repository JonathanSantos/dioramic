//! QA (T-301): arrow ends (`<-`, `<->`, `--`, `arrow:` overrides) and routes (`curved`, `straight`, `orthogonal`)
//! are not covered by `examples/`, so they get synthetic specs here. Every variant must keep the arrow
//! invariants of doc 06 (aim < 1°, tangent arrival, no hidden tip) in every rotation and density.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use dioramic_core::{CompileOptions, Diagram};
use dioramic_spec::{Density, Rotation, RotationSpec, SourceFormat, ThemeName};
use test_support::arrows::HeadEnd;
use test_support::{assert_arrows_aim_at_targets, assert_deterministic, measure_arrows};

const ROUTES: [&str; 3] = ["curved", "straight", "orthogonal"];

fn spec(routing: &str) -> String {
    format!(
        "diorama: 1
title: Edge variants
layout: {{ routing: {routing} }}
zones:
  - {{ id: edge, label: Edge }}
  - {{ id: core, label: Core }}
nodes:
  - {{ id: users, icon: users, label: Users }}
  - {{ id: lb, icon: loadbalancer, zone: edge, label: LB }}
  - {{ id: api, icon: server, zone: core, label: API }}
  - {{ id: worker, icon: server, zone: core, label: Worker }}
  - {{ id: db, icon: database, zone: core, label: DB }}
  - {{ id: cache, icon: cache, zone: core, label: Cache }}
edges:
  - users -> lb: HTTPS
  - api <- lb: proxied
  - api <-> worker: jobs
  - api -- cache
  - worker ..> db: async
  - {{ from: api, to: db, label: SQL, arrow: both }}
  - {{ from: worker, to: cache, arrow: none }}
"
    )
}

fn compile(routing: &str, rotation: Rotation, density: Density) -> Diagram {
    let options = CompileOptions {
        theme: Some(ThemeName::Blueprint),
        rotation: Some(RotationSpec::Fixed(rotation)),
        density: Some(density),
        ..CompileOptions::default()
    };
    dioramic_core::compile_source(&spec(routing), SourceFormat::Yaml, &options)
        .unwrap_or_else(|diagnostics| panic!("routing {routing}: {diagnostics}"))
}

#[test]
fn every_route_keeps_arrow_invariants_in_every_view() {
    for routing in ROUTES {
        for density in Density::ALL {
            for rotation in Rotation::ALL {
                assert_arrows_aim_at_targets(&compile(routing, rotation, density).scene);
            }
        }
    }
}

#[test]
fn arrow_ends_produce_the_expected_heads() {
    let scene = compile("curved", Rotation::R0, Density::Spaced).scene;
    let heads_of = |id: &str| -> Vec<(HeadEnd, String)> {
        measure_arrows(&scene)
            .into_iter()
            .filter(|measurement| measurement.edge == id)
            .map(|measurement| (measurement.end, measurement.target))
            .collect()
    };
    // `api <- lb`: from/to kept, the only head sits at the start and points at `api`.
    assert_eq!(heads_of("api->lb"), vec![(HeadEnd::Source, "api".to_owned())]);
    let both = heads_of("api->worker");
    assert_eq!(both.len(), 2, "{both:?}");
    assert!(
        both.contains(&(HeadEnd::Target, "worker".to_owned())) && both.contains(&(HeadEnd::Source, "api".to_owned()))
    );
    assert!(heads_of("api->cache").is_empty(), "`--` has no head");
    assert!(heads_of("worker->cache").is_empty(), "`arrow: none` has no head");
    assert_eq!(heads_of("api->db").len(), 2, "`arrow: both` override");
    assert_eq!(heads_of("users->lb"), vec![(HeadEnd::Target, "lb".to_owned())]);
}

#[test]
fn routes_actually_change_the_path_shape() {
    use dioramic_core::scene::EdgePath;
    let curved = compile("curved", Rotation::R0, Density::Spaced).scene;
    let orthogonal = compile("orthogonal", Rotation::R0, Density::Spaced).scene;
    assert!(curved.edges.iter().all(|edge| matches!(edge.path, EdgePath::Curve(_))), "curved routing uses curves");
    assert!(
        orthogonal.edges.iter().any(|edge| matches!(edge.path, EdgePath::Polyline(_))),
        "orthogonal routing should produce at least one polyline"
    );
}

#[test]
fn edge_variants_are_deterministic() {
    for routing in ROUTES {
        assert_deterministic(|| {
            let diagram = compile(routing, Rotation::R90, Density::Compact);
            dioramic_core::render_svg(&diagram, &dioramic_core::svg::SvgOptions::default())
        });
    }
}
