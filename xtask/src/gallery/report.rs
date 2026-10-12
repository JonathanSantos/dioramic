//! `target/gallery/report.json`: machine-readable quality numbers of every render.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use super::job::{AutoRotation, SceneMetrics, ViewOutcome};
use test_support::{LayoutMetrics, SceneQuality, View};

/// Whole run.
#[derive(Debug, Serialize)]
pub struct Report {
    /// `--filter` used.
    pub filter: Option<String>,
    /// PNG scale.
    pub scale: f64,
    /// Themes rendered (`--themes`).
    pub themes: Vec<String>,
    /// Totals over every render.
    pub totals: Totals,
    /// Per example.
    pub examples: Vec<ExampleReport>,
}

/// Totals.
#[derive(Debug, Default, Serialize)]
pub struct Totals {
    /// Renders attempted.
    pub renders: usize,
    /// Renders without any error (SVG and PNG written).
    pub complete: usize,
    /// Renders with at least one failed stage.
    pub failed: usize,
    /// Renders whose scene has residual collisions.
    pub with_collisions: usize,
    /// Renders with arrows off target or hidden.
    pub with_arrow_problems: usize,
    /// Worst collision score (px²).
    pub max_collision_score: f64,
    /// Worst arrow angle error (degrees).
    pub max_arrow_error_deg: f64,
    /// Renders with at least one `W_EDGE_THROUGH_NODE`.
    pub with_edge_through_node: usize,
    /// Most `W_EDGE_THROUGH_NODE` in one render.
    pub max_edge_through_node: usize,
    /// Longest label leader of any render (px).
    pub max_leader_px: f64,
    /// Edge × edge crossings summed over every render (T-400).
    pub crossings: usize,
    /// Crossings between edges that share a node, summed over every render (should be 0).
    pub crossings_shared_node: usize,
    /// Renders with at least one crossing between edges that share a node.
    pub with_crossings_shared_node: usize,
    /// Most bends of one edge in any render (T-406).
    pub bends_max: usize,
    /// Mean bends per edge over every render (mean of the per-render means).
    pub bends_mean: f64,
    /// Longest route over the Manhattan distance between its ends, in any render (T-407).
    pub detour_max: f64,
    /// Stroke glued to another edge (parallel, closer than a lane), summed over every render (tiles, T-407).
    pub overlap_len: f64,
    /// Renders with some stroke glued to another edge.
    pub with_overlap: usize,
    /// Longest route over the Manhattan distance between the centers of its nodes, in any render (T-421).
    pub detour_center_max: f64,
    /// Longest stretch of one route past its nodes, in any render (tiles, T-421).
    pub stray_max: f64,
    /// Most bends of one elbow route above the fewest its end faces allow, in any render (T-421).
    pub excess_bends_max: usize,
    /// Renders with an elbow route two bends or more above the fewest its end faces allow (T-421).
    pub with_excess_bends: usize,
    /// Edge × node label pairs where the stroke runs under the label, summed over every render (T-422).
    pub under_node_label: usize,
    /// Pairs of edges crossing each other twice or more, summed over every render (T-422).
    pub double_crossings: usize,
    /// Sum of the per-render means and number of measured renders (for `bends_mean`).
    #[serde(skip)]
    bends_mean_sum: f64,
    #[serde(skip)]
    measured: usize,
    /// Smallest label size / viewBox width of any render (%); `None` before the first scene.
    pub min_text_to_width: Option<f64>,
    /// Wall time of the whole gallery (ms).
    pub wall_ms: f64,
}

/// One example.
#[derive(Debug, Serialize)]
pub struct ExampleReport {
    /// Example name.
    pub name: String,
    /// Contact sheet path (relative to the workspace).
    pub contact_sheet: Option<String>,
    /// `rotation: auto` per theme × density.
    pub auto_rotation: Vec<AutoRotationReport>,
    /// Every view.
    pub renders: Vec<RenderReport>,
}

/// `rotation: auto` result.
#[derive(Debug, Serialize)]
pub struct AutoRotationReport {
    /// Theme.
    pub theme: String,
    /// Density.
    pub density: String,
    /// Angle chosen by `auto`.
    pub chosen: Option<u16>,
    /// Collision score per angle (`"0"`, `"90"`, …).
    pub rotation_scores: Option<BTreeMap<u16, f64>>,
    /// Why it could not be computed.
    pub error: Option<String>,
}

