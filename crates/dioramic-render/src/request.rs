//! Request / result types of [`crate::Engine::render`]. Serializable so the desktop app can send them
//! over Tauri IPC unchanged; the MCP server maps its own tool schema onto them.

use std::collections::BTreeMap;

use dioramic_core::icons::IconRegistry;
use dioramic_spec::{
    AnimationPreset, ColorRef, Density, Diagnostics, GridOptions, GridStyle, LayoutAlgorithm, Model, Rotation,
    RotationSpec, Routing, SourceFormat, SpecV1, ThemeName,
};
use serde::{Deserialize, Serialize};

/// Source text format (serializable mirror of [`SourceFormat`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextFormat {
    /// JSON.
    Json,
    /// YAML.
    Yaml,
}

impl From<TextFormat> for SourceFormat {
    fn from(format: TextFormat) -> Self {
        match format {
            TextFormat::Json => SourceFormat::Json,
            TextFormat::Yaml => SourceFormat::Yaml,
        }
    }
}

/// Spec text plus its format (`None` → sniffed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceText {
    /// Spec text.
    pub text: String,
    /// Format; sniffed from content when absent.
    #[serde(default)]
    pub format: Option<TextFormat>,
}

/// Where the spec comes from. The engine never reads files: callers load `spec_path` themselves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecSource {
    /// Raw text (file contents, editor buffer).
    Text(SourceText),
    /// Already parsed spec (MCP inline `spec`).
    Spec(Box<SpecV1>),
    /// Already normalized model.
    Model(Box<Model>),
}

/// Output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputFormat {
    /// PNG at `scale` (default).
    #[default]
    Png,
    /// Static SVG (text as outlines by default).
    Svg,
    /// SVG with CSS/SMIL animation.
    SvgAnimated,
    /// High-quality GIF: 1600 px, 50 fps, global palette, delta frames, seamless loop.
    Gif,
}

impl OutputFormat {
    /// File extension without dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Svg | Self::SvgAnimated => "svg",
            Self::Gif => "gif",
        }
    }

    /// MIME type.
    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Svg | Self::SvgAnimated => "image/svg+xml",
            Self::Gif => "image/gif",
        }
    }

    /// True for formats with motion (preview is a filmstrip).
    pub fn is_animated(self) -> bool {
        matches!(self, Self::SvgAnimated | Self::Gif)
    }
}

/// Whether to return a preview image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PreviewMode {
    /// PNG ≤ 1024 px (filmstrip for animated formats).
    #[default]
    Image,
    /// No preview.
    None,
}

/// Animation overrides.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct AnimationRequest {
    /// Preset.
    #[serde(default)]
    pub preset: Option<AnimationPreset>,
    /// Loop duration (s).
    #[serde(default)]
    pub duration_s: Option<f64>,
    /// Frames per second.
    #[serde(default)]
    pub fps: Option<u32>,
}

/// Background grid overrides (`view.grid`). Each `None` keeps what the spec says; the values are checked
/// like the spec's own (`size` 0.25–4 tiles, `opacity` 0–1 → `E_BAD_VALUE`; `color` → `E_BAD_COLOR`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GridRequest {
    /// Side of a grid cell in plan tiles.
    #[serde(default)]
    pub size: Option<f64>,
    /// Lines, dots or none.
    #[serde(default)]
    pub style: Option<GridStyle>,
    /// Opacity, 0 to 1.
    #[serde(default)]
    pub opacity: Option<f64>,
    /// Color token or hex.
    #[serde(default)]
    pub color: Option<ColorRef>,
}

impl GridRequest {
    /// True when no grid field is overridden.
    pub fn is_empty(&self) -> bool {
        self.size.is_none() && self.style.is_none() && self.opacity.is_none() && self.color.is_none()
    }
}

