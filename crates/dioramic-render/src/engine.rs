//! The facade.

use std::sync::{Arc, OnceLock};

use dioramic_core::animation::{AnimationOverrides, AnimationPlan};
use dioramic_core::scene::Scene;
use dioramic_core::svg::{SvgMode, SvgOptions, TextRendering, emit_svg};
use dioramic_core::{CompileOptions, Diagram, compile_source, compile_with, render_svg};
use dioramic_spec::{AnimationPreset, Model, SourceFormat, SpecV1, normalize, parse, validate};
use resvg::tiny_skia::Pixmap;
use resvg::usvg::fontdb;

use crate::error::RenderError;
use crate::filmstrip::filmstrip;
use crate::fonts::font_database;
use crate::frames::{checked_frame_count, evenly_spaced_indices, render_frames, render_frames_at};
use crate::gif::{GifOptions, encode_gif};
use crate::overrides;
use crate::png::encode_png;
use crate::preview::downscale;
use crate::raster::{RasterSize, rasterize};
use crate::request::{
    OutputFormat, Preview, PreviewKind, PreviewMode, RenderRequest, RenderResult, RenderSummary, SpecSource,
    ViewSummary,
};
use crate::{FILMSTRIP_FRAMES, GIF_DEFAULT_WIDTH_PX, PREVIEW_MAX_SIDE_PX};

/// Render engine. Cheap to clone; holds the shared font database. One per process is enough
/// (MCP server and desktop keep it for their lifetime).
#[derive(Debug, Clone)]
pub struct Engine {
    fonts: Arc<fontdb::Database>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    /// Engine with the embedded fonts.
    pub fn new() -> Self {
        Self { fonts: font_database() }
    }

    /// Process-wide engine.
    pub fn shared() -> &'static Engine {
        static ENGINE: OnceLock<Engine> = OnceLock::new();
        ENGINE.get_or_init(Engine::new)
    }

    /// Font database used for raster.
    pub fn fonts(&self) -> &Arc<fontdb::Database> {
        &self.fonts
    }

    /// Parses/normalizes/validates the source and compiles it with the request overrides, without
    /// rasterizing (MCP `validate_diagram`, desktop live preview).
    pub fn compile(&self, request: &RenderRequest) -> Result<Diagram, RenderError> {
        let options = compile_options(request);
        let diagram = match &request.source {
            SpecSource::Text(source) => {
                let format = source.format.map_or_else(|| SourceFormat::sniff(&source.text), SourceFormat::from);
                if request.has_spec_overrides() {
                    compile_spec(parse(&source.text, format)?, request, &options)?
                } else {
                    compile_source(&source.text, format, &options)?
                }
            }
            SpecSource::Spec(spec) => compile_spec(spec.as_ref().clone(), request, &options)?,
            SpecSource::Model(model) => {
                let mut model = model.as_ref().clone();
                overrides::apply_to_model(&mut model, request);
                compile_validated(&model, &options)?
            }
        };
        Ok(diagram)
    }

    /// Renders: compile → SVG → (raster → PNG | frames → GIF) → preview → summary.
    pub fn render(&self, request: &RenderRequest) -> Result<RenderResult, RenderError> {
        let diagram = self.compile(request)?;
        let rendered = match request.format {
            OutputFormat::Png => render_png(&diagram, request)?,
            OutputFormat::Svg => render_static_svg(&diagram, request)?,
            OutputFormat::SvgAnimated => render_animated_svg(&diagram, request)?,
            OutputFormat::Gif => render_gif(&diagram, request)?,
        };
        let summary = RenderSummary {
            format: request.format,
            width: rendered.width,
            height: rendered.height,
            bytes: rendered.output.len() as u64,
            frames: rendered.timing.map(|timing| timing.frames),
            duration_ms: rendered.timing.map(|timing| timing.duration_ms),
            view: view_summary(&diagram, request.format),
            diagnostics: diagram.diagnostics,
        };
        Ok(RenderResult { output: rendered.output, preview: rendered.preview, summary })
    }
}