impl AutoRotationReport {
    /// From a job result.
    pub fn of(auto: &AutoRotation) -> Self {
        let (chosen, rotation_scores, error) = match &auto.result {
            Ok((chosen, scores)) => {
                let scores = scores.iter().map(|(degrees, score)| (*degrees, round(*score))).collect();
                (Some(*chosen), Some(scores), None)
            }
            Err(error) => (None, None, Some(error.clone())),
        };
        Self {
            theme: auto.theme.as_str().to_owned(),
            density: auto.density.as_str().to_owned(),
            chosen,
            rotation_scores,
            error,
        }
    }
}

/// One view.
#[derive(Debug, Serialize)]
pub struct RenderReport {
    /// `blueprint-r90-compact`.
    pub view: String,
    /// Theme.
    pub theme: String,
    /// Rotation (degrees).
    pub rotation: u16,
    /// Density.
    pub density: String,
    /// SVG path.
    pub svg: Option<String>,
    /// PNG path.
    pub png: Option<String>,
    /// PNG size `[width, height]`.
    pub png_size: Option<[u32; 2]>,
    /// Collisions.
    pub collision: Option<CollisionReport>,
    /// Arrow heads.
    pub arrows: Option<ArrowReport>,
    /// Edges flagged `W_EDGE_THROUGH_NODE` (the collision score ignores them).
    pub edge_through_node: Option<usize>,
    /// Longest leader line from an icon to its label (px).
    pub max_leader_px: Option<f64>,
    /// Edge × edge crossing points (T-400).
    pub crossings: Option<usize>,
    /// Crossing points between edges that share a node (should be 0).
    pub crossings_shared_node: Option<usize>,
    /// Most 90° bends of one edge (T-406).
    pub bends_max: Option<usize>,
    /// Mean bends per edge.
    pub bends_mean: Option<f64>,
    /// Longest route over the Manhattan distance between its ends (T-407).
    pub detour_max: Option<f64>,
    /// Stroke glued to another edge: parallel and closer than one lane (tiles, T-407).
    pub overlap_len: Option<f64>,
    /// Longest route over the Manhattan distance between the centers of its nodes (T-421).
    pub detour_center_max: Option<f64>,
    /// Longest stretch of one route past its nodes (tiles, T-421).
    pub stray_max: Option<f64>,
    /// Most bends of one elbow route above the fewest its end faces allow (T-421).
    pub excess_bends_max: Option<usize>,
    /// Edge × node label pairs where the stroke runs under the label (T-422).
    pub under_node_label: Option<usize>,
    /// Pairs of edges crossing each other twice or more (T-422).
    pub double_crossings: Option<usize>,
    /// Where the sources stand in the reading of the screen, 0 = first, 1 = last (T-415).
    pub source_reading_rank: Option<f64>,
    /// Edge length, hub centrality and floor area of the plan, in tiles (T-411).
    pub layout: Option<LayoutReport>,
    /// Node label font size / viewBox width, in percent (README demo ≈ 1.3).
    pub text_to_width: Option<f64>,
    /// Scene diagnostics.
    pub diagnostics: Vec<String>,
    /// Compile time (ms).
    pub compile_ms: Option<f64>,
    /// Render time (ms, compile included).
    pub render_ms: Option<f64>,
    /// Crops written.
    pub crops: Vec<String>,
    /// Failed stages.
    pub errors: Vec<String>,
}

/// Layout numbers of a view (T-411), in tiles.
#[derive(Debug, Serialize)]
pub struct LayoutReport {
    /// Sum of the distances between the centers of the ends of every edge.
    pub edge_length_total: f64,
    /// Mean of those distances.
    pub edge_length_mean: f64,
    /// Mean distance of the nodes to the middle of the plan, weighted by their edge count.
    pub hub_centrality_offset: f64,
    /// Area of the box around zones and node centers (tiles²).
    pub area_tiles: f64,
}

impl LayoutReport {
    fn of(metrics: &LayoutMetrics) -> Self {
        Self {
            edge_length_total: round(metrics.edge_length_total),
            edge_length_mean: round(metrics.edge_length_mean),
            hub_centrality_offset: round(metrics.hub_centrality_offset),
            area_tiles: round(metrics.area_tiles),
        }
    }
}

/// Collision numbers.
#[derive(Debug, Serialize)]
pub struct CollisionReport {
    /// Residual area (px²); 0 = nothing overlaps.
    pub score: f64,
    /// One line per overlap.
    pub overlaps: Vec<String>,
}

/// Arrow numbers.
#[derive(Debug, Serialize)]
pub struct ArrowReport {
    /// Heads measured.
    pub heads: usize,
    /// Worst head-vs-center angle (degrees).
    pub max_center_error_deg: f64,
    /// Worst head-vs-tangent angle (degrees).
    pub max_tangent_error_deg: f64,
    /// Worst of both.
    pub max_error_deg: f64,
    /// Heads off target or hidden.
    pub problems: Vec<String>,
}

