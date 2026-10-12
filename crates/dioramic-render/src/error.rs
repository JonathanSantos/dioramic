use dioramic_spec::Diagnostics;
use thiserror::Error;

/// Why a render failed. `Invalid` carries the spec diagnostics the AI must fix.
#[derive(Debug, Error)]
pub enum RenderError {
    /// The spec has `E_*` errors.
    #[error("invalid spec:\n{0}")]
    Invalid(Diagnostics),
    /// A hard limit was exceeded (size, frames, nodes).
    #[error("limit exceeded: {what} is {actual}, maximum is {limit}")]
    Limit {
        /// What was limited (`"width"`, `"frames"`).
        what: &'static str,
        /// Requested value.
        actual: u64,
        /// Limit.
        limit: u64,
    },
    /// The SVG could not be parsed or rasterized.
    #[error("raster failed: {0}")]
    Raster(String),
    /// PNG/GIF encoding failed.
    #[error("encoding failed: {0}")]
    Encode(String),
    /// The request is inconsistent (e.g. animated format without frames).
    #[error("invalid request: {0}")]
    Request(String),
    /// Feature not implemented yet (stub of the given task).
    #[error("not implemented yet ({0})")]
    NotImplemented(&'static str),
}

impl From<Diagnostics> for RenderError {
    fn from(diagnostics: Diagnostics) -> Self {
        Self::Invalid(diagnostics)
    }
}
