//! The `rmcp` server: tool and prompt routers, resources, `initialize` info and workspace roots.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use dioramic_core::icons::IconRegistry;
use dioramic_packs::{PackStore, RegistryCache, load_project_icons};
use dioramic_render::Engine;
use rmcp::handler::server::router::prompt::PromptRouter;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, ListResourceTemplatesResult, ListResourcesResult,
    PaginatedRequestParams, PromptMessage, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Role,
    ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{
    ErrorData, RoleServer, ServerHandler, prompt, prompt_handler, prompt_router, tool, tool_handler, tool_router,
};

use crate::outcome::ToolError;
use crate::params::{GetGuideParams, RenderDiagramParams, SearchIconsParams, ValidateDiagramParams};
use crate::prompts::{self, AnimateRequestFlowArgs, DiagramFromRepoArgs, RedrawImageArgs};
use crate::resources::{self, ResourceError};
use crate::workspace::Workspace;
use crate::{INSTRUCTIONS, SERVER_NAME, icons_tool, render_tool, validate_tool};

/// Environment variable that turns `structuredContent` off (`0`/`false`): some Claude Code versions dropped
/// the image when both were present (`docs/work/research/clients.md` §1.2).
pub const STRUCTURED_ENV: &str = "DIORAMIC_MCP_STRUCTURED";
/// How long to wait for the client's `roots/list` answer before falling back to the working directory.
const ROOTS_TIMEOUT: Duration = Duration::from_secs(5);

/// Tool descriptions (kept well under the ~2048-char truncation of Claude Code).
pub mod descriptions {
    /// `render_diagram`.
    pub const RENDER_DIAGRAM: &str = "Render an isometric architecture / system design diagram from a DioramaSpec \
(inline `spec` object or `spec_path` to a .diorama.yaml file) and save it as PNG, SVG, animated SVG or GIF. \
Returns a preview image (a 4-frame filmstrip for animated formats) plus a JSON summary: output_path, spec_path, \
width, height, bytes, frames, duration_ms, view (rotation, density, theme, layout_algorithm, routing, grid, \
icon_animation, rotation_scores) and diagnostics. theme, rotation, density, layout_algorithm (flow | hub), routing \
(curved | orthogonal | straight | rails, elbow = rails), grid {size, style, opacity, color} and icon_animation \
override the spec for this render only (theme schematic defaults to hub + rails, edges colored by kind). \
On E_* errors nothing is rendered (isError): read the diagnostics, fix the spec and call again. \
Also fix W_* warnings you can see in the preview (max 3 rounds). Inline specs are saved as \
<slug>.diorama.yaml next to the output so humans and the desktop app can open them. \
Files are written only inside the project (client roots or working directory).";

    /// `validate_diagram`.
    pub const VALIDATE_DIAGRAM: &str = "Check a DioramaSpec for an architecture diagram without rendering \
(milliseconds). Returns errors and lint warnings plus a text summary of the layout: the plan position of every \
zone and node in tiles, so you can reason about space without an image. Same `spec` / `spec_path` input as \
render_diagram.";

    /// `search_icons`.
    pub const SEARCH_ICONS: &str = "Search the diagram icon catalog (servers, databases, caches, queues, load \
balancers, gateways, firewalls, Kubernetes, users, devices, monitoring) by name, alias or category, plus the \
installed provider packs: AWS (aws/lambda, aws/s3), Google Cloud (gcp/bigquery), Azure (azure/functions) and \
Kubernetes (k8s/pod), and project icons (local/<name>). Call it before using an icon you are not sure exists. \
Returns [{id, name, aliases, category, kind, pack}] and the installed packs; `pack` filters (builtin, aws, gcp, \
azure, k8s, local); `preview: true` also returns a contact sheet image. A pack that is not installed is \
installed by the user with `dioramic icons install <pack>`. If nothing fits, use `icon: block` with a color and label.";