impl RenderReport {
    /// From a job result; paths relative to `root`.
    pub fn of(view: View, outcome: &ViewOutcome, root: &Path) -> Self {
        let relative = |path: &Path| path.strip_prefix(root).unwrap_or(path).display().to_string();
        let metrics = outcome.metrics.as_ref();
        Self {
            view: view.label(),
            theme: view.theme.as_str().to_owned(),
            rotation: view.rotation.degrees(),
            density: view.density.as_str().to_owned(),
            svg: outcome.svg.as_deref().map(relative),
            png: outcome.png.as_deref().map(relative),
            png_size: outcome.png_size.map(|(width, height)| [width, height]),
            collision: metrics.map(|metrics| CollisionReport {
                score: round(metrics.collision_score),
                overlaps: metrics.overlaps.clone(),
            }),
            arrows: metrics.map(ArrowReport::of),
            edge_through_node: metrics.map(|metrics| metrics.quality.edge_through_node),
            max_leader_px: metrics.map(|metrics| round(metrics.quality.max_leader_px)),
            crossings: metrics.map(|metrics| metrics.quality.crossings),
            crossings_shared_node: metrics.map(|metrics| metrics.quality.crossings_shared_node),
            bends_max: metrics.map(|metrics| metrics.quality.bends_max),
            bends_mean: metrics.map(|metrics| round(metrics.quality.bends_mean)),
            detour_max: metrics.map(|metrics| round(metrics.quality.detour_max)),
            overlap_len: metrics.map(|metrics| round(metrics.quality.overlap_len)),
            detour_center_max: metrics.map(|metrics| round(metrics.quality.detour_center_max)),
            stray_max: metrics.map(|metrics| round(metrics.quality.stray_max)),
            excess_bends_max: metrics.map(|metrics| metrics.quality.excess_bends_max),
            under_node_label: metrics.map(|metrics| metrics.quality.under_node_label),
            double_crossings: metrics.map(|metrics| metrics.quality.double_crossings),
            source_reading_rank: metrics.and_then(|metrics| metrics.quality.source_reading_rank).map(round),
            layout: metrics.map(|metrics| LayoutReport::of(&metrics.layout)),
            text_to_width: metrics.map(|metrics| round_percent(metrics.quality.text_to_width_percent)),
            diagnostics: metrics.map(|metrics| metrics.diagnostics.clone()).unwrap_or_default(),
            compile_ms: outcome.compile_ms.map(round),
            render_ms: outcome.render_ms.map(round),
            crops: outcome.crops.iter().map(|path| relative(path)).collect(),
            errors: outcome.errors.clone(),
        }
    }
}

impl ArrowReport {
    fn of(metrics: &SceneMetrics) -> Self {
        Self {
            heads: metrics.arrow_heads,
            max_center_error_deg: round(metrics.max_center_error_deg),
            max_tangent_error_deg: round(metrics.max_tangent_error_deg),
            max_error_deg: round(metrics.max_arrow_error_deg()),
            problems: metrics.arrow_problems.clone(),
        }
    }
}

impl Totals {
    /// Adds one render.
    pub fn add(&mut self, outcome: &ViewOutcome) {
        self.renders += 1;
        if outcome.errors.is_empty() {
            self.complete += 1;
        } else {
            self.failed += 1;
        }
        if let Some(metrics) = &outcome.metrics {
            if metrics.collision_score > test_support::COLLISION_SCORE_EPSILON {
                self.with_collisions += 1;
            }
            if !metrics.arrow_problems.is_empty() {
                self.with_arrow_problems += 1;
            }
            self.max_collision_score = self.max_collision_score.max(round(metrics.collision_score));
            self.max_arrow_error_deg = self.max_arrow_error_deg.max(round(metrics.max_arrow_error_deg()));
            self.add_quality(&metrics.quality);
        }
    }

