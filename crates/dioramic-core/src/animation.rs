//! Animation as a function of time: `scene(t)` (`docs/07-animacao.md`). Contract: C-005.
//! Implementation: T-104 (`flow` required; `build` if time allows; `story`/`pulse` later).
//!
//! `t ∈ [0, 1)` is the normalized loop time. Raster frames and the animated SVG are produced from the same
//! plan, so the GIF and the SVG match. Loops are seamless: the dash offset moves an integer number of
//! periods per loop and packets use `t mod 1` with a per-edge phase.

use dioramic_spec::{AnimationPreset, FlowMode, Model};

use crate::geometry::ScreenPoint;
use crate::scene::SceneEdge;

/// Dash pattern of animated edges (on, off) in px.
pub const DASH_PATTERN: (f64, f64) = (10.0, 6.0);
/// Dash period (on + off) in px.
pub const DASH_PERIOD: f64 = DASH_PATTERN.0 + DASH_PATTERN.1;
/// Dash periods travelled per loop (integer → seamless loop).
pub const DASH_PERIODS_PER_LOOP: f64 = 2.0;
/// Default loop duration (s).
pub const DEFAULT_DURATION_S: f64 = 2.0;
/// Default raster fps (50 fps = 2 cs GIF delay, the fastest browsers honor).
pub const DEFAULT_FPS: u32 = 50;
/// Phase step between consecutive edges' packets.
pub const PACKET_PHASE_STEP: f64 = 0.37;
/// `build`: normalized time when the first node starts dropping in.
const BUILD_NODES_START: f64 = 0.08;
/// `build`: normalized time span over which node start times are spread by rank.
const BUILD_NODES_SPREAD: f64 = 0.42;
/// `build`: duration of one node's drop-in.
const BUILD_NODE_DURATION: f64 = 0.16;
/// `build`: height a node falls from (px).
pub const BUILD_DROP_PX: f64 = 40.0;
/// `build`: when edges start fading in, and for how long.
const BUILD_EDGES_START: f64 = 0.7;
const BUILD_EDGES_DURATION: f64 = 0.15;
/// `build`: zones fade in during the first part of the timeline.
const BUILD_ZONES_DURATION: f64 = 0.08;
/// Overshoot of `easeOutBack` (the standard 1.70158).
const EASE_OUT_BACK_OVERSHOOT: f64 = 1.701_58;

/// Caller overrides (MCP `animation`, CLI flags).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AnimationOverrides {
    /// Preset.
    pub preset: Option<AnimationPreset>,
    /// Loop duration (s).
    pub duration_s: Option<f64>,
    /// Frames per second.
    pub fps: Option<u32>,
}

/// Resolved animation for a diagram.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationPlan {
    /// Preset.
    pub preset: AnimationPreset,
    /// Loop duration (s).
    pub duration_s: f64,
    /// Raster fps.
    pub fps: u32,
    /// Loops forever.
    pub looped: bool,
    /// Per edge (scene order): animated by `flow`? (all edges when none opts in).
    pub flowing_edges: Vec<bool>,
}

impl AnimationPlan {
    /// Plan from the model's `animation` block plus overrides. `None` when neither requests an animation.
    pub fn from_model(model: &Model, overrides: &AnimationOverrides) -> Option<Self> {
        let spec = model.animation.as_ref();
        if spec.is_none() && overrides.preset.is_none() {
            return None;
        }
        let preset = overrides.preset.or(spec.map(|animation| animation.preset)).unwrap_or_default();
        let duration_s = overrides
            .duration_s
            .or(spec.map(|animation| animation.duration_s))
            .filter(|duration| duration.is_finite() && *duration > 0.0)
            .unwrap_or(DEFAULT_DURATION_S);
        let fps = overrides.fps.or(spec.map(|animation| animation.fps)).filter(|fps| *fps > 0).unwrap_or(DEFAULT_FPS);
        let looped = match spec {
            Some(animation) if overrides.preset.is_none() => animation.looped,
            _ => preset != AnimationPreset::Build,
        };
        let any_opted_in = model.edges.iter().any(|edge| edge.flow != FlowMode::Off);
        let flowing_edges = model.edges.iter().map(|edge| !any_opted_in || edge.flow != FlowMode::Off).collect();
        Some(Self { preset, duration_s, fps, looped, flowing_edges })
    }

    /// True when edge `edge_index` carries the `flow` dashes and packet.
    pub fn is_flowing(&self, edge_index: usize) -> bool {
        self.preset_flows() && self.flowing_edges.get(edge_index).copied().unwrap_or(false)
    }

    /// Presets that animate traffic (`story`/`pulse` fall back to `flow` until implemented).
    fn preset_flows(&self) -> bool {
        self.preset != AnimationPreset::Build
    }

    /// Number of raster frames: `round(duration_s × fps)`, at least 1.
    pub fn frame_count(&self) -> u32 {
        let frames = (self.duration_s * f64::from(self.fps)).round();
        if frames.is_finite() && frames >= 1.0 { frames as u32 } else { 1 }
    }

