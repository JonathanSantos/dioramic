//! Dioramic MCP server (`rmcp`, stdio). Design: `docs/05-mcp.md`. Implementation: T-201.
//!
//! Tools (v1): `render_diagram`, `validate_diagram`, `search_icons`, `get_guide`. Every tool delegates to
//! [`dioramic_render::Engine`] (render/compile) and `dioramic_core::icons` (search); CPU-heavy work runs in
//! `tokio::task::spawn_blocking`. Logs go to stderr only (stdout is the JSON-RPC channel).
//!
//! Resources: `dioramic://schema/v1`, `dioramic://guide/{topic}`, `dioramic://examples/{name}`,
//! `dioramic://icons/catalog`. Prompts: `diagram-from-repo`, `animate-request-flow`, `redraw-image`.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod guides;
mod icons_tool;
mod outcome;
mod params;
mod prompts;
mod render_tool;
mod resources;
mod server;
mod source;
mod validate_tool;
mod workspace;

use dioramic_render::Engine;
use rmcp::ServiceExt;
use thiserror::Error;

pub use guides::GuideTopic;
pub use server::{DioramicServer, STRUCTURED_ENV, ServerOptions, descriptions};
pub use workspace::{PathError, Workspace};

/// Server name in `initialize`.
pub const SERVER_NAME: &str = "dioramic";

/// `instructions` of `initialize` (`docs/05-mcp.md#instruções-do-servidor`). Keep it short: clients inject it in
/// every conversation and Claude Code truncates it at 2048 characters.
pub const INSTRUCTIONS: &str = "dioramic draws beautiful isometric architecture diagrams from a small declarative spec (DioramaSpec).
Workflow: (1) write a minimal spec — nodes, edges, optional zones; NO coordinates or colors at first;
(2) call render_diagram; (3) look at the returned preview image and diagnostics; fix and re-render (max 3 rounds).
Call get_guide(\"spec\") the first time; call search_icons before using an icon you are not sure exists.
Edges shorthand: \"a -> b: label @style\". Prefer spec_path when you can edit files.
If the diagram is too long or sparse, set layout.density: compact; if labels or arrows are hidden, set view.rotation: auto.
For a high-level system design, use theme: schematic (hub layout, rails, edges colored by kind, legend) and leave wrap,
rotation and at out. Tangled edges: layout.algorithm: hub. Engineering look: layout.routing: rails (alias elbow).
Edge colors: layout.edge_colors semantic | style | mono; legend: view.legend auto | on | off.
GIF / animated SVG icons move a little (LEDs, waves); animation.icons: false keeps them still.
Outputs are written only inside the project (client roots or working directory), by default in ./diagrams/.";

/// Errors of the server process.
#[derive(Debug, Error)]
pub enum McpError {
    /// Transport / service failure.
    #[error("mcp service error: {0}")]
    Service(String),
}

/// Serves MCP over stdio until the client disconnects (`dioramic mcp`).
pub async fn serve_stdio() -> Result<(), McpError> {
    let server = DioramicServer::new(Engine::new());
    let running = server.serve(rmcp::transport::stdio()).await.map_err(|error| McpError::Service(error.to_string()))?;
    tracing::info!("dioramic MCP server ready on stdio");
    running.waiting().await.map_err(|error| McpError::Service(error.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Claude Code truncates `instructions` and each tool description at 2048 chars (research/clients.md §1.3).
    const CLIENT_TRUNCATION_MARGIN_CHARS: usize = 2000;

    #[test]
    fn instructions_fit_the_client_limit() {
        assert!(INSTRUCTIONS.chars().count() < CLIENT_TRUNCATION_MARGIN_CHARS);
    }

    #[test]
    fn tool_descriptions_fit_the_client_limit_and_carry_search_words() {
        let server = DioramicServer::with_options(Engine::new(), ServerOptions::default());
        let tools = server.tools();
        assert_eq!(tools.len(), 4);
        for tool in &tools {
            let description = tool.description.as_deref().unwrap_or_default();
            assert!(description.chars().count() < CLIENT_TRUNCATION_MARGIN_CHARS, "{} is too long", tool.name);
            assert!(description.contains("diagram"), "{} must mention 'diagram' for tool search", tool.name);
        }
    }

    #[test]
    fn structured_content_is_on_by_default() {
        assert!(ServerOptions::default().structured_content);
    }
}
