//! Plan layout: where zones and nodes go on the floor, in tiles (`docs/04-spec.md#layout`,
//! `docs/06-renderizacao.md#densidade`). Contract: `docs/work/contracts/C-004-layout.md`. Implementation: T-103.
//!
//! The layout is computed once in the rotation-0 world. Camera rotation is a separate pure step
//! ([`rotate`]) applied before projection, so the spec's `at: [x, y]` always means the same place.

use dioramic_spec::{BorderStyle, Density, Diagnostics, LayoutAlgorithm, Model, NodeId, NodeSize, Rotation, ZoneId};

mod arrange;
mod block;
mod hub;
mod ranks;

pub use hub::source_reading_rank;

use arrange::{Arranger, FlowFrame, Spacing};

use crate::geometry::{PlanPoint, PlanRect, PlanVector, rotate_about};
use crate::icons::{self, IconEntry, IconMetrics, IconRegistry};

/// Spacing parameters of a density (prototype `DENSITY`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DensityParams {
    /// Distance between node centers inside a zone (tiles).
    pub node_gap: f64,
    /// Inner zone padding (tiles).
    pub zone_padding: f64,
    /// Gap between zones (tiles).
    pub zone_gap: f64,
    /// Default leader line length of node labels (px).
    pub label_lift_px: f64,
}

/// Parameters of `spaced` (2.4 / 1.45 / 1.0 / 52) and `compact` (1.7 / 0.95 / 0.6 / 38).
pub fn density_params(density: Density) -> DensityParams {
    match density {
        Density::Spaced => DensityParams { node_gap: 2.4, zone_padding: 1.45, zone_gap: 1.0, label_lift_px: 52.0 },
        Density::Compact => DensityParams { node_gap: 1.7, zone_padding: 0.95, zone_gap: 0.6, label_lift_px: 38.0 },
    }
}

/// A zone placed on the plan.
#[derive(Debug, Clone, PartialEq)]
pub struct ZoneBox {
    /// Id.
    pub id: ZoneId,
    /// Index in `model.zones` (palette rotation, spec path).
    pub index: usize,
    /// Rectangle in tiles.
    pub rect: PlanRect,
    /// Nesting depth (0 = top level). Parents are painted before children.
    pub depth: u32,
    /// Border.
    pub border: BorderStyle,
}

/// A node placed on the plan.
#[derive(Debug, Clone, PartialEq)]
pub struct NodePlace {
    /// Id.
    pub id: NodeId,
    /// Index in `model.nodes`.
    pub index: usize,
    /// Center in tiles.
    pub center: PlanPoint,
    /// Elevation (tiles).
    pub z: f64,
    /// Resolved icon.
    pub icon: &'static IconEntry,
    /// Icon scale.
    pub size: NodeSize,
    /// Footprint and height for the current rotation.
    pub metrics: IconMetrics,
    /// Flow rank (0 = sources). Drives zone order and the `build` animation.
    pub rank: u32,
}

/// The whole plan layout.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanLayout {
    /// Density used.
    pub density: Density,
    /// Spacing parameters used.
    pub params: DensityParams,
    /// Camera rotation this layout is expressed in (`R0` straight out of [`layout`]).
    pub rotation: Rotation,
    /// Zones, parents before children.
    pub zones: Vec<ZoneBox>,
    /// Nodes in `model.nodes` order.
    pub nodes: Vec<NodePlace>,
    /// Bounds of zones and node footprints.
    pub bounds: PlanRect,
}

impl PlanLayout {
    /// Node by id.
    pub fn node(&self, id: &NodeId) -> Option<&NodePlace> {
        self.nodes.iter().find(|node| &node.id == id)
    }

    /// Pivot used by [`rotate`]: center of all zones (or of all nodes when there are no zones).
    pub fn pivot(&self) -> PlanPoint {
        self.bounds.center()
    }
}

/// Computes the plan layout of a structurally valid model (icons are resolved here; an unknown icon is an
/// `E_UNKNOWN_ICON` error).
///
/// Rules (T-103): flow ranks from the edge graph (sources first, cycles broken deterministically);
/// zones ordered by the rank of their nodes along `layout.flow`; `spaced` keeps one row while the picture
/// stays legible and otherwise folds it serpentine into the most legible block, `compact` folds ⌈√n⌉ per row
/// serpentine (`layout.wrap` overrides both); nodes inside a zone follow `arrange`; nested zones are laid out
/// inside their parent; a node without zone sits beside the zone it has most edges with (or gets its own
/// lane when it has none); `at` / `size` pins are honored and never overlapped; no two footprints overlap.
///
/// Without pins the plan is normalized so its bounds start at `(0, 0)`; with any `at` pin coordinates
/// are absolute, so the pinned items land exactly where the spec says.
pub fn layout(model: &Model, density: Density) -> Result<PlanLayout, Diagnostics> {
    layout_with(model, density, IconRegistry::empty())
}

