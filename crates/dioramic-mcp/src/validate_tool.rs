//! `validate_diagram`: diagnostics plus a text summary of the layout (zones and nodes in plan tiles),
//! without rasterizing, so the model can reason about space without an image.

use std::fmt::Write as _;

use dioramic_core::Diagram;
use dioramic_core::geometry::PlanRect;
use std::path::Path;

use dioramic_core::icons::{self, IconRegistry};
use dioramic_render::{Engine, RenderRequest, SpecSource};
use dioramic_spec::{Density, Diagnostics, LayoutAlgorithm, Model, Rotation, ThemeName};
use rmcp::model::{CallToolResult, ContentBlock};
use serde::Serialize;

use crate::outcome::{ToolError, bullet_list, run_blocking};
use crate::params::ValidateDiagramParams;
use crate::source;
use crate::workspace::Workspace;

/// Tile coordinates are reported with one decimal (half tiles are the finest unit users write).
const TILE_PRECISION: f64 = 10.0;

/// Layout summary returned by `validate_diagram`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LayoutSummary {
    /// Camera rotation used (resolved when `auto`).
    pub rotation: Rotation,
    /// Density used.
    pub density: Density,
    /// Theme used.
    pub theme: ThemeName,
    /// Placement algorithm used (`layout.algorithm`, after the theme's default).
    pub layout_algorithm: LayoutAlgorithm,
    /// Canvas size in px (`[width, height]`, scale 1).
    pub canvas: [u32; 2],
    /// Zones, parents first.
    pub zones: Vec<ZoneSummary>,
    /// Nodes in spec order.
    pub nodes: Vec<NodeSummary>,
    /// Warnings and hints.
    pub diagnostics: Diagnostics,
}

/// A zone rectangle in plan tiles (rotation-0 world).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ZoneSummary {
    /// Id.
    pub id: String,
    /// Label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Parent zone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Top corner `[x, y]`.
    pub from: [f64; 2],
    /// Opposite corner `[x, y]`.
    pub to: [f64; 2],
}

/// A node center in plan tiles (rotation-0 world).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NodeSummary {
    /// Id.
    pub id: String,
    /// Resolved icon id.
    pub icon: String,
    /// Label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Zone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
    /// Center `[x, y]`.
    pub at: [f64; 2],
}

/// Runs the tool.
pub async fn validate_diagram(
    engine: Engine,
    workspace: &Workspace,
    params: ValidateDiagramParams,
    icons: &(dyn Fn(&Path) -> IconRegistry + Sync),
    structured: bool,
) -> Result<CallToolResult, ToolError> {
    let loaded = source::load(&params.input, workspace)?;
    let registry = icons(&source::spec_directory(&loaded, workspace));
    let mut request = RenderRequest::new(SpecSource::Text(loaded.text));
    request.theme = params.theme;
    request.rotation = params.rotation;
    request.density = params.density;
    request.routing = params.routing;
    request.layout_algorithm = params.layout_algorithm;
    request.icons = registry.clone();
    let diagram = run_blocking(move || Ok(engine.compile(&request)?)).await?;
    let summary = summarize(&diagram, &registry);
    let mut result = CallToolResult::success(vec![ContentBlock::text(describe(&summary))]);
    if structured {
        result.structured_content = serde_json::to_value(&summary).ok();
    }
    Ok(result)
}

/// Builds the summary from a compiled diagram.
pub fn summarize(diagram: &Diagram, icons: &IconRegistry) -> LayoutSummary {
    let scene = &diagram.scene;
    let zones = scene
        .zones
        .iter()
        .map(|zone| {
            let spec_zone = diagram.model.zone(&zone.id);
            ZoneSummary {
                id: zone.id.as_str().to_owned(),
                label: spec_zone.and_then(|spec_zone| spec_zone.label.clone()),
                parent: spec_zone
                    .and_then(|spec_zone| spec_zone.parent.as_ref())
                    .map(|parent| parent.as_str().to_owned()),
                from: corner(&zone.plan_rect, Corner::Min),
                to: corner(&zone.plan_rect, Corner::Max),
            }
        })
        .collect();
    LayoutSummary {
        rotation: scene.rotation,
        density: scene.density,
        theme: scene.theme.name,
        layout_algorithm: diagram.model.layout.algorithm,
        canvas: [pixels(scene.viewbox.width()), pixels(scene.viewbox.height())],
        zones,
        nodes: node_summaries(diagram, icons),
        diagnostics: diagram.diagnostics.clone(),
    }
}