    /// Normalized time of frame `index`: `index / frame_count` (never reaches 1.0 → seamless loop).
    pub fn frame_time(&self, index: u32) -> f64 {
        f64::from(index) / f64::from(self.frame_count())
    }

    /// GIF frame delay in centiseconds (`100 / fps`, at least 2: browsers clamp 0–1 cs to 10 cs).
    pub fn gif_delay_cs(&self) -> u16 {
        let delay = (100.0 / f64::from(self.fps.max(1))).round().max(2.0);
        delay.min(f64::from(u16::MAX)) as u16
    }
}

/// Packet phase of the `edge_index`-th edge: `(i × 0.37) mod 1`.
pub fn packet_phase(edge_index: usize) -> f64 {
    (edge_index as f64 * PACKET_PHASE_STEP).rem_euclid(1.0)
}

/// Fraction of the path over which a packet fades in after leaving its source and fades out before
/// reaching its target, so the jump from the end of the path back to its start (once per loop) is not
/// seen as a packet popping in and out (T-315).
pub const PACKET_FADE_FRACTION: f64 = 0.08;

/// Opacity of a packet at `along` (its position along the path in the travel direction, `[0, 1)`):
/// 0 at both ends, 1 between the fades.
pub fn packet_opacity(along: f64) -> f64 {
    let along = along.rem_euclid(1.0);
    let fade_in = along / PACKET_FADE_FRACTION;
    let fade_out = (1.0 - along) / PACKET_FADE_FRACTION;
    fade_in.min(fade_out).clamp(0.0, 1.0)
}

/// Opacity of the packet of edge `edge_index` at loop time `t` (see [`packet_opacity`]).
pub fn packet_opacity_at(edge_index: usize, t: f64) -> f64 {
    packet_opacity(t.rem_euclid(1.0) + packet_phase(edge_index))
}

/// Seam check of a looping animation (T-315): `step_changes[i]` counts the pixels that change from frame
/// `i` to frame `i + 1`, the last entry being the seam (last frame → first). Returns seam ÷ mean of the
/// other steps; 1 means the loop point is as smooth as any other step (target ≤ 1.15).
pub fn loop_seam_ratio(step_changes: &[u64]) -> Option<f64> {
    let (seam, steps) = step_changes.split_last()?;
    if steps.is_empty() {
        return None;
    }
    let mean = steps.iter().map(|&count| count as f64).sum::<f64>() / steps.len() as f64;
    (mean > 0.0).then(|| *seam as f64 / mean)
}

/// State of an edge at time `t`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeFrame {
    /// Dash offset (px), `None` when the edge is not animated.
    pub dash_offset: Option<f64>,
    /// Packet position on the edge (uniform speed by arc length).
    pub packet: Option<ScreenPoint>,
    /// Edge opacity (story dimming, build drawing).
    pub opacity: f64,
}

/// State of a node at time `t`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeFrame {
    /// Opacity.
    pub opacity: f64,
    /// Vertical lift in px (build drop-in with easeOutBack).
    pub lift_px: f64,
}

/// Evaluates an edge at `t`.
pub fn edge_frame(plan: &AnimationPlan, edge_index: usize, edge: &SceneEdge, t: f64) -> EdgeFrame {
    let t = t.rem_euclid(1.0);
    if plan.preset == AnimationPreset::Build {
        let opacity = ease_in_out_cubic(progress(t, BUILD_EDGES_START, BUILD_EDGES_DURATION));
        return EdgeFrame { dash_offset: None, packet: None, opacity };
    }
    if !plan.is_flowing(edge_index) {
        return EdgeFrame { dash_offset: None, packet: None, opacity: 1.0 };
    }
    let reverse = edge.flow == FlowMode::Reverse;
    let travelled = t * DASH_PERIOD * DASH_PERIODS_PER_LOOP;
    let dash_offset = if reverse { travelled } else { -travelled };
    let along = (t + packet_phase(edge_index)).rem_euclid(1.0);
    let fraction = if reverse { 1.0 - along } else { along };
    EdgeFrame { dash_offset: Some(dash_offset), packet: Some(edge.path.point_at_fraction(fraction)), opacity: 1.0 }
}

/// Evaluates a node at `t` given its flow rank.
pub fn node_frame(plan: &AnimationPlan, rank: u32, max_rank: u32, t: f64) -> NodeFrame {
    if plan.preset != AnimationPreset::Build {
        return NodeFrame { opacity: 1.0, lift_px: 0.0 };
    }
    let p = progress(t.clamp(0.0, 1.0), build_node_start(rank, max_rank), BUILD_NODE_DURATION);
    NodeFrame { opacity: ease_in_out_cubic(p), lift_px: (1.0 - ease_out_back(p)) * BUILD_DROP_PX }
}

/// Opacity of zones at `t` (`build` fades them in first; every other preset keeps them visible).
pub fn zone_opacity(plan: &AnimationPlan, t: f64) -> f64 {
    if plan.preset != AnimationPreset::Build {
        return 1.0;
    }
    ease_in_out_cubic(progress(t.clamp(0.0, 1.0), 0.0, BUILD_ZONES_DURATION))
}

