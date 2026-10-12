//! Animation in the SVG: values at `t` for raster frames, CSS + SMIL for the animated document.
//! Both come from the same [`AnimationPlan`], so the GIF and the animated SVG match.

use std::fmt::Write as _;

use dioramic_spec::{AnimationPreset, FlowMode};

use crate::animation::{
    self, AnimationPlan, DASH_PATTERN, DASH_PERIOD, DASH_PERIODS_PER_LOOP, PACKET_FADE_FRACTION, build_edges_timing,
    build_node_timing, build_zones_duration, edge_frame, node_frame, packet_opacity_at, packet_phase,
};
use crate::canvas::{GroupAttributes, IsoCanvas, ItemRole, ItemTag, Stroke, Style, fmt_num, fmt_ratio};
use crate::color::Color;
use crate::geometry::{Rect, ScreenPoint};
use crate::scene::{Scene, SceneEdge, SceneNode};

use super::SvgMode;
use super::icon_motion::HookRender;
use super::writing::path_data;

/// Packet look: glow radius, dot radius and outline (px); glow blur std deviation.
const PACKET_GLOW_RADIUS: f64 = 9.0;
const PACKET_DOT_RADIUS: f64 = 4.2;
const PACKET_OUTLINE_WIDTH: f64 = 2.4;
const PACKET_GLOW_BLUR: f64 = 3.0;
/// Easing of the build drop-in in CSS (`easeOutBack`) and of fades.
const CSS_EASE_OUT_BACK: &str = "cubic-bezier(0.34,1.56,0.64,1)";

/// What an animated group belongs to.
#[derive(Debug, Clone, Copy)]
pub enum MotionKind {
    /// A zone or zone label.
    Zone,
    /// An edge or its pill.
    Edge,
    /// A node part (leader, label) with its flow rank and the scene's max rank.
    Node(u32, u32),
}

/// The animation context of one emission.
pub struct Motion<'a> {
    plan: Option<&'a AnimationPlan>,
    mode: SvgMode,
    max_rank: u32,
}

impl<'a> Motion<'a> {
    /// Without a plan every mode behaves like `Static`.
    pub fn new(scene: &Scene, plan: Option<&'a AnimationPlan>, mode: SvgMode) -> Self {
        let max_rank = scene.nodes.iter().map(|node| node.rank).max().unwrap_or(0);
        match plan {
            Some(plan) => Self { plan: Some(plan), mode, max_rank },
            None => Self { plan: None, mode: SvgMode::Static, max_rank },
        }
    }

    fn frame_time(&self) -> Option<(f64, &'a AnimationPlan)> {
        match (self.mode, self.plan) {
            (SvgMode::Frame { t }, Some(plan)) => Some((t, plan)),
            _ => None,
        }
    }