/// Nodes in spec order (the scene keeps paint order).
fn node_summaries(diagram: &Diagram, registry: &IconRegistry) -> Vec<NodeSummary> {
    let model: &Model = &diagram.model;
    model
        .nodes
        .iter()
        .filter_map(|node| {
            let placed = diagram.scene.nodes.iter().find(|placed| placed.id == node.id)?;
            Some(NodeSummary {
                id: node.id.as_str().to_owned(),
                icon: icons::resolve_with(node, registry)
                    .map_or_else(|_| icons::block().id, |resolution| resolution.entry.id)
                    .to_owned(),
                label: node.label.clone(),
                zone: node.zone.as_ref().map(|zone| zone.as_str().to_owned()),
                at: [tiles(placed.plan_center.x), tiles(placed.plan_center.y)],
            })
        })
        .collect()
}

/// Human/model-readable text version of the summary.
pub fn describe(summary: &LayoutSummary) -> String {
    let warnings = summary.diagnostics.len();
    let mut text = format!("OK: the spec is valid ({warnings} warning(s)/hint(s)).\n");
    let _ = writeln!(
        text,
        "View: rotation {}, density {}, theme {}, layout {}; canvas {}x{} px at scale 1.",
        summary.rotation,
        summary.density.as_str(),
        summary.theme.as_str(),
        summary.layout_algorithm.as_str(),
        summary.canvas[0],
        summary.canvas[1]
    );
    text.push_str("Plan axes (rotation 0): x grows down-right, y grows down-left; units are tiles.\n");
    if !summary.zones.is_empty() {
        text.push_str("Zones (from -> to):\n");
        for zone in &summary.zones {
            let _ = writeln!(
                text,
                "- {}{}{}: {} -> {}",
                zone.id,
                quoted(zone.label.as_deref()),
                zone.parent.as_ref().map(|parent| format!(" in {parent}")).unwrap_or_default(),
                point(zone.from),
                point(zone.to)
            );
        }
    }
    text.push_str("Nodes (center):\n");
    for node in &summary.nodes {
        let _ = writeln!(
            text,
            "- {} [{}]{}{} at {}",
            node.id,
            node.icon,
            quoted(node.label.as_deref()),
            node.zone.as_ref().map(|zone| format!(" in {zone}")).unwrap_or_default(),
            point(node.at)
        );
    }
    if !summary.diagnostics.is_empty() {
        let _ = write!(text, "Diagnostics:\n{}\n", bullet_list(&summary.diagnostics));
    }
    text
}

enum Corner {
    Min,
    Max,
}

fn corner(rect: &PlanRect, which: Corner) -> [f64; 2] {
    match which {
        Corner::Min => [tiles(rect.x0), tiles(rect.y0)],
        Corner::Max => [tiles(rect.x1), tiles(rect.y1)],
    }
}

fn tiles(value: f64) -> f64 {
    (value * TILE_PRECISION).round() / TILE_PRECISION
}

fn pixels(length: f64) -> u32 {
    length.round().clamp(0.0, f64::from(u32::MAX)) as u32
}

fn point([x, y]: [f64; 2]) -> String {
    format!("({x:.1}, {y:.1})")
}

fn quoted(label: Option<&str>) -> String {
    label.map(|label| format!(" \"{label}\"")).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use dioramic_spec::{Diagnostic, DiagnosticCode};

    use super::*;

    fn sample() -> LayoutSummary {
        LayoutSummary {
            rotation: Rotation::R90,
            density: Density::Compact,
            theme: ThemeName::Paper,
            layout_algorithm: LayoutAlgorithm::Hub,
            canvas: [1200, 800],
            zones: vec![ZoneSummary {
                id: "dmz".to_owned(),
                label: Some("DMZ".to_owned()),
                parent: None,
                from: [0.0, 0.0],
                to: [4.5, 3.0],
            }],
            nodes: vec![NodeSummary {
                id: "lb".to_owned(),
                icon: "loadbalancer".to_owned(),
                label: None,
                zone: Some("dmz".to_owned()),
                at: [1.5, 1.0],
            }],
            diagnostics: Diagnostic::new(DiagnosticCode::Isolated, "nodes[0]", "node 'lb' has no edges").into(),
        }
    }

    #[test]
    fn description_lists_view_zones_nodes_and_diagnostics() {
        let text = describe(&sample());
        assert!(text.contains("rotation 90, density compact, theme paper, layout hub"), "{text}");
        assert!(text.contains("- dmz \"DMZ\": (0.0, 0.0) -> (4.5, 3.0)"), "{text}");
        assert!(text.contains("- lb [loadbalancer] in dmz at (1.5, 1.0)"), "{text}");
        assert!(text.contains("W_ISOLATED nodes[0]"), "{text}");
    }

    #[test]
    fn tiles_are_rounded_to_one_decimal() {
        assert_eq!(tiles(1.04999), 1.0);
        assert_eq!(tiles(2.25), 2.3);
    }
}