    fn add_quality(&mut self, quality: &SceneQuality) {
        if quality.edge_through_node > 0 {
            self.with_edge_through_node += 1;
        }
        self.max_edge_through_node = self.max_edge_through_node.max(quality.edge_through_node);
        self.max_leader_px = self.max_leader_px.max(round(quality.max_leader_px));
        self.crossings += quality.crossings;
        self.crossings_shared_node += quality.crossings_shared_node;
        if quality.crossings_shared_node > 0 {
            self.with_crossings_shared_node += 1;
        }
        self.bends_max = self.bends_max.max(quality.bends_max);
        self.bends_mean_sum += quality.bends_mean;
        self.measured += 1;
        self.bends_mean = round(self.bends_mean_sum / self.measured as f64);
        self.detour_max = self.detour_max.max(round(quality.detour_max));
        let overlap = round(quality.overlap_len);
        self.overlap_len = round(self.overlap_len + overlap);
        if overlap > 0.0 {
            self.with_overlap += 1;
        }
        self.detour_center_max = self.detour_center_max.max(round(quality.detour_center_max));
        self.stray_max = self.stray_max.max(round(quality.stray_max));
        self.excess_bends_max = self.excess_bends_max.max(quality.excess_bends_max);
        if quality.excess_bends_max >= EXCESS_BENDS_FLAGGED {
            self.with_excess_bends += 1;
        }
        self.under_node_label += quality.under_node_label;
        self.double_crossings += quality.double_crossings;
        let text = round_percent(quality.text_to_width_percent);
        self.min_text_to_width = Some(self.min_text_to_width.map_or(text, |current| current.min(text)));
    }
}

/// Bends above the fewest of the end faces from which a route reads as a step or a walk round (T-421).
const EXCESS_BENDS_FLAGGED: usize = 2;

/// Three decimals: text/width percentages differ in the second decimal between densities.
pub fn round_percent(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// Two decimals: enough for px² and degrees, and stable diffs between runs.
pub fn round(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gallery::job::SceneMetrics;

    fn outcome(quality: SceneQuality) -> ViewOutcome {
        ViewOutcome { metrics: Some(SceneMetrics { quality, ..SceneMetrics::default() }), ..ViewOutcome::default() }
    }

    #[test]
    fn totals_track_the_legibility_extremes() {
        let mut totals = Totals::default();
        totals.add(&outcome(SceneQuality {
            edge_through_node: 0,
            max_leader_px: 40.0,
            text_to_width_percent: 1.29,
            crossings: 2,
            crossings_shared_node: 0,
            bends_max: 2,
            bends_mean: 1.0,
            detour_max: 1.2,
            overlap_len: 0.0,
            ..SceneQuality::default()
        }));
        totals.add(&outcome(SceneQuality {
            edge_through_node: 3,
            max_leader_px: 206.4,
            text_to_width_percent: 0.66,
            crossings: 3,
            crossings_shared_node: 1,
            bends_max: 4,
            bends_mean: 2.0,
            detour_max: 1.05,
            overlap_len: 0.5,
            stray_max: 1.25,
            excess_bends_max: 2,
            under_node_label: 1,
            double_crossings: 1,
            ..SceneQuality::default()
        }));
        assert_eq!(totals.with_edge_through_node, 1);
        assert_eq!(totals.max_edge_through_node, 3);
        assert_eq!(totals.max_leader_px, 206.4);
        assert_eq!(totals.min_text_to_width, Some(0.66));
        assert_eq!((totals.crossings, totals.crossings_shared_node, totals.with_crossings_shared_node), (5, 1, 1));
        assert_eq!((totals.bends_max, totals.bends_mean), (4, 1.5));
        assert_eq!(totals.detour_max, 1.2);
        assert_eq!((totals.overlap_len, totals.with_overlap), (0.5, 1));
        assert_eq!((totals.stray_max, totals.excess_bends_max, totals.with_excess_bends), (1.25, 2, 1));
        assert_eq!((totals.under_node_label, totals.double_crossings), (1, 1));
    }

    #[test]
    fn render_report_exposes_the_quality_fields() {
        let quality = SceneQuality {
            edge_through_node: 2,
            max_leader_px: 51.234,
            text_to_width_percent: 1.28765,
            crossings: 4,
            crossings_shared_node: 1,
            bends_max: 3,
            bends_mean: 1.25,
            detour_max: 1.3333,
            overlap_len: 0.256,
            detour_center_max: 1.234,
            ..SceneQuality::default()
        };
        let report = RenderReport::of(View::default(), &outcome(quality), Path::new("/"));
        let json = serde_json::to_value(&report).unwrap_or_default();
        assert_eq!(json["edge_through_node"], 2);
        assert_eq!(json["max_leader_px"], 51.23);
        assert_eq!(json["text_to_width"], 1.288);
        assert_eq!((json["crossings"].as_u64(), json["crossings_shared_node"].as_u64()), (Some(4), Some(1)));
        assert_eq!(json["detour_max"], 1.33);
        assert_eq!(json["overlap_len"], 0.26);
        assert_eq!(json["detour_center_max"], 1.23);
    }
}
