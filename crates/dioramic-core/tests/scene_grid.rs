//! Background grid (T-404): `view.grid` reaches the scene and the SVG, defaults keep the v1 grid, and the
//! lattice the grid is drawn on is the one zones sit on, in every rotation.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use dioramic_core::geometry::Projection;
use dioramic_core::scene::SceneGrid;
use dioramic_core::svg::SvgOptions;
use dioramic_core::{CompileOptions, compile_source, render_svg};
use dioramic_spec::{GridOptions, GridStyle, Rotation, RotationSpec, SourceFormat};

const SPEC: &str = "diorama: 1\nzones: [{ id: a }, { id: b }]\nnodes:\n  - { id: x, zone: a }\n  - { id: y, zone: b }\nedges:\n  - x -> y\n";

fn svg_with(view: &str) -> String {
    let source = format!("{SPEC}{view}");
    let diagram = compile_source(&source, SourceFormat::Yaml, &CompileOptions::default()).unwrap();
    render_svg(&diagram, &SvgOptions::default())
}

/// The `<g data-role="grid" …>` element of an SVG.
fn grid_group(svg: &str) -> &str {
    let start = svg.find("data-role=\"grid\"").expect("grid group");
    let end = svg[start..].find("</g>").expect("end of grid group");
    &svg[start..start + end]
}

#[test]
fn defaults_keep_the_v1_grid() {
    let diagram = compile_source(SPEC, SourceFormat::Yaml, &CompileOptions::default()).unwrap();
    let theme = diagram.scene.theme.clone();
    assert_eq!(diagram.scene.grid, SceneGrid::of(&GridOptions::default(), &theme));
    let svg = render_svg(&diagram, &SvgOptions::default());
    let group = grid_group(&svg);
    assert!(group.contains("<line") && !group.contains("opacity"), "{group}");
}

#[test]
fn size_style_opacity_and_color_reach_the_svg() {
    let lines = |view: &str| grid_group(&svg_with(view)).matches("<line").count();
    let (fine, default, coarse) =
        (lines("view: { grid: { size: 0.5 } }\n"), lines(""), lines("view: { grid: { size: 2 } }\n"));
    assert!(fine > default && default > coarse, "{fine} > {default} > {coarse}");
    let dots = svg_with("view: { grid: { style: dots, opacity: 0.4, color: teal } }\n");
    let group = grid_group(&dots);
    assert!(group.contains("<circle") && !group.contains("<line"), "{group}");
    assert!(group.contains("opacity=\"0.4\""), "{group}");
    let none = svg_with("view: { grid: { style: none } }\n");
    assert!(!none.contains("data-role=\"grid\""));
}

#[test]
fn the_grid_follows_the_diagram_in_every_rotation() {
    // The first zone starts at the world origin; whatever the camera does, a lattice point stays on that
    // zone corner, so zones and nodes keep their place on the grid.
    for rotation in Rotation::ALL {
        let options = CompileOptions { rotation: Some(RotationSpec::Fixed(rotation)), ..CompileOptions::default() };
        let source = format!("{SPEC}view: {{ grid: {{ size: 0.5 }} }}\n");
        let diagram = compile_source(&source, SourceFormat::Yaml, &options).unwrap();
        let scene = &diagram.scene;
        assert_eq!(scene.grid.style, GridStyle::Lines);
        let projection = Projection::new(scene.tile_px);
        let first = scene.zones.iter().find(|zone| zone.id.as_str() == "a").expect("zone a");
        let on_origin =
            first.polygon.iter().any(|corner| projection.unproject(*corner, 0.0).distance(scene.grid.origin) < 1e-6);
        assert!(on_origin, "{rotation:?}: no corner of zone a on the grid origin {:?}", scene.grid.origin);
    }
}
