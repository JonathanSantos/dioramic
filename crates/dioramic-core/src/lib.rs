//! Dioramic engine core: everything between a normalized [`Model`](dioramic_spec::Model) and an SVG string.
//!
//! No I/O and no native dependencies: this crate must keep compiling for `wasm32-unknown-unknown`
//! (`cargo check -p dioramic-core --target wasm32-unknown-unknown`).
//!
//! | Module | Contract | Owner |
//! |---|---|---|
//! | [`geometry`], [`canvas`], [`color`] | C-002 | refiner (done) |
//! | [`theme`], [`icons`] | C-002 / C-003 | T-102 |
//! | [`layout`] | C-004 | T-103 |
//! | [`scene`], [`svg`], [`animation`], [`text`], pipeline (`compile*`, `render_svg`) | C-005 | T-104 |

pub mod animation;
pub mod canvas;
pub mod color;
pub mod geometry;
pub mod icons;
pub mod layout;
mod pipeline;
pub mod scene;
pub mod svg;
pub mod text;
pub mod theme;

pub use pipeline::{CompileOptions, Diagram, compile, compile_source, compile_with, render_svg};

/// Re-export of the spec crate, so consumers need a single dependency for the common path.
pub use dioramic_spec as spec;
