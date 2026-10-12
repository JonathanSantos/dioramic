//! One gallery cell: compile a fixture in one view, measure the scene, write the SVG and the PNG.

use std::path::{Path, PathBuf};
use std::time::Instant;

use dioramic_core::Diagram;
use dioramic_core::scene::{ARROW_ANGLE_TOLERANCE_DEG, Scene};
use dioramic_core::svg::SvgOptions;
use dioramic_render::{Engine, OutputFormat, Pixmap, PreviewMode, RenderError, RenderRequest};
use dioramic_spec::{Density, RotationSpec, ThemeName};
use test_support::{Fixture, LayoutMetrics, SceneQuality, View, describe_arrow, describe_overlap, measure_arrows};

use super::panics::guarded;
use super::pixels::{Region, crop_zoom, fit_within};

/// Thumbnail box of the contact sheet (px).
pub const THUMBNAIL_WIDTH: u32 = 480;
/// Thumbnail box of the contact sheet (px).
pub const THUMBNAIL_HEIGHT: u32 = 330;

/// What one job needs besides the fixture and the view.
#[derive(Debug, Clone, Copy)]
pub struct JobSettings<'a> {
    /// `target/gallery/<example>/`.
    pub directory: &'a Path,
    /// PNG pixel density.
    pub scale: f64,
    /// Crop region and zoom.
    pub crop: Option<(Region, u32)>,
}

/// Geometry-only quality numbers of a scene.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SceneMetrics {
    /// Residual collision area (px²).
    pub collision_score: f64,
    /// One line per overlap.
    pub overlaps: Vec<String>,
    /// Number of arrow heads.
    pub arrow_heads: usize,
    /// Worst angle between head and target center (degrees).
    pub max_center_error_deg: f64,
    /// Worst angle between head and stroke arrival (degrees).
    pub max_tangent_error_deg: f64,
    /// Heads off target or hidden.
    pub arrow_problems: Vec<String>,
    /// Scene diagnostics (`W_LABEL_OVERLAP`, …).
    pub diagnostics: Vec<String>,
    /// Edges through nodes, longest leader, text size over width.
    pub quality: SceneQuality,
    /// Edge length, hub centrality and floor area (T-411).
    pub layout: LayoutMetrics,
}

impl SceneMetrics {
    fn of(scene: &Scene, diagram: &Diagram) -> Self {
        let measurements = measure_arrows(scene);
        let worst =
            |pick: fn(&test_support::ArrowMeasurement) -> f64| measurements.iter().map(pick).fold(0.0, f64::max);
        Self {
            collision_score: scene.collision.score,
            overlaps: scene.collision.overlaps.iter().map(describe_overlap).collect(),
            arrow_heads: measurements.len(),
            max_center_error_deg: worst(|measurement| measurement.center_error_deg),
            max_tangent_error_deg: worst(|measurement| measurement.tangent_error_deg),
            arrow_problems: measurements
                .iter()
                .filter(|measurement| !measurement.is_ok(ARROW_ANGLE_TOLERANCE_DEG))
                .map(describe_arrow)
                .collect(),
            diagnostics: diagram.diagnostics.iter().map(ToString::to_string).collect(),
            quality: SceneQuality::of(scene, &diagram.diagnostics),
            layout: LayoutMetrics::of(scene),
        }
    }

    /// Worst of the two arrow angles.
    pub fn max_arrow_error_deg(&self) -> f64 {
        self.max_center_error_deg.max(self.max_tangent_error_deg)
    }

    /// Something a reviewer should look at.
    pub fn has_problems(&self) -> bool {
        self.collision_score > test_support::COLLISION_SCORE_EPSILON || !self.arrow_problems.is_empty()
    }
}

/// Everything produced for one view.
#[derive(Debug, Default)]
pub struct ViewOutcome {
    /// SVG written.
    pub svg: Option<PathBuf>,
    /// PNG written.
    pub png: Option<PathBuf>,
    /// PNG size.
    pub png_size: Option<(u32, u32)>,
    /// Downscaled PNG for the contact sheet.
    pub thumbnail: Option<Pixmap>,
    /// Scene metrics.
    pub metrics: Option<SceneMetrics>,
    /// Compile time (ms).
    pub compile_ms: Option<f64>,
    /// Full render time, compile included (ms).
    pub render_ms: Option<f64>,
    /// Crops written.
    pub crops: Vec<PathBuf>,
    /// One line per failed stage (`compile: …`, `png: …`).
    pub errors: Vec<String>,
}

/// Renders one view of a fixture. Never panics: failures become [`ViewOutcome::errors`].
pub fn render_view(engine: &Engine, fixture: &Fixture, view: View, settings: JobSettings<'_>) -> ViewOutcome {
    let mut outcome = ViewOutcome::default();
    let stem = settings.directory.join(view.label());
    let svg_path = stem.with_extension("svg");
    let png_path = stem.with_extension("png");
    remove_stale(&[&svg_path, &png_path]);

    let request = request_for(fixture, view, settings.scale);
    let started = Instant::now();
    match flatten("compile", guarded(|| engine.compile(&request))) {
        Ok(diagram) => {
            outcome.compile_ms = Some(elapsed_ms(started));
            outcome.metrics = Some(SceneMetrics::of(&diagram.scene, &diagram));
            write_svg(&diagram, &svg_path, &mut outcome);
        }
        Err(error) => outcome.errors.push(error),
    }
    write_png(engine, &request, &png_path, settings, &mut outcome);
    outcome
}

