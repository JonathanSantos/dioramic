//! `dioramic mcp`: delegates to [`dioramic_mcp::serve_stdio`] on a tokio runtime.

use anyhow::{Context, Result};
use tracing_subscriber::EnvFilter;

use crate::ui::UserError;

/// Log filter variable; logs always go to stderr because stdout is the JSON-RPC channel.
const LOG_ENV: &str = "DIORAMIC_LOG";
const DEFAULT_LOG_FILTER: &str = "warn";

/// Serves until the client disconnects.
pub fn run() -> Result<()> {
    init_logging();
    let runtime =
        tokio::runtime::Builder::new_multi_thread().enable_all().build().context("cannot start the async runtime")?;
    // Only the `Display` of the error is used: its variants belong to the MCP crate (T-201).
    runtime.block_on(dioramic_mcp::serve_stdio()).map_err(|error| {
        UserError::new(format!("the MCP server stopped: {error}"))
            .hint(format!("run with {LOG_ENV}=debug for details; render, lint and icons work without the server"))
            .into()
    })
}

fn init_logging() {
    let filter = EnvFilter::try_from_env(LOG_ENV).unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER));
    // `try_init` fails only when a subscriber is already set, which is harmless here.
    let _ = tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).with_ansi(false).try_init();
}
