//! `dioramic render`: spec file → image file, through [`Engine::render`].

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use dioramic_render::{
    Engine, GridRequest, OutputFormat, PreviewMode, RenderError, RenderRequest, RenderResult, SourceText,
};
use dioramic_spec::{Diagnostics, LayoutAlgorithm, RotationSpec, Routing, SourceFormat};

use crate::cli::RenderArgs;
use crate::commands::{read_spec, spec_icons, text_format};
use crate::ui::{self, AlreadyReported, BOLD, DIM, GREEN, RED, UserError, paint};

/// Suffix stripped from the spec name to name the output (`checkout.diorama.yaml` → `checkout.png`).
const SPEC_INFIX: &str = ".diorama";

/// Runs `render` once.
pub fn run(args: &RenderArgs) -> Result<()> {
    let job = RenderJob::from_args(args)?;
    match job.render(Engine::shared()) {
        Ok(()) => Ok(()),
        Err(RenderFailure::Reported) => Err(AlreadyReported.into()),
        Err(RenderFailure::Other(error)) => Err(error),
    }
}

/// A resolved render: input, output path and request options.
#[derive(Debug, Clone)]
pub struct RenderJob {
    /// Spec file.
    pub spec: PathBuf,
    /// Output file.
    pub output: PathBuf,
    /// Output format.
    pub format: OutputFormat,
    args: RenderArgs,
}

/// Why one render did not produce a file.
#[derive(Debug)]
pub enum RenderFailure {
    /// Diagnostics or the error were already printed (spec errors); `watch` keeps going.
    Reported,
    /// Anything else (I/O, engine not available).
    Other(anyhow::Error),
}

impl From<anyhow::Error> for RenderFailure {
    fn from(error: anyhow::Error) -> Self {
        Self::Other(error)
    }
}

impl RenderJob {
    /// Resolves the format and the output path.
    pub fn from_args(args: &RenderArgs) -> Result<Self> {
        let format = resolve_format(args.format.map(OutputFormat::from), args.output.as_deref())?;
        let output = args.output.clone().unwrap_or_else(|| default_output_path(&args.spec, format));
        if let (Some(explicit), Some(extension)) = (args.format, extension_of(&output)) {
            let expected = OutputFormat::from(explicit).extension();
            if extension != expected {
                ui::print_warning(&format!(
                    "writing {} data to a .{extension} file (expected .{expected})",
                    OutputFormat::from(explicit).extension().to_uppercase()
                ));
            }
        }
        Ok(Self { spec: args.spec.clone(), output, format, args: args.clone() })
    }

    /// Reads the spec, renders it, writes the output and prints a one-line summary plus diagnostics.
    pub fn render(&self, engine: &Engine) -> Result<(), RenderFailure> {
        let text = read_spec(&self.spec)?;
        let icons = spec_icons(&self.spec, &text);
        let mut request = self.request(text);
        request.icons = icons;
        let result = match engine.render(&request) {
            Ok(result) => result,
            Err(error) => return Err(self.report_engine_error(error)),
        };
        self.write_output(&result)?;
        self.print_summary(&result);
        ui::print_diagnostics(&result.summary.diagnostics);
        Ok(())
    }

    fn request(&self, text: String) -> RenderRequest {
        let mut request = RenderRequest::new(dioramic_render::SpecSource::Text(SourceText {
            text,
            format: SourceFormat::from_path(&self.spec).map(text_format),
        }));
        request.format = self.format;
        request.theme = self.args.theme.map(Into::into);
        request.rotation = self.args.rotation;
        request.density = self.args.density.map(Into::into);
        request.routing = self.args.routing.map(Into::into);
        request.layout_algorithm = self.args.layout_algorithm.map(Into::into);
        request.grid = GridRequest {
            size: self.args.grid_size,
            style: self.args.grid_style.map(Into::into),
            opacity: self.args.grid_opacity,
            color: self.args.grid_color.clone(),
        };
        request.icon_animation = self.args.icon_animation();
        request.scale = self.args.scale;
        request.width_px = self.args.width;
        request.preview = PreviewMode::None;
        request
    }

    fn report_engine_error(&self, error: RenderError) -> RenderFailure {
        match error {
            RenderError::Invalid(diagnostics) => {
                report_invalid_spec(&self.spec, &diagnostics);
                RenderFailure::Reported
            }
            RenderError::NotImplemented(task) => RenderFailure::Other(
                UserError::new(format!("rendering is not available in this build yet ({task})"))
                    .hint(format!("`dioramic lint {}` already validates the spec", self.spec.display()))
                    .into(),
            ),
            RenderError::Limit { what, actual, limit } => RenderFailure::Other(
                UserError::new(format!("{what} is {actual}, the maximum is {limit}"))
                    .hint("lower --scale or --width, or split the diagram")
                    .into(),
            ),
            RenderError::Request(message) => RenderFailure::Other(UserError::new(message).into()),
            other => RenderFailure::Other(anyhow::Error::new(other).context("render failed")),
        }
    }