    /// `get_guide`.
    pub const GET_GUIDE: &str = "Short markdown guide for writing DioramaSpec architecture diagrams. Topics: \
quickstart (workflow and minimal spec), spec (full format, edge shorthand, diagnostics), styles (themes, colors, \
edge styles), animation (GIF / animated SVG presets), examples (complete specs), icons (catalog). Read `spec` the \
first time you write a diagram.";
}

/// Runtime options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerOptions {
    /// Send `structuredContent` besides the text JSON.
    pub structured_content: bool,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self { structured_content: true }
    }
}

impl ServerOptions {
    /// Options from the environment ([`STRUCTURED_ENV`]).
    pub fn from_env() -> Self {
        let disabled = std::env::var(STRUCTURED_ENV)
            .is_ok_and(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off"));
        Self { structured_content: !disabled }
    }
}

/// The MCP server state: one engine (fonts, caches) for the whole process, and the installed icon packs
/// (re-read only when the pack store changes, so `dioramic icons install` is picked up without a restart).
#[derive(Debug, Clone)]
pub struct DioramicServer {
    engine: Engine,
    options: ServerOptions,
    icon_packs: Arc<RegistryCache>,
    tool_router: ToolRouter<Self>,
    prompt_router: PromptRouter<Self>,
}

impl DioramicServer {
    /// New server around an engine, with options from the environment.
    pub fn new(engine: Engine) -> Self {
        Self::with_options(engine, ServerOptions::from_env())
    }

    /// New server with explicit options.
    pub fn with_options(engine: Engine, options: ServerOptions) -> Self {
        Self {
            engine,
            options,
            icon_packs: Arc::new(RegistryCache::new(PackStore::from_env().ok())),
            tool_router: Self::tool_router(),
            prompt_router: Self::prompt_router(),
        }
    }

    /// Uses `store` for installed icon packs (`None`: built-in icons and project icons only).
    pub fn with_icon_store(mut self, store: Option<PackStore>) -> Self {
        self.icon_packs = Arc::new(RegistryCache::new(store));
        self
    }

    /// Installed packs plus the project icons (`icons/` in `directory`).
    fn icons_for(&self, directory: &Path) -> IconRegistry {
        let (mut registry, problems) = self.icon_packs.load();
        let problems = problems.into_iter().chain(load_project_icons(&mut registry, directory));
        for problem in problems {
            tracing::warn!(path = %problem.path.display(), message = %problem.message, "icon not loaded");
        }
        registry
    }

    /// The engine.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Tool definitions (for tests and docs).
    pub fn tools(&self) -> Vec<rmcp::model::Tool> {
        self.tool_router.list_all()
    }

    /// Client roots when announced and answered in time, else the working directory.
    async fn workspace(&self, context: &RequestContext<RoleServer>) -> Result<Workspace, ToolError> {
        match client_roots(context).await {
            Some(workspace) => Ok(workspace),
            None => Ok(Workspace::current_dir()?),
        }
    }

