//! How tight and how hub-centered a layout is (T-411), measured on the floor plan of a compiled scene, in tiles:
//! the length of the edges between node centers, how far busy nodes sit from the middle, and the floor area.
//!
//! Rotation does not change any of them (the camera turns the plan in quarter turns), so the numbers compare
//! `layout.algorithm: flow` and `hub` view by view.

use dioramic_core::geometry::{PlanPoint, PlanRect};
use dioramic_core::scene::Scene;

/// Layout numbers of one scene.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LayoutMetrics {
    /// Sum of the straight distances between the centers of the two ends of every edge (tiles).
    pub edge_length_total: f64,
    /// [`LayoutMetrics::edge_length_total`] over the number of edges (0 without edges).
    pub edge_length_mean: f64,
    /// Mean distance from each node to the middle of the plan, weighted by the node's edge count (tiles): small
    /// when the busy places are central.
    pub hub_centrality_offset: f64,
    /// Area of the box around zones and node centers (tiles²).
    pub area_tiles: f64,
}

impl LayoutMetrics {
    /// Measures `scene`.
    pub fn of(scene: &Scene) -> Self {
        let center_of =
            |id: &dioramic_spec::NodeId| scene.nodes.iter().find(|node| &node.id == id).map(|node| node.plan_center);
        let lengths: Vec<f64> = scene
            .edges
            .iter()
            .filter(|edge| edge.from != edge.to)
            .filter_map(|edge| Some(center_of(&edge.from)?.distance(center_of(&edge.to)?)))
            .collect();
        let edge_length_total: f64 = lengths.iter().sum();
        let bounds = plan_bounds(scene);
        Self {
            edge_length_total,
            edge_length_mean: if lengths.is_empty() { 0.0 } else { edge_length_total / lengths.len() as f64 },
            hub_centrality_offset: bounds.map_or(0.0, |bounds| hub_offset(scene, bounds.center())),
            area_tiles: bounds.map_or(0.0, |bounds| bounds.width() * bounds.depth()),
        }
    }
}

fn plan_bounds(scene: &Scene) -> Option<PlanRect> {
    let point = |at: PlanPoint| PlanRect::new(at.x, at.y, at.x, at.y);
    scene
        .zones
        .iter()
        .map(|zone| zone.plan_rect)
        .chain(scene.nodes.iter().map(|node| point(node.plan_center)))
        .reduce(PlanRect::union)
}

fn hub_offset(scene: &Scene, middle: PlanPoint) -> f64 {
    let (mut sum, mut weight) = (0.0, 0.0);
    for node in &scene.nodes {
        let degree = scene
            .edges
            .iter()
            .filter(|edge| edge.from != edge.to && (edge.from == node.id || edge.to == node.id))
            .count() as f64;
        sum += degree * node.plan_center.distance(middle);
        weight += degree;
    }
    if weight > 0.0 { sum / weight } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use dioramic_spec::{LayoutAlgorithm, ValueSource};

    use super::*;
    use crate::{View, fixture};

    #[test]
    fn metrics_follow_the_plan() {
        let scene = fixture("checkout").scene(View::default());
        let metrics = LayoutMetrics::of(&scene);
        assert!(metrics.edge_length_total > 0.0 && metrics.area_tiles > 0.0);
        assert!((metrics.edge_length_mean * scene.edges.len() as f64 - metrics.edge_length_total).abs() < 1e-9);
        assert!(metrics.hub_centrality_offset > 0.0);
    }

    #[test]
    fn hub_layout_is_tighter_on_the_web_platform() {
        let example = fixture("web-platform");
        // Each algorithm is set on the parsed model, as if written in the spec, so the comparison holds whatever
        // the theme default is and whatever else the example writes under `layout:` (its `wrap`).
        let scene_with = |algorithm: LayoutAlgorithm| {
            let mut model = example.model();
            model.layout.algorithm = algorithm;
            model.layout.algorithm_source = ValueSource::Spec;
            dioramic_core::compile_with(&model, &View::default().compile_options())
                .unwrap_or_else(|diagnostics| panic!("web-platform ({algorithm:?}): {diagnostics}"))
                .scene
        };
        let before = LayoutMetrics::of(&scene_with(LayoutAlgorithm::Flow));
        let after = LayoutMetrics::of(&scene_with(LayoutAlgorithm::Hub));
        assert!(after.edge_length_total < before.edge_length_total, "{after:?} vs {before:?}");
        assert!(after.hub_centrality_offset <= before.hub_centrality_offset, "{after:?} vs {before:?}");
    }
}
