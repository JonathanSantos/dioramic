//! The `schematic` theme and semantic edge colors end to end (T-408/T-409): classes and their colors,
//! midway heads, dotted async links, outlined zones, the legend, and the classic themes left untouched.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use dioramic_core::scene::Scene;
use dioramic_core::svg::SvgOptions;
use dioramic_core::theme::Theme;
use dioramic_core::{CompileOptions, Diagram, compile_source, render_svg};
use dioramic_spec::{EdgeKind, Routing, SourceFormat, ThemeName};

const PLATFORM: &str = "diorama: 1
zones: [{ id: app, label: APP }]
nodes:
  - { id: web, icon: laptop }
  - { id: lb, icon: loadbalancer }
  - { id: api, icon: server, zone: app }
  - { id: queue, icon: queue, zone: app }
  - { id: db, icon: postgres }
  - { id: cache, icon: redis }
edges:
  - web -> lb: HTTPS
  - lb -> api
  - api -> db: SQL
  - api -> cache
  - api ..> queue
  - lb -> db @muted
";

fn compile(source: &str, theme: ThemeName) -> Diagram {
    let options = CompileOptions { theme: Some(theme), ..CompileOptions::default() };
    compile_source(source, SourceFormat::Yaml, &options).unwrap_or_else(|errors| panic!("{errors:?}"))
}

fn edge<'s>(scene: &'s Scene, id: &str) -> &'s dioramic_core::scene::SceneEdge {
    scene.edges.iter().find(|edge| edge.id.as_str() == id).unwrap()
}

#[test]
fn schematic_colors_unstyled_edges_by_class() {
    let diagram = compile(PLATFORM, ThemeName::Schematic);
    let scene = &diagram.scene;
    let kinds = Theme::schematic().edges.kinds;
    let expect = [
        ("web->lb", EdgeKind::Client),
        ("lb->api", EdgeKind::Service),
        ("api->db", EdgeKind::Data),
        ("api->cache", EdgeKind::Cache),
        ("api->queue", EdgeKind::Messaging),
    ];
    for (id, kind) in expect {
        let edge = edge(scene, id);
        assert_eq!(edge.kind, kind, "{id}");
        assert_eq!(edge.color, kinds.get(kind).color, "{id}");
    }
    assert!(edge(scene, "api->queue").dashed, "messaging is dashed (dotted in schematic)");
    let muted = edge(scene, "lb->db");
    assert_eq!(muted.kind, EdgeKind::Data);
    assert_eq!(muted.color, Theme::schematic().edges.muted, "an explicit style wins");
    assert_eq!(diagram.model.layout.routing, Routing::Rails, "the theme routes on rails");
}

#[test]
fn schematic_paints_heads_midway_and_outlines_zones() {
    let diagram = compile(PLATFORM, ThemeName::Schematic);
    let scene = &diagram.scene;
    assert!(scene.edges.iter().any(|edge| !edge.midway.is_empty()), "long edges carry a midway head");
    for edge in scene.edges.iter().filter(|edge| !edge.midway.is_empty()) {
        assert_eq!(edge.midway.len(), edge.heads.len(), "{}", edge.id);
        for head in &edge.midway {
            let center = head.tip.midpoint(head.base[0].midpoint(head.base[1]));
            let path_start = edge.path.point_at_fraction(0.0);
            let path_end = edge.path.point_at_fraction(1.0);
            assert!(
                center.distance(path_start) > 1.0 && center.distance(path_end) > 1.0,
                "{} head is not at an end",
                edge.id
            );
        }
    }
    let svg = render_svg(&diagram, &SvgOptions::default());
    assert!(svg.contains("fill=\"#101318\""), "black heads");
    assert!(svg.contains("stroke-dasharray=\"0 "), "dotted async link and leaders");
    let zone = &svg[svg.find("data-role=\"zone\"").unwrap()..];
    let polygon = &zone[..zone.find("/>").unwrap()];
    assert!(polygon.contains("fill=\"none\""), "outlined zone: {polygon}");
}

#[test]
fn schematic_legend_lists_the_classes_in_use() {
    let diagram = compile(PLATFORM, ThemeName::Schematic);
    let legend = diagram.scene.legend.as_ref().expect("legend");
    let names: Vec<&str> = legend.entries.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, ["Client", "Service", "Messaging", "Cache", "Data"]);
    assert!(diagram.scene.viewbox.contains(legend.rect.center()), "inside the frame");
    for node in &diagram.scene.nodes {
        assert!(node.shape.screen_bbox.overlap_area(&legend.rect, 0.0) <= 0.0, "legend over {}", node.id);
    }
    let svg = render_svg(&diagram, &SvgOptions::default());
    assert!(svg.contains("data-role=\"legend\""));
    let off = compile(&PLATFORM.replace("diorama: 1\n", "diorama: 1\nview: { legend: off }\n"), ThemeName::Schematic);
    assert!(off.scene.legend.is_none());
}

#[test]
fn classic_themes_keep_style_colors_end_heads_and_no_legend() {
    for theme in [ThemeName::Blueprint, ThemeName::Midnight, ThemeName::Paper, ThemeName::Mono] {
        let diagram = compile(PLATFORM, theme);
        let scene = &diagram.scene;
        assert!(scene.legend.is_none(), "{theme:?}");
        assert!(scene.edges.iter().all(|edge| edge.midway.is_empty()), "{theme:?}");
        let primary = Theme::builtin(theme).edges.primary;
        assert_eq!(edge(scene, "api->db").color, primary, "{theme:?}: style colors");
        assert_eq!(diagram.model.layout.routing, Routing::Curved, "{theme:?}");
    }
}

#[test]
fn semantic_colors_are_opt_in_for_classic_themes() {
    let semantic = PLATFORM.replace("diorama: 1\n", "diorama: 1\nlayout: { edge_colors: semantic }\n");
    let diagram = compile(&semantic, ThemeName::Blueprint);
    let kinds = Theme::blueprint().edges.kinds;
    assert_eq!(edge(&diagram.scene, "api->db").color, kinds.data.color);
    assert!(diagram.scene.legend.is_some());
    let recolored = semantic.replace("diorama: 1\n", "diorama: 1\nstyles: { data: { color: \"#123456\" } }\n");
    let diagram = compile(&recolored, ThemeName::Blueprint);
    assert_eq!(edge(&diagram.scene, "api->db").color.to_hex(), "#123456", "styles.<class> recolors the class");
    let mono = PLATFORM.replace("diorama: 1\n", "diorama: 1\nlayout: { edge_colors: mono }\n");
    let diagram = compile(&mono, ThemeName::Schematic);
    let primary = Theme::schematic().edges.primary;
    assert!(diagram.scene.edges.iter().all(|edge| edge.color == primary), "one color for all");
}