/// Everything needed to render once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderRequest {
    /// Spec.
    pub source: SpecSource,
    /// Output format.
    #[serde(default)]
    pub format: OutputFormat,
    /// Theme override.
    #[serde(default)]
    pub theme: Option<ThemeName>,
    /// Rotation override (`auto` allowed).
    #[serde(default)]
    pub rotation: Option<RotationSpec>,
    /// Density override.
    #[serde(default)]
    pub density: Option<Density>,
    /// Default edge routing override (`layout.routing`). Edges with their own `route` keep it, exactly as when
    /// the spec sets `layout.routing`, so pinning the override reproduces the render.
    #[serde(default)]
    pub routing: Option<Routing>,
    /// Placement algorithm override (`layout.algorithm`: `flow` or `hub`). Without it the spec's, or the theme's
    /// default (`schematic` lays out with `hub`), is used.
    #[serde(default)]
    pub layout_algorithm: Option<LayoutAlgorithm>,
    /// Background grid overrides (`view.grid`).
    #[serde(default)]
    pub grid: GridRequest,
    /// Small looping motion inside the icons in animated outputs (`animation.icons`); `None` keeps the spec's.
    #[serde(default)]
    pub icon_animation: Option<bool>,
    /// PNG pixel density, 1–4 (default 2). Ignored by GIF when `width_px` is set.
    #[serde(default = "default_scale")]
    pub scale: f64,
    /// Target output width in px (GIF default 1600; PNG default = viewBox width × scale).
    #[serde(default)]
    pub width_px: Option<u32>,
    /// Animation overrides.
    #[serde(default)]
    pub animation: AnimationRequest,
    /// Preview.
    #[serde(default)]
    pub preview: PreviewMode,
    /// Emit SVG text as glyph outlines (default true for `svg`/`svg-animated` exports).
    #[serde(default = "default_true")]
    pub svg_text_as_outlines: bool,
    /// Flat icons (installed provider packs, project icons) loaded by the caller; the engine reads no files.
    /// Not serialized: each process loads its own (`dioramic_packs`).
    #[serde(skip)]
    pub icons: IconRegistry,
}

fn default_scale() -> f64 {
    2.0
}

fn default_true() -> bool {
    true
}

impl RenderRequest {
    /// PNG request with defaults for a source.
    pub fn new(source: SpecSource) -> Self {
        Self {
            source,
            format: OutputFormat::default(),
            theme: None,
            rotation: None,
            density: None,
            routing: None,
            layout_algorithm: None,
            grid: GridRequest::default(),
            icon_animation: None,
            scale: default_scale(),
            width_px: None,
            animation: AnimationRequest::default(),
            preview: PreviewMode::default(),
            svg_text_as_outlines: true,
            icons: IconRegistry::default(),
        }
    }

    /// Convenience: request from spec text (format sniffed).
    pub fn from_text(text: impl Into<String>) -> Self {
        Self::new(SpecSource::Text(SourceText { text: text.into(), format: None }))
    }

    /// True when the request changes the spec itself (routing, layout algorithm, grid, icon motion), not only
    /// compile options.
    pub fn has_spec_overrides(&self) -> bool {
        self.routing.is_some()
            || self.layout_algorithm.is_some()
            || !self.grid.is_empty()
            || self.icon_animation.is_some()
    }
}

/// Kind of preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PreviewKind {
    /// Downscaled single image.
    Image,
    /// 4 key frames side by side.
    Filmstrip,
}

/// Preview PNG for the AI / UI thumbnails.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preview {
    /// PNG bytes.
    pub png: Vec<u8>,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
    /// Kind.
    pub kind: PreviewKind,
}

/// View actually used (resolved `auto`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewSummary {
    /// Rotation used.
    pub rotation: Rotation,
    /// Density used.
    pub density: Density,
    /// Theme used.
    pub theme: ThemeName,
    /// Default edge routing used (`layout.routing`).
    #[serde(default)]
    pub routing: Routing,
    /// Placement algorithm used (`layout.algorithm`, after the theme's default).
    #[serde(default)]
    pub layout_algorithm: LayoutAlgorithm,
    /// Background grid used (`view.grid`, defaults filled in).
    #[serde(default)]
    pub grid: GridOptions,
    /// Whether the icons move (animated formats only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_animation: Option<bool>,
    /// Collision score per angle (`"0"`, `"90"`…) when `auto` was requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation_scores: Option<BTreeMap<String, f64>>,
}

/// JSON summary returned with every render (`docs/05-mcp.md`, minus paths which the caller adds).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderSummary {
    /// Format.
    pub format: OutputFormat,
    /// Output width (px; viewBox width for SVG).
    pub width: u32,
    /// Output height.
    pub height: u32,
    /// Output size in bytes.
    pub bytes: u64,
    /// Frames (animated formats).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frames: Option<u32>,
    /// Loop duration (ms, animated formats).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u32>,
    /// View.
    pub view: ViewSummary,
    /// Warnings and hints.
    pub diagnostics: Diagnostics,
}

/// Result of a render.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderResult {
    /// Output bytes (PNG, SVG UTF-8, GIF).
    pub output: Vec<u8>,
    /// Preview, if requested.
    pub preview: Option<Preview>,
    /// Summary.
    pub summary: RenderSummary,
}