    fn animated_plan(&self) -> Option<&'a AnimationPlan> {
        match self.mode {
            SvgMode::Animated => self.plan,
            _ => None,
        }
    }

    fn is_build(&self) -> bool {
        self.plan.is_some_and(|plan| plan.preset == AnimationPreset::Build)
    }

    /// Opens the tagged group of an item, with its opacity at `t` (frames) or its build classes (animated).
    pub fn open(&self, canvas: &mut IsoCanvas, tag: ItemTag, kind: MotionKind) {
        let mut attributes = GroupAttributes { tag: Some(tag), ..GroupAttributes::default() };
        if let Some((t, plan)) = self.frame_time() {
            let opacity = match kind {
                MotionKind::Zone => animation::zone_opacity(plan, t),
                MotionKind::Edge if plan.preset == AnimationPreset::Build => edge_frame_opacity(plan, t),
                MotionKind::Edge => 1.0,
                MotionKind::Node(rank, max_rank) => node_frame(plan, rank, max_rank, t).opacity,
            };
            attributes.opacity = (opacity < 1.0).then_some(opacity);
        } else if self.animated_plan().is_some() && self.is_build() {
            attributes.class = Some(match kind {
                MotionKind::Zone => "build-zone".to_owned(),
                MotionKind::Edge => "build-edge".to_owned(),
                MotionKind::Node(rank, _) => format!("build-node rank-{rank}"),
            });
        }
        canvas.open_group(&attributes);
    }

    /// Opening tag wrapping a node body (`build` lift and opacity), or `None` when the node is static.
    pub fn wrap_node(&self, node: &SceneNode, max_rank: u32) -> Option<String> {
        if !self.is_build() {
            return None;
        }
        if let Some((t, plan)) = self.frame_time() {
            let frame = node_frame(plan, node.rank, max_rank, t);
            return Some(format!(
                "<g transform=\"translate(0,{})\" opacity=\"{}\">",
                fmt_num(-frame.lift_px),
                fmt_ratio(frame.opacity)
            ));
        }
        self.animated_plan().map(|_| format!("<g class=\"build-node rank-{}\">", node.rank))
    }

    /// How the icons' motion hooks are written (T-402): static without a plan, at `t` in frames, SMIL when animated.
    pub fn icon_hooks(&self) -> HookRender {
        match (self.mode, self.plan) {
            (SvgMode::Frame { t }, Some(plan)) => HookRender::Frame { t, duration_s: plan.duration_s },
            (SvgMode::Animated, Some(plan)) => HookRender::Animated { duration_s: plan.duration_s },
            _ => HookRender::Static,
        }
    }

    fn flowing(&self, index: usize) -> bool {
        self.plan.is_some_and(|plan| plan.is_flowing(index))
    }

    /// Stroke of an edge: dashes with the offset at `t` in frames; dashes left to CSS in the animated mode.
    pub fn edge_stroke(&self, index: usize, edge: &SceneEdge, base: Stroke) -> Stroke {
        if !self.flowing(index) {
            return base;
        }
        if let Some((t, plan)) = self.frame_time() {
            let mut stroke = base.dashed(DASH_PATTERN.0, DASH_PATTERN.1);
            stroke.dash_offset = edge_frame(plan, index, edge, t).dash_offset;
            return stroke;
        }
        base
    }

    /// CSS class of a flowing edge in the animated mode.
    pub fn edge_class(&self, index: usize, edge: &SceneEdge) -> Option<String> {
        if self.animated_plan().is_none() || !self.flowing(index) {
            return None;
        }
        Some(if edge.flow == FlowMode::Reverse { "flow-reverse" } else { "flow" }.to_owned())
    }

    /// Packets: a dot at `t` per flowing edge (frames) or a dot riding `<animateMotion>` (animated).
    pub fn draw_packets(&self, canvas: &mut IsoCanvas, scene: &Scene) {
        for (index, edge) in scene.edges.iter().enumerate() {
            if !self.flowing(index) {
                continue;
            }
            if let Some((t, plan)) = self.frame_time() {
                let Some(position) = edge_frame(plan, index, edge, t).packet else { continue };
                let opacity = packet_opacity_at(index, t);
                if opacity <= 0.0 {
                    continue;
                }
                open_packet(canvas, edge, (opacity < 1.0).then_some(opacity));
                packet_dot(canvas, scene, edge.color, position);
                canvas.close_group();
            } else if let Some(plan) = self.animated_plan() {
                open_packet(canvas, edge, None);
                packet_dot(canvas, scene, edge.color, ScreenPoint::new(0.0, 0.0));
                let key_points = if edge.flow == FlowMode::Reverse { "1;0" } else { "0;1" };
                let duration = plan.duration_s;
                let begin = -packet_phase(index) * duration;
                let mut motion = String::new();
                let _ = write!(
                    motion,
                    "<animateMotion dur=\"{}s\" begin=\"{}s\" repeatCount=\"indefinite\" calcMode=\"linear\" keyPoints=\"{key_points}\" keyTimes=\"0;1\" path=\"{}\"/>",
                    fmt_ratio(duration),
                    fmt_ratio(begin),
                    path_data(&edge.path).as_str()
                );
                // Same fade as the frames: invisible at both ends of the path, opaque in between.
                let _ = write!(
                    motion,
                    "<animate attributeName=\"opacity\" dur=\"{}s\" begin=\"{}s\" repeatCount=\"indefinite\" values=\"0;1;1;0\" keyTimes=\"0;{};{};1\"/>",
                    fmt_ratio(duration),
                    fmt_ratio(begin),
                    fmt_ratio(PACKET_FADE_FRACTION),
                    fmt_ratio(1.0 - PACKET_FADE_FRACTION)
                );
                canvas.raw(&motion, Rect::EMPTY);
                canvas.close_group();
            }
        }
    }

    /// Registers the defs the animation uses (packet glow).
    pub fn define(&self, canvas: &mut IsoCanvas, scene: &Scene) {
        let has_packets = (0..scene.edges.len()).any(|index| self.flowing(index))
            && (self.frame_time().is_some() || self.animated_plan().is_some());
        if has_packets {
            canvas.define(
                "glow",
                format!(
                    "<filter id=\"glow\" x=\"-100%\" y=\"-100%\" width=\"300%\" height=\"300%\"><feGaussianBlur stdDeviation=\"{PACKET_GLOW_BLUR}\"/></filter>"
                ),
            );
        }
    }

    /// `<style>` of the animated mode (empty otherwise).
    pub fn stylesheet(&self) -> String {
        let Some(plan) = self.animated_plan() else {
            return String::new();
        };
        let duration = fmt_ratio(plan.duration_s);
        let travel = fmt_num(DASH_PERIOD * DASH_PERIODS_PER_LOOP);
        let dash = format!("{} {}", fmt_num(DASH_PATTERN.0), fmt_num(DASH_PATTERN.1));
        let mut css = String::from("<style>");
        if plan.preset == AnimationPreset::Build {
            let iterations = if plan.looped { "infinite" } else { "1" };
            let (edges_delay, edges_duration) = build_edges_timing(plan);
            let _ = write!(
                css,
                ".build-zone{{animation:dioramic-fade {}s ease-in-out {iterations} both}}.build-edge{{animation:dioramic-fade {}s ease-in-out {}s {iterations} both}}.build-node{{animation:dioramic-drop {}s {CSS_EASE_OUT_BACK} {iterations} both}}@keyframes dioramic-fade{{from{{opacity:0}}to{{opacity:1}}}}@keyframes dioramic-drop{{from{{opacity:0;transform:translateY(-{}px)}}to{{opacity:1;transform:translateY(0)}}}}",
                fmt_ratio(build_zones_duration(plan)),
                fmt_ratio(edges_duration),
                fmt_ratio(edges_delay),
                fmt_ratio(build_node_timing(plan, 0, 1).1),
                fmt_num(animation::BUILD_DROP_PX),
            );
            for rank in 0..=self.max_rank {
                let (delay, _) = build_node_timing(plan, rank, self.max_rank);
                let _ = write!(css, ".rank-{rank}{{animation-delay:{}s}}", fmt_ratio(delay));
            }
        } else {
            let _ = write!(
                css,
                ".flow{{stroke-dasharray:{dash};animation:dioramic-flow {duration}s linear infinite}}.flow-reverse{{stroke-dasharray:{dash};animation:dioramic-flow-reverse {duration}s linear infinite}}@keyframes dioramic-flow{{from{{stroke-dashoffset:0}}to{{stroke-dashoffset:-{travel}}}}}@keyframes dioramic-flow-reverse{{from{{stroke-dashoffset:0}}to{{stroke-dashoffset:{travel}}}}}"
            );
        }
        css.push_str("</style>");
        css
    }
}

fn edge_frame_opacity(plan: &AnimationPlan, t: f64) -> f64 {
    let (delay, duration) = build_edges_timing(plan);
    let start = delay / plan.duration_s;
    let span = duration / plan.duration_s;
    let p = ((t - start) / span).clamp(0.0, 1.0);
    if p < 0.5 { 4.0 * p * p * p } else { 1.0 - (-2.0 * p + 2.0).powi(3) / 2.0 }
}

fn open_packet(canvas: &mut IsoCanvas, edge: &SceneEdge, opacity: Option<f64>) {
    canvas.open_group(&GroupAttributes {
        tag: Some(ItemTag::new(edge.id.as_str(), ItemRole::Packet)),
        opacity,
        ..GroupAttributes::default()
    });
}

fn packet_dot(canvas: &mut IsoCanvas, scene: &Scene, color: Color, center: ScreenPoint) {
    let glow = Style::fill(color).with_opacity(scene.theme.glow_opacity).with_filter("glow");
    canvas.circle(center, PACKET_GLOW_RADIUS, &glow);
    let dot = Style::fill(Color::WHITE).with_stroke(Stroke::new(color, PACKET_OUTLINE_WIDTH));
    canvas.circle(center, PACKET_DOT_RADIUS, &dot);
}