/// [`layout`] with flat icons (provider packs, project icons) available to the nodes.
pub fn layout_with(model: &Model, density: Density, registry: &IconRegistry) -> Result<PlanLayout, Diagnostics> {
    let params = density_params(density);
    let icons = resolve_icons(model, registry)?;
    let metrics: Vec<IconMetrics> =
        model.nodes.iter().zip(&icons).map(|(node, icon)| icons::metrics(icon, node.size, Rotation::R0)).collect();
    let flow = ranks::flow_ranks(model);
    let frame = FlowFrame::new(model.layout.flow);
    let footprint_half: Vec<f64> = metrics.iter().map(largest_half).collect();
    let spacing = spacing(&params, density, &metrics);
    let hub_arrangement = match model.layout.algorithm {
        LayoutAlgorithm::Hub => hub::arrange(model, &flow, &spacing, &footprint_half, frame, density),
        LayoutAlgorithm::Flow => None,
    };
    let on_lattice = hub_arrangement.is_some();
    let arrangement =
        hub_arrangement.unwrap_or_else(|| Arranger::new(model, &flow, spacing, footprint_half, frame).arrange(density));

    let mut zones: Vec<ZoneBox> = model
        .zones
        .iter()
        .enumerate()
        .map(|(index, zone)| ZoneBox {
            id: zone.id.clone(),
            index,
            rect: frame.rect(arrangement.zone_rects[index]),
            depth: zone_depth(&arrangement.zone_parent, index),
            border: zone.border,
        })
        .collect();
    zones.sort_by_key(|zone| (zone.depth, zone.index));
    let nodes: Vec<NodePlace> = model
        .nodes
        .iter()
        .enumerate()
        .zip(icons.into_iter().zip(metrics))
        .map(|((index, node), (icon, metrics))| NodePlace {
            id: node.id.clone(),
            index,
            center: frame.point(arrangement.node_centers[index]),
            z: node.z,
            icon,
            size: node.size,
            metrics,
            rank: flow.rank[index],
        })
        .collect();
    let mut plan =
        PlanLayout { density, params, rotation: Rotation::R0, bounds: plan_bounds(&zones, &nodes), zones, nodes };
    if !arrangement.anchored {
        if on_lattice {
            normalize_origin_on_lattice(&mut plan);
        } else {
            normalize_origin(&mut plan);
        }
    }
    Ok(plan)
}

/// Rotates a layout about its [`PlanLayout::pivot`] by `rotation` (relative to its current rotation),
/// renormalizing rectangles and swapping non-square footprints on odd quarter turns.
pub fn rotate(layout: &PlanLayout, rotation: Rotation) -> PlanLayout {
    let pivot = layout.pivot();
    let total =
        Rotation::from_quarter_turns(i32::from(layout.rotation.quarter_turns()) + i32::from(rotation.quarter_turns()));
    let zones =
        layout.zones.iter().map(|zone| ZoneBox { rect: zone.rect.rotated(pivot, rotation), ..zone.clone() }).collect();
    let nodes = layout
        .nodes
        .iter()
        .map(|node| NodePlace {
            center: rotate_about(node.center, pivot, rotation),
            metrics: icons::metrics(node.icon, node.size, total),
            ..node.clone()
        })
        .collect();
    PlanLayout { rotation: total, zones, nodes, bounds: layout.bounds.rotated(pivot, rotation), ..layout.clone() }
}

/// Clearance kept around every footprint (tiles): two neighbours never come closer than twice this.
const FOOTPRINT_CLEARANCE: f64 = 0.2;
/// Extra room on top of the clearance so neighbours never merely touch.
const PITCH_SLACK: f64 = 0.1;
/// Minimum distance between a footprint and the border of its zone (tiles).
const ZONE_INSET: f64 = 0.3;
/// `compact`: free floor between a footprint and its zone border. The zone label chip lies on the front
/// border (0.14–0.5 tile in, `scene::labels`), so the footprint starts past it with a little air.
const COMPACT_BORDER_ROOM: f64 = 0.7;
/// `compact`: free floor between two footprints along the flow, so the edge joining neighbours keeps a
/// visible run between them (and room for its arrowhead).
const COMPACT_EDGE_RUN: f64 = 1.0;