/// `rotation: auto` for one theme × density: chosen angle and the score of each angle.
#[derive(Debug, Clone, PartialEq)]
pub struct AutoRotation {
    /// Theme.
    pub theme: ThemeName,
    /// Density.
    pub density: Density,
    /// Chosen angle and `(angle, score)` of the 4 candidates, or the error.
    pub result: Result<(u16, Vec<(u16, f64)>), String>,
}

/// Compiles with `rotation: auto` (geometry only) to report the score of each angle.
pub fn auto_rotation(engine: &Engine, fixture: &Fixture, theme: ThemeName, density: Density) -> AutoRotation {
    let mut request = request_for(fixture, View::new(theme, Default::default(), density), 1.0);
    request.rotation = Some(RotationSpec::Auto);
    let result = flatten("auto", guarded(|| engine.compile(&request))).and_then(|diagram| {
        let scene = &diagram.scene;
        let scores = scene
            .rotation_scores
            .map(|scores| scores.scores.iter().map(|(rotation, score)| (rotation.degrees(), *score)).collect())
            .ok_or_else(|| "auto: scene has no rotation_scores".to_owned())?;
        Ok((scene.rotation.degrees(), scores))
    });
    AutoRotation { theme, density, result }
}

fn request_for(fixture: &Fixture, view: View, scale: f64) -> RenderRequest {
    let mut request = RenderRequest::from_text(fixture.source.clone());
    request.format = OutputFormat::Png;
    request.theme = Some(view.theme);
    request.rotation = Some(RotationSpec::Fixed(view.rotation));
    request.density = Some(view.density);
    request.scale = scale;
    request.preview = PreviewMode::None;
    request
}

fn write_svg(diagram: &Diagram, path: &Path, outcome: &mut ViewOutcome) {
    let written = guarded(|| dioramic_core::render_svg(diagram, &SvgOptions::default()))
        .map_err(|error| format!("svg: {error}"))
        .and_then(|svg| std::fs::write(path, svg).map_err(|error| format!("svg: {}: {error}", path.display())));
    match written {
        Ok(()) => outcome.svg = Some(path.to_path_buf()),
        Err(error) => outcome.errors.push(error),
    }
}

fn write_png(
    engine: &Engine,
    request: &RenderRequest,
    path: &Path,
    settings: JobSettings<'_>,
    outcome: &mut ViewOutcome,
) {
    let started = Instant::now();
    let rendered = match flatten("png", guarded(|| engine.render(request))) {
        Ok(rendered) => rendered,
        Err(error) => {
            outcome.errors.push(error);
            return;
        }
    };
    outcome.render_ms = Some(elapsed_ms(started));
    if let Err(error) = std::fs::write(path, &rendered.output) {
        outcome.errors.push(format!("png: {}: {error}", path.display()));
        return;
    }
    outcome.png = Some(path.to_path_buf());
    let Ok(pixmap) = Pixmap::decode_png(&rendered.output) else {
        outcome.errors.push("png: engine output is not a decodable PNG".to_owned());
        return;
    };
    outcome.png_size = Some((pixmap.width(), pixmap.height()));
    outcome.thumbnail = fit_within(&pixmap, THUMBNAIL_WIDTH, THUMBNAIL_HEIGHT);
    if let Some((region, zoom)) = settings.crop {
        write_crop(&pixmap, region, zoom, path, outcome);
    }
}

fn write_crop(pixmap: &Pixmap, region: Region, zoom: u32, png_path: &Path, outcome: &mut ViewOutcome) {
    let stem = png_path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("view");
    let directory = png_path.with_file_name("crops");
    let path = directory.join(format!("{stem}-{}-z{zoom}.png", region.label()));
    let written = crop_zoom(pixmap, region, zoom)
        .and_then(|crop| crop.encode_png().map_err(|error| error.to_string()))
        .and_then(|bytes| {
            std::fs::create_dir_all(&directory)
                .and_then(|()| std::fs::write(&path, bytes))
                .map_err(|error| format!("{}: {error}", path.display()))
        });
    match written {
        Ok(()) => outcome.crops.push(path),
        Err(error) => outcome.errors.push(format!("crop: {error}")),
    }
}

/// Merges "panicked" and "returned an error" into one message prefixed with the stage.
fn flatten<T>(stage: &str, result: Result<Result<T, RenderError>, String>) -> Result<T, String> {
    match result {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(format!("{stage}: {}", single_line(&error.to_string()))),
        Err(panic) => Err(format!("{stage}: {panic}")),
    }
}

/// Diagnostics are multi-line; the console and the sheet want one line (the report keeps them all).
fn single_line(text: &str) -> String {
    text.lines().map(str::trim).filter(|line| !line.is_empty()).collect::<Vec<_>>().join(" | ")
}

fn remove_stale(paths: &[&Path]) {
    for path in paths {
        let _ = std::fs::remove_file(path);
    }
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}