/// Output of one format before the summary is assembled.
struct Rendered {
    output: Vec<u8>,
    width: u32,
    height: u32,
    timing: Option<Timing>,
    preview: Option<Preview>,
}

/// Frames and loop length of an animated output.
#[derive(Clone, Copy)]
struct Timing {
    frames: u32,
    duration_ms: u32,
}

impl Timing {
    fn of(plan: &AnimationPlan) -> Self {
        let duration_ms = (plan.duration_s * MILLIS_PER_SECOND).round().clamp(0.0, f64::from(u32::MAX)) as u32;
        Self { frames: plan.frame_count(), duration_ms }
    }
}

const MILLIS_PER_SECOND: f64 = 1000.0;
/// Largest PNG pixel density accepted (`docs/05-mcp.md`).
const MAX_SCALE: f64 = 4.0;
/// Raster scale of the preview of SVG exports before downscaling to [`PREVIEW_MAX_SIDE_PX`].
const SVG_PREVIEW_SCALE: f64 = 1.0;

fn render_png(diagram: &Diagram, request: &RenderRequest) -> Result<Rendered, RenderError> {
    let size = match request.width_px {
        Some(width) => RasterSize::Width(width),
        None => RasterSize::Scale(checked_scale(request.scale)?),
    };
    let svg = render_svg(diagram, &SvgOptions { metadata: false, ..SvgOptions::default() });
    let pixmap = rasterize(&svg, size)?;
    let preview = wants_preview(request).then(|| image_preview(&pixmap)).transpose()?;
    Ok(Rendered { output: encode_png(&pixmap)?, width: pixmap.width(), height: pixmap.height(), timing: None, preview })
}

fn render_static_svg(diagram: &Diagram, request: &RenderRequest) -> Result<Rendered, RenderError> {
    let options = SvgOptions { text: text_rendering(request), ..SvgOptions::default() };
    let svg = render_svg(diagram, &options);
    let preview = wants_preview(request)
        .then(|| image_preview(&rasterize(&svg, RasterSize::Scale(SVG_PREVIEW_SCALE))?))
        .transpose()?;
    let (width, height) = viewbox_size(&diagram.scene);
    Ok(Rendered { output: svg.into_bytes(), width, height, timing: None, preview })
}

fn render_animated_svg(diagram: &Diagram, request: &RenderRequest) -> Result<Rendered, RenderError> {
    let plan = animation_plan(diagram, request)?;
    let timing = Timing::of(&plan);
    checked_frame_count(&plan)?;
    let options = SvgOptions { mode: SvgMode::Animated, text: text_rendering(request), ..SvgOptions::default() };
    let svg = emit_svg(&diagram.scene, Some(&plan), &options);
    let preview = if wants_preview(request) {
        let times: Vec<f64> = evenly_spaced_indices(timing.frames as usize, FILMSTRIP_FRAMES)
            .into_iter()
            .map(|index| plan.frame_time(index as u32))
            .collect();
        let frames = render_frames_at(diagram, &plan, &times, RasterSize::Scale(SVG_PREVIEW_SCALE))?;
        Some(filmstrip_preview(&frames)?)
    } else {
        None
    };
    let (width, height) = viewbox_size(&diagram.scene);
    Ok(Rendered { output: svg.into_bytes(), width, height, timing: Some(timing), preview })
}

fn render_gif(diagram: &Diagram, request: &RenderRequest) -> Result<Rendered, RenderError> {
    let plan = animation_plan(diagram, request)?;
    let width = request.width_px.unwrap_or(GIF_DEFAULT_WIDTH_PX);
    let frames = render_frames(diagram, &plan, RasterSize::Width(width))?;
    let (width, height) = frames.first().map_or((0, 0), |frame| (frame.width(), frame.height()));
    let options = GifOptions { delay_cs: plan.gif_delay_cs(), ..GifOptions::default() };
    let output = encode_gif(&frames, &options)?;
    let preview = wants_preview(request).then(|| filmstrip_preview(&frames)).transpose()?;
    Ok(Rendered { output, width, height, timing: Some(Timing::of(&plan)), preview })
}