/// `compact`: extra distance between siblings stacked across the flow (tiles). Neighbours across the flow sit
/// side by side on screen; at the bare 1.7-tile pitch their labels (≈ 150 px wide) overlap and cannot
/// stagger within the leader cap, so crowded zones (six devices in a teller area) piled labels on icons.
const COMPACT_LABEL_ROOM_ACROSS: f64 = 0.5;
/// `spaced`: extra room along the flow for a node joined to a zone-mate by a labelled edge when one of the
/// two is a hub (tiles). At the plain pitch the short edge from a busy hub to its neighbour (a core switch
/// and the server beside it) has no free stretch for its pill: it lands on the hub's label or hides the stroke
/// (T-370). Kept small: it only buys the pill a few px, and spreading every zone shrinks the text of the page.
const SPACED_PILL_RUN: f64 = 0.3;

fn resolve_icons(model: &Model, registry: &IconRegistry) -> Result<Vec<&'static IconEntry>, Diagnostics> {
    let mut errors = Diagnostics::new();
    let mut resolved = Vec::with_capacity(model.nodes.len());
    for node in &model.nodes {
        match icons::resolve_with(node, registry) {
            Ok(resolution) => resolved.push(resolution.entry),
            Err(diagnostic) => errors.push(diagnostic),
        }
    }
    if errors.has_errors() { Err(errors) } else { Ok(resolved) }
}

/// Largest half extent of a footprint (the half side of its bounding square).
fn largest_half(metrics: &IconMetrics) -> f64 {
    let (half_width, half_depth) = metrics.footprint.half_extents();
    half_width.max(half_depth)
}

/// Density spacing, widened when a footprint is too large for it (big icons, `size: l`).
///
/// `spaced` only lengthens the cell of a node with a labelled edge inside its zone ([`SPACED_PILL_RUN`]).
/// `compact` keeps a free run between neighbours along the flow, label room between siblings across it, and
/// grows each zone's padding with its own largest icon, so icons stay inside their zone and its label fits.
fn spacing(params: &DensityParams, density: Density, metrics: &[IconMetrics]) -> Spacing {
    let largest_half = metrics.iter().map(largest_half).fold(0.0, f64::max);
    let pitch = params.node_gap.max(2.0 * (largest_half + FOOTPRINT_CLEARANCE) + PITCH_SLACK);
    let padding = params.zone_padding.max(largest_half + ZONE_INSET);
    let zone_margin = params.zone_gap / 2.0;
    match density {
        Density::Spaced => Spacing {
            pitch_along: pitch,
            pitch_across: pitch,
            padding,
            border_room: ZONE_INSET,
            zone_margin,
            pill_run: SPACED_PILL_RUN,
        },
        Density::Compact => Spacing {
            pitch_along: pitch.max(2.0 * largest_half + COMPACT_EDGE_RUN),
            pitch_across: pitch + COMPACT_LABEL_ROOM_ACROSS,
            padding: params.zone_padding,
            border_room: COMPACT_BORDER_ROOM,
            zone_margin,
            pill_run: 0.0,
        },
    }
}

fn zone_depth(parents: &[Option<usize>], zone: usize) -> u32 {
    let mut depth = 0;
    let mut current = parents[zone];
    while let Some(parent) = current {
        depth += 1;
        current = parents[parent];
    }
    depth
}

/// Footprint rectangle of a placed node (bounding box for circles).
fn footprint_rect(node: &NodePlace) -> PlanRect {
    let (half_width, half_depth) = node.metrics.footprint.half_extents();
    PlanRect::around(node.center, half_width, half_depth)
}

fn plan_bounds(zones: &[ZoneBox], nodes: &[NodePlace]) -> PlanRect {
    zones
        .iter()
        .map(|zone| zone.rect)
        .chain(nodes.iter().map(footprint_rect))
        .reduce(PlanRect::union)
        .unwrap_or(PlanRect::new(0.0, 0.0, 0.0, 0.0))
}

/// Shifts an unpinned plan so its bounds start at the origin.
fn normalize_origin(plan: &mut PlanLayout) {
    shift_plan(plan, PlanVector::new(-plan.bounds.x0, -plan.bounds.y0));
}

/// Shifts a `hub` plan so its bounds start within one lattice step of the origin, keeping every node center on
/// the half-tile lattice.
fn normalize_origin_on_lattice(plan: &mut PlanLayout) {
    let floor = |value: f64| (value / hub::LATTICE_STEP).floor() * hub::LATTICE_STEP;
    shift_plan(plan, PlanVector::new(-floor(plan.bounds.x0), -floor(plan.bounds.y0)));
}

fn shift_plan(plan: &mut PlanLayout, shift: PlanVector) {
    for zone in &mut plan.zones {
        zone.rect = PlanRect::new(
            zone.rect.x0 + shift.x,
            zone.rect.y0 + shift.y,
            zone.rect.x1 + shift.x,
            zone.rect.y1 + shift.y,
        );
    }
    for node in &mut plan.nodes {
        node.center = node.center + shift;
    }
    plan.bounds = plan_bounds(&plan.zones, &plan.nodes);
}

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;