    fn finish(&self, outcome: Result<CallToolResult, ToolError>) -> CallToolResult {
        outcome.unwrap_or_else(|error| {
            tracing::warn!(%error, "tool call failed");
            error.into_call_result(self.options.structured_content)
        })
    }
}

// `roots` is deprecated by SEP-2577 but it is what Claude Code, Claude Desktop and VS Code speak today.
#[allow(deprecated)]
async fn client_roots(context: &RequestContext<RoleServer>) -> Option<Workspace> {
    context.client_capabilities()?.roots?;
    let listed = tokio::time::timeout(ROOTS_TIMEOUT, context.peer.list_roots()).await.ok()?.ok()?;
    Workspace::from_root_uris(listed.roots.iter().map(|root| root.uri.as_str())).ok()
}

#[tool_router]
impl DioramicServer {
    #[tool(
        name = "render_diagram",
        description = descriptions::RENDER_DIAGRAM,
        annotations(title = "Render diagram", read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false)
    )]
    async fn render_diagram(
        &self,
        Parameters(params): Parameters<RenderDiagramParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = match self.workspace(&context).await {
            Ok(workspace) => {
                let icons = |directory: &Path| self.icons_for(directory);
                render_tool::render_diagram(
                    self.engine.clone(),
                    &workspace,
                    params,
                    &icons,
                    self.options.structured_content,
                )
                .await
            }
            Err(error) => Err(error),
        };
        Ok(self.finish(outcome))
    }

    #[tool(
        name = "validate_diagram",
        description = descriptions::VALIDATE_DIAGRAM,
        annotations(title = "Validate diagram", read_only_hint = true, open_world_hint = false)
    )]
    async fn validate_diagram(
        &self,
        Parameters(params): Parameters<ValidateDiagramParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = match self.workspace(&context).await {
            Ok(workspace) => {
                let icons = |directory: &Path| self.icons_for(directory);
                validate_tool::validate_diagram(
                    self.engine.clone(),
                    &workspace,
                    params,
                    &icons,
                    self.options.structured_content,
                )
                .await
            }
            Err(error) => Err(error),
        };
        Ok(self.finish(outcome))
    }

    #[tool(
        name = "search_icons",
        description = descriptions::SEARCH_ICONS,
        annotations(title = "Search icons", read_only_hint = true, open_world_hint = false)
    )]
    async fn search_icons(
        &self,
        Parameters(params): Parameters<SearchIconsParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = match self.workspace(&context).await {
            Ok(workspace) => {
                let registry = self.icons_for(workspace.base());
                icons_tool::search_icons(params, registry, self.options.structured_content).await
            }
            Err(error) => Err(error),
        };
        Ok(self.finish(outcome))
    }

    #[tool(
        name = "get_guide",
        description = descriptions::GET_GUIDE,
        annotations(title = "Get guide", read_only_hint = true, open_world_hint = false)
    )]
    async fn get_guide(&self, Parameters(params): Parameters<GetGuideParams>) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![ContentBlock::text(params.topic.markdown())]))
    }
}

// The macro emits public `*_prompt_attr` helpers without docs.
#[allow(missing_docs)]
#[prompt_router]
impl DioramicServer {
    /// Prompt `diagram-from-repo`.
    #[prompt(name = "diagram-from-repo", description = prompts::DIAGRAM_FROM_REPO_DESCRIPTION)]
    async fn diagram_from_repo(&self, Parameters(args): Parameters<DiagramFromRepoArgs>) -> Vec<PromptMessage> {
        vec![PromptMessage::new_text(Role::User, prompts::diagram_from_repo(&args))]
    }

    /// Prompt `animate-request-flow`.
    #[prompt(name = "animate-request-flow", description = prompts::ANIMATE_REQUEST_FLOW_DESCRIPTION)]
    async fn animate_request_flow(&self, Parameters(args): Parameters<AnimateRequestFlowArgs>) -> Vec<PromptMessage> {
        vec![PromptMessage::new_text(Role::User, prompts::animate_request_flow(&args))]
    }

    /// Prompt `redraw-image`.
    #[prompt(name = "redraw-image", description = prompts::REDRAW_IMAGE_DESCRIPTION)]
    async fn redraw_image(&self, Parameters(args): Parameters<RedrawImageArgs>) -> Vec<PromptMessage> {
        vec![PromptMessage::new_text(Role::User, prompts::redraw_image(&args))]
    }
}

#[tool_handler(router = self.tool_router)]
#[prompt_handler(router = self.prompt_router)]
impl ServerHandler for DioramicServer {
    fn get_info(&self) -> ServerConfig {
        let capabilities = ServerCapabilities::builder().enable_tools().enable_prompts().enable_resources().build();
        ServerConfig::new(capabilities)
            .with_server_info(Implementation::new(SERVER_NAME, env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        Ok(ListResourcesResult::with_all_items(resources::list()))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        Ok(ListResourceTemplatesResult::with_all_items(resources::templates()))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        match resources::read(&request.uri) {
            Ok(contents) => Ok(ReadResourceResult::new(vec![contents]).into()),
            Err(error @ ResourceError::NotFound(_)) => Err(ErrorData::resource_not_found(error.to_string(), None)),
            Err(error) => Err(ErrorData::internal_error(error.to_string(), None)),
        }
    }
}
