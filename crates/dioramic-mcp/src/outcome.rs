//! Tool failures and how they become `CallToolResult`s. Every failure the model can act on is a tool
//! result with `isError: true` (never a JSON-RPC error, which clients show opaquely).

use std::io;
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use dioramic_render::RenderError;
use dioramic_spec::Diagnostics;
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{Value, json};
use thiserror::Error;

use crate::workspace::PathError;

/// MIME of every image the server returns.
pub const PNG_MIME: &str = "image/png";

/// Why a tool call failed.
#[derive(Debug, Error)]
pub enum ToolError {
    /// The spec has `E_*` errors.
    #[error("the spec has {} error(s); nothing was rendered", .0.errors().count())]
    InvalidSpec(Diagnostics),
    /// Path outside the roots or unresolvable.
    #[error(transparent)]
    Path(#[from] PathError),
    /// Inconsistent parameters.
    #[error("{0}")]
    BadRequest(String),
    /// Format listed in the schema but not implemented.
    #[error("format '{0}' is not supported yet; use png, svg, svg-animated or gif")]
    UnsupportedFormat(&'static str),
    /// The engine failed for a reason other than the spec.
    #[error("render failed: {0}")]
    Render(String),
    /// Reading or writing a file failed.
    #[error("cannot {action} '{path}': {source}")]
    Io {
        /// `read` | `write` | `create directory`.
        action: &'static str,
        /// File.
        path: String,
        /// Cause.
        source: io::Error,
    },
    /// Bug or unfinished engine part (a panic in a blocking task).
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<RenderError> for ToolError {
    fn from(error: RenderError) -> Self {
        match error {
            RenderError::Invalid(diagnostics) => Self::InvalidSpec(diagnostics),
            other => Self::Render(other.to_string()),
        }
    }
}

impl From<Diagnostics> for ToolError {
    fn from(diagnostics: Diagnostics) -> Self {
        Self::InvalidSpec(diagnostics)
    }
}

impl ToolError {
    /// I/O error on `path`.
    pub fn io(action: &'static str, path: &Path, source: io::Error) -> Self {
        Self::Io { action, path: path.display().to_string(), source }
    }

    /// Diagnostics carried by the error (empty unless the spec is invalid).
    pub fn diagnostics(&self) -> Diagnostics {
        match self {
            Self::InvalidSpec(diagnostics) => diagnostics.clone(),
            _ => Diagnostics::new(),
        }
    }

    /// `isError: true` result: a readable message, then the JSON payload (`error` + `diagnostics`).
    pub fn into_call_result(self, structured: bool) -> CallToolResult {
        let diagnostics = self.diagnostics();
        let message = match &self {
            Self::InvalidSpec(_) => format!("{self}. Fix these and call again:\n{}", bullet_list(&diagnostics)),
            _ => self.to_string(),
        };
        let payload = json!({ "error": self.to_string(), "diagnostics": diagnostics });
        let mut result =
            CallToolResult::error(vec![ContentBlock::text(message), ContentBlock::text(payload.to_string())]);
        if structured {
            result.structured_content = Some(payload);
        }
        result
    }
}

/// Successful result: `blocks` followed by the JSON payload as text (so clients that drop
/// `structuredContent` or images still get everything), plus `structuredContent` when enabled.
pub fn success(blocks: Vec<ContentBlock>, payload: Value, structured: bool) -> CallToolResult {
    let mut content = blocks;
    content.push(ContentBlock::text(payload.to_string()));
    let mut result = CallToolResult::success(content);
    if structured {
        result.structured_content = Some(payload);
    }
    result
}

/// PNG bytes as an MCP image block.
pub fn png_block(png: &[u8]) -> ContentBlock {
    ContentBlock::image(BASE64.encode(png), PNG_MIME)
}

/// One diagnostic per line (`- E_UNKNOWN_REF edges[2]: ... (hint)`).
pub fn bullet_list(diagnostics: &Diagnostics) -> String {
    diagnostics.iter().map(|diagnostic| format!("- {diagnostic}")).collect::<Vec<_>>().join("\n")
}

/// Runs CPU-heavy work on the blocking pool. A panic (e.g. an engine part still `todo!()`) becomes
/// [`ToolError::Internal`] instead of tearing down the server.
pub async fn run_blocking<T, F>(work: F) -> Result<T, ToolError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ToolError> + Send + 'static,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(outcome) => outcome,
        Err(join_error) if join_error.is_panic() => Err(ToolError::Internal(panic_message(join_error.into_panic()))),
        Err(join_error) => Err(ToolError::Internal(join_error.to_string())),
    }
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "engine panicked".to_owned())
}

#[cfg(test)]
mod tests {
    use dioramic_spec::{Diagnostic, DiagnosticCode};

    use super::*;

    fn unknown_ref() -> Diagnostics {
        Diagnostic::new(DiagnosticCode::UnknownRef, "edges[0]", "node 'dbb' does not exist")
            .with_hint("did you mean 'db'?")
            .into()
    }

    #[test]
    fn invalid_spec_is_an_error_result_with_diagnostics() {
        let result = ToolError::InvalidSpec(unknown_ref()).into_call_result(true);
        assert_eq!(result.is_error, Some(true));
        let message = result.content[0].as_text().map(|text| text.text.clone()).unwrap_or_default();
        assert!(message.contains("E_UNKNOWN_REF edges[0]"), "{message}");
        let structured = result.structured_content.unwrap_or_default();
        assert_eq!(structured["diagnostics"][0]["code"], "E_UNKNOWN_REF");
    }

    #[test]
    fn structured_content_can_be_disabled() {
        let result = ToolError::BadRequest("nope".to_owned()).into_call_result(false);
        assert!(result.structured_content.is_none());
        assert_eq!(result.content.len(), 2);
    }

    #[test]
    fn render_errors_map_by_kind() {
        assert!(matches!(ToolError::from(RenderError::Invalid(unknown_ref())), ToolError::InvalidSpec(_)));
        assert!(matches!(ToolError::from(RenderError::NotImplemented("T-105")), ToolError::Render(_)));
    }

    #[tokio::test]
    async fn panics_in_blocking_work_become_internal_errors() {
        let outcome: Result<(), ToolError> = run_blocking(|| panic!("not yet implemented: T-102")).await;
        assert!(matches!(outcome, Err(ToolError::Internal(message)) if message.contains("T-102")));
    }

    #[test]
    fn success_appends_the_payload_as_text() {
        let result = success(vec![png_block(&[1, 2, 3])], json!({ "ok": true }), true);
        assert_eq!(result.is_error, Some(false));
        assert!(result.content[0].as_image().is_some());
        assert_eq!(result.content[1].as_text().map(|text| text.text.as_str()), Some("{\"ok\":true}"));
    }
}