/// Normalized time at which a node of `rank` starts dropping in (`build`): origins first.
pub fn build_node_start(rank: u32, max_rank: u32) -> f64 {
    let share = if max_rank == 0 { 0.0 } else { f64::from(rank.min(max_rank)) / f64::from(max_rank) };
    BUILD_NODES_START + share * BUILD_NODES_SPREAD
}

/// Build timing in seconds for the animated SVG: `(delay, duration)` of a node drop-in.
pub fn build_node_timing(plan: &AnimationPlan, rank: u32, max_rank: u32) -> (f64, f64) {
    (build_node_start(rank, max_rank) * plan.duration_s, BUILD_NODE_DURATION * plan.duration_s)
}

/// Build timing in seconds for the animated SVG: `(delay, duration)` of the edges fade-in.
pub fn build_edges_timing(plan: &AnimationPlan) -> (f64, f64) {
    (BUILD_EDGES_START * plan.duration_s, BUILD_EDGES_DURATION * plan.duration_s)
}

/// Build timing in seconds for the animated SVG: duration of the zones fade-in.
pub fn build_zones_duration(plan: &AnimationPlan) -> f64 {
    BUILD_ZONES_DURATION * plan.duration_s
}

fn progress(t: f64, start: f64, duration: f64) -> f64 {
    if duration <= 0.0 { f64::from(u8::from(t >= start)) } else { ((t - start) / duration).clamp(0.0, 1.0) }
}

fn ease_in_out_cubic(p: f64) -> f64 {
    if p < 0.5 { 4.0 * p * p * p } else { 1.0 - (-2.0 * p + 2.0).powi(3) / 2.0 }
}

fn ease_out_back(p: f64) -> f64 {
    let c1 = EASE_OUT_BACK_OVERSHOOT;
    let c3 = c1 + 1.0;
    1.0 + c3 * (p - 1.0).powi(3) + c1 * (p - 1.0).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(fps: u32, duration_s: f64) -> AnimationPlan {
        AnimationPlan { preset: AnimationPreset::Flow, duration_s, fps, looped: true, flowing_edges: Vec::new() }
    }

    #[test]
    fn readme_gif_recipe_has_100_frames_at_2cs() {
        let readme = plan(50, 2.0);
        assert_eq!(readme.frame_count(), 100);
        assert_eq!(readme.gif_delay_cs(), 2);
        assert!(readme.frame_time(99) < 1.0);
    }

    #[test]
    fn easing_hits_its_endpoints() {
        assert!(ease_in_out_cubic(0.0).abs() < 1e-12 && (ease_in_out_cubic(1.0) - 1.0).abs() < 1e-12);
        assert!(ease_out_back(0.0).abs() < 1e-12 && (ease_out_back(1.0) - 1.0).abs() < 1e-12);
        assert!(ease_out_back(0.8) > 1.0, "easeOutBack overshoots");
    }

    #[test]
    fn build_nodes_land_in_rank_order_and_stay() {
        let build = AnimationPlan { preset: AnimationPreset::Build, ..plan(50, 3.0) };
        let first = node_frame(&build, 0, 3, 0.2);
        let last = node_frame(&build, 3, 3, 0.2);
        assert!(first.opacity > last.opacity);
        assert_eq!(node_frame(&build, 3, 3, 0.99), NodeFrame { opacity: 1.0, lift_px: 0.0 });
        assert_eq!(node_frame(&build, 0, 0, 0.0).opacity, 0.0);
        assert_eq!(node_frame(&plan(50, 2.0), 2, 3, 0.0), NodeFrame { opacity: 1.0, lift_px: 0.0 });
    }

    #[test]
    fn phases_are_in_unit_interval() {
        assert!((packet_phase(1) - 0.37).abs() < 1e-12);
        assert!((0..50).map(packet_phase).all(|phase| (0.0..1.0).contains(&phase)));
    }

    #[test]
    fn packets_fade_at_both_ends_and_the_loop_point_is_continuous() {
        assert_eq!(packet_opacity(0.0), 0.0);
        assert_eq!(packet_opacity(0.5), 1.0);
        assert!((packet_opacity(PACKET_FADE_FRACTION / 2.0) - 0.5).abs() < 1e-12);
        assert!(packet_opacity(0.999) < 0.02);
        // Just before and just after the wrap the packet is (nearly) invisible: no pop.
        for edge in 0..5 {
            let wrap = (1.0 - packet_phase(edge)).rem_euclid(1.0);
            assert!(packet_opacity_at(edge, wrap - 1e-3) < 0.02 && packet_opacity_at(edge, wrap + 1e-3) < 0.02);
        }
    }

    #[test]
    fn loop_seam_ratio_compares_the_seam_with_the_mean_step() {
        assert_eq!(loop_seam_ratio(&[100, 100, 100, 115]), Some(1.15));
        assert_eq!(loop_seam_ratio(&[7]), None);
        assert_eq!(loop_seam_ratio(&[]), None);
    }
}
