//! Dioramic render facade: the single entry point used by the MCP server, the CLI and the desktop app.
//!
//! ```text
//! RenderRequest ──Engine::render──▶ compile (dioramic-core) ─▶ SVG ─▶ resvg ─▶ PNG / GIF ─▶ RenderResult
//! ```
//!
//! Fonts are the embedded Inter/Space Grotesk files, loaded once per process ([`fonts::font_database`]).
//! Always use the types re-exported by resvg (`resvg::usvg`, `resvg::tiny_skia`, `resvg::usvg::fontdb`)
//! so versions never drift. Contract: `docs/work/contracts/C-006-render-engine.md`. Implementation: T-105.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod engine;
mod error;
pub mod filmstrip;
pub mod fonts;
pub mod frames;
pub mod gif;
mod overrides;
pub mod png;
pub mod preview;
pub mod raster;
mod request;

pub use engine::Engine;
pub use error::RenderError;
pub use request::{
    AnimationRequest, GridRequest, OutputFormat, Preview, PreviewKind, PreviewMode, RenderRequest, RenderResult,
    RenderSummary, SourceText, SpecSource, TextFormat, ViewSummary,
};

/// Re-export so consumers (tests, xtask) name the same `Pixmap` type as this crate.
pub use resvg::tiny_skia::Pixmap;

/// Longest side of the preview image returned to the AI (≈1k image tokens).
pub const PREVIEW_MAX_SIDE_PX: u32 = 1024;
/// Default GIF width (README column is ≤ 838 px; 1600 px ≈ 1:1 on retina).
pub const GIF_DEFAULT_WIDTH_PX: u32 = 1600;
/// Hard limit per output side (`docs/05-mcp.md#segurança`).
pub const MAX_SIDE_PX: u32 = 8192;
/// Hard limit of animation frames.
pub const MAX_FRAMES: u32 = 600;
/// Frames in the filmstrip preview of animated formats.
pub const FILMSTRIP_FRAMES: usize = 4;