/// The diagram's own plan, or a `flow` loop (with the request overrides) when the spec has no animation:
/// asking for an animated format is enough to animate.
fn animation_plan(diagram: &Diagram, request: &RenderRequest) -> Result<AnimationPlan, RenderError> {
    if let Some(plan) = &diagram.animation {
        return Ok(plan.clone());
    }
    let overrides = AnimationOverrides {
        preset: Some(request.animation.preset.unwrap_or(AnimationPreset::Flow)),
        duration_s: request.animation.duration_s,
        fps: request.animation.fps,
    };
    AnimationPlan::from_model(&diagram.model, &overrides)
        .ok_or_else(|| RenderError::Request(format!("{} needs an animation", request.format.extension())))
}

fn checked_scale(scale: f64) -> Result<f64, RenderError> {
    if scale.is_finite() && scale > 0.0 && scale <= MAX_SCALE {
        Ok(scale)
    } else {
        Err(RenderError::Request(format!("scale must be in (0, {MAX_SCALE}], got {scale}")))
    }
}

fn text_rendering(request: &RenderRequest) -> TextRendering {
    if request.svg_text_as_outlines { TextRendering::Outlines } else { TextRendering::Text }
}

fn wants_preview(request: &RenderRequest) -> bool {
    request.preview == PreviewMode::Image
}

fn image_preview(pixmap: &Pixmap) -> Result<Preview, RenderError> {
    preview_of(&downscale(pixmap, PREVIEW_MAX_SIDE_PX)?, PreviewKind::Image)
}

fn filmstrip_preview(frames: &[Pixmap]) -> Result<Preview, RenderError> {
    preview_of(&filmstrip(frames, PREVIEW_MAX_SIDE_PX)?, PreviewKind::Filmstrip)
}

fn preview_of(pixmap: &Pixmap, kind: PreviewKind) -> Result<Preview, RenderError> {
    Ok(Preview { png: encode_png(pixmap)?, width: pixmap.width(), height: pixmap.height(), kind })
}

fn viewbox_size(scene: &Scene) -> (u32, u32) {
    let side = |length: f64| length.round().clamp(0.0, f64::from(u32::MAX)) as u32;
    (side(scene.viewbox.width()), side(scene.viewbox.height()))
}

fn view_summary(diagram: &Diagram, format: OutputFormat) -> ViewSummary {
    let scene = &diagram.scene;
    let model = &diagram.model;
    ViewSummary {
        rotation: scene.rotation,
        density: scene.density,
        theme: scene.theme.name,
        routing: model.layout.routing,
        layout_algorithm: model.layout.algorithm,
        grid: model.view.grid.clone(),
        icon_animation: format.is_animated().then(|| model.animation.as_ref().is_none_or(|animation| animation.icons)),
        rotation_scores: scene.rotation_scores.map(|scores| {
            scores.scores.iter().map(|(rotation, score)| (rotation.degrees().to_string(), *score)).collect()
        }),
    }
}

/// Spec → overrides → normalize → validate → compile.
fn compile_spec(mut spec: SpecV1, request: &RenderRequest, options: &CompileOptions) -> Result<Diagram, RenderError> {
    overrides::apply_to_spec(&mut spec, request);
    compile_validated(&normalize(spec)?, options)
}

fn compile_validated(model: &Model, options: &CompileOptions) -> Result<Diagram, RenderError> {
    let findings = validate(model);
    if findings.has_errors() {
        return Err(RenderError::Invalid(findings));
    }
    let mut diagram = compile_with(model, options)?;
    let mut diagnostics = findings;
    diagnostics.extend(diagram.diagnostics);
    diagnostics.sort();
    diagram.diagnostics = diagnostics;
    Ok(diagram)
}

fn compile_options(request: &RenderRequest) -> CompileOptions {
    CompileOptions {
        theme: request.theme,
        rotation: request.rotation,
        density: request.density,
        tile_px: None,
        animation: AnimationOverrides {
            preset: request.animation.preset,
            duration_s: request.animation.duration_s,
            fps: request.animation.fps,
        },
        icons: request.icons.clone(),
    }
}