    fn write_output(&self, result: &RenderResult) -> Result<()> {
        if let Some(parent) = self.output.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent).with_context(|| format!("cannot create the directory {}", parent.display()))?;
        }
        fs::write(&self.output, &result.output).with_context(|| format!("cannot write {}", self.output.display()))
    }

    fn print_summary(&self, result: &RenderResult) {
        anstream::println!(
            "{} {}  {}",
            paint(GREEN, "✓"),
            paint(BOLD, self.output.display()),
            paint(DIM, summary_details(result, self.args.rotation))
        );
    }
}

/// Prints spec errors with a one-line header (shared with `watch`).
pub fn report_invalid_spec(spec: &Path, diagnostics: &Diagnostics) {
    anstream::eprintln!(
        "{} {}  {}",
        paint(RED, "✗"),
        paint(BOLD, spec.display()),
        paint(DIM, ui::diagnostic_counts(diagnostics))
    );
    ui::print_diagnostics(diagnostics);
}

/// `1600×900 px · 212.4 KB · rotation 90° (auto) · spaced · blueprint`.
fn summary_details(result: &RenderResult, requested: Option<RotationSpec>) -> String {
    let summary = &result.summary;
    let unit = if matches!(summary.format, OutputFormat::Png | OutputFormat::Gif) { "px" } else { "viewBox" };
    let mut parts = vec![format!("{}×{} {unit}", summary.width, summary.height), ui::human_bytes(summary.bytes)];
    if let (Some(frames), Some(duration_ms)) = (summary.frames, summary.duration_ms) {
        parts.push(format!("{frames} frames · {:.1} s", f64::from(duration_ms) / 1000.0));
    }
    let auto = if requested == Some(RotationSpec::Auto) { " (auto)" } else { "" };
    parts.push(format!("rotation {}°{auto}", summary.view.rotation.degrees()));
    parts.push(summary.view.density.as_str().to_owned());
    parts.push(summary.view.theme.as_str().to_owned());
    if summary.view.layout_algorithm != LayoutAlgorithm::Flow {
        parts.push(format!("{} layout", summary.view.layout_algorithm.as_str()));
    }
    if summary.view.routing != Routing::Curved {
        parts.push(routing_name(summary.view.routing).to_owned());
    }
    if summary.view.icon_animation == Some(false) {
        parts.push("icons still".to_owned());
    }
    parts.join(" · ")
}

/// Spec spelling of a routing (`rails`).
fn routing_name(routing: Routing) -> &'static str {
    match routing {
        Routing::Curved => "curved",
        Routing::Orthogonal => "orthogonal",
        Routing::Straight => "straight",
        Routing::Rails => "rails",
    }
}

/// `--format`, else the extension of `--output`, else PNG.
fn resolve_format(explicit: Option<OutputFormat>, output: Option<&Path>) -> Result<OutputFormat> {
    if let Some(format) = explicit {
        return Ok(format);
    }
    let Some(extension) = output.and_then(extension_of) else {
        return Ok(OutputFormat::Png);
    };
    match extension.as_str() {
        "png" => Ok(OutputFormat::Png),
        "svg" => Ok(OutputFormat::Svg),
        "gif" => Ok(OutputFormat::Gif),
        other => Err(UserError::new(format!("unsupported output extension '.{other}'"))
            .hint("use .png, .svg or .gif, or pass --format png|svg|svg-animated|gif")
            .into()),
    }
}

fn extension_of(path: &Path) -> Option<String> {
    path.extension().and_then(|extension| extension.to_str()).map(str::to_ascii_lowercase)
}

/// Next to the spec, without `.diorama.<ext>`: `diagrams/checkout.diorama.yaml` → `diagrams/checkout.png`.
fn default_output_path(spec: &Path, format: OutputFormat) -> PathBuf {
    let stem = spec.file_stem().and_then(|stem| stem.to_str()).unwrap_or("diagram");
    let stem = stem.strip_suffix(SPEC_INFIX).unwrap_or(stem);
    spec.with_file_name(format!("{stem}.{}", format.extension()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_sits_next_to_the_spec() {
        let path = default_output_path(Path::new("diagrams/checkout.diorama.yaml"), OutputFormat::Gif);
        assert_eq!(path, Path::new("diagrams/checkout.gif"));
        let path = default_output_path(Path::new("plain.yaml"), OutputFormat::Svg);
        assert_eq!(path, Path::new("plain.svg"));
    }

    #[test]
    fn format_comes_from_flag_then_extension_then_png() {
        let format = |explicit, output: Option<&str>| resolve_format(explicit, output.map(Path::new)).ok();
        assert_eq!(format(None, None), Some(OutputFormat::Png));
        assert_eq!(format(None, Some("x.GIF")), Some(OutputFormat::Gif));
        assert_eq!(format(None, Some("x.svg")), Some(OutputFormat::Svg));
        assert_eq!(format(Some(OutputFormat::SvgAnimated), Some("x.svg")), Some(OutputFormat::SvgAnimated));
        assert_eq!(format(None, Some("x.jpg")), None);
    }
}
