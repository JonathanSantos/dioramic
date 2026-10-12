//! Prompt texts (Claude Code shows them as `/mcp__dioramic__<name>`, VS Code as `/dioramic.<name>`).

use schemars::JsonSchema;
use serde::Deserialize;

/// `diagram-from-repo` arguments.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct DiagramFromRepoArgs {
    /// Part of the system to focus on (e.g. "checkout flow", "data pipeline"). Default: the whole runtime.
    #[serde(default)]
    pub focus: Option<String>,
}

/// `animate-request-flow` arguments.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct AnimateRequestFlowArgs {
    /// The request to follow (e.g. "user places an order").
    #[serde(default)]
    pub request: Option<String>,
    /// Existing `.diorama.yaml` to animate. Default: draw the architecture first.
    #[serde(default)]
    pub spec_path: Option<String>,
}

/// `redraw-image` arguments.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct RedrawImageArgs {
    /// Extra instructions (theme, what to drop or rename).
    #[serde(default)]
    pub notes: Option<String>,
}

/// Prompt descriptions.
pub const DIAGRAM_FROM_REPO_DESCRIPTION: &str =
    "Analyze this repository and draw its runtime architecture as an isometric diagram.";
/// See [`DIAGRAM_FROM_REPO_DESCRIPTION`].
pub const ANIMATE_REQUEST_FLOW_DESCRIPTION: &str =
    "Animate the path of one request through the architecture, step by step (GIF with captions).";
/// See [`DIAGRAM_FROM_REPO_DESCRIPTION`].
pub const REDRAW_IMAGE_DESCRIPTION: &str =
    "Recreate the attached architecture diagram image as a DioramaSpec and render it.";

/// User message of `diagram-from-repo`.
pub fn diagram_from_repo(args: &DiagramFromRepoArgs) -> String {
    let focus = args.focus.as_deref().map_or_else(
        || "the whole runtime architecture".to_owned(),
        |focus| format!("the runtime architecture of: {focus}"),
    );
    format!(
        "Analyze this repository and draw {focus} as an isometric Dioramic diagram.\n\
         1. Find the runtime components: services, entry points, data stores, queues, external APIs, \
            clients and infrastructure (look at docker-compose, k8s manifests, terraform, config, main modules).\n\
         2. Call get_guide(\"spec\") if you have not yet. Write a minimal spec at diagrams/architecture.diorama.yaml: \
            one node per component (short labels), edges for real calls/data flows with short labels \
            (protocol or purpose), zones only for real boundaries (network, cluster, tier). No coordinates or colors.\n\
         3. Call render_diagram with spec_path, look at the preview and diagnostics, fix and re-render (max 3 rounds).\n\
         4. Reply with the image path and a 3-5 line summary of what the diagram shows."
    )
}

/// User message of `animate-request-flow`.
pub fn animate_request_flow(args: &AnimateRequestFlowArgs) -> String {
    let request = args.request.as_deref().unwrap_or("the most important user request");
    let start = args.spec_path.as_deref().map_or_else(
        || "First draw the architecture (see the diagram-from-repo workflow) as a .diorama.yaml file.".to_owned(),
        |path| format!("Start from the existing spec {path}."),
    );
    format!(
        "Animate how {request} flows through the system, step by step.\n\
         {start}\n\
         Call get_guide(\"animation\"). Add `animation: {{ preset: story, steps: [...] }}` with one step per hop \
         (`path: [a, b, c]` or `edges: [a->b]`) and a short caption each (\"1. ...\", \"2. ...\").\n\
         Render with render_diagram(spec_path, format: gif). Check the filmstrip preview: every step visible, \
         captions readable. Fix and re-render (max 3 rounds), then reply with the GIF path."
    )
}

/// User message of `redraw-image`.
pub fn redraw_image(args: &RedrawImageArgs) -> String {
    let notes = args.notes.as_deref().map(|notes| format!("\nAlso: {notes}")).unwrap_or_default();
    format!(
        "Recreate the attached diagram image as a DioramaSpec.\n\
         1. List every component, its type (pick icons with search_icons), its group/boundary and every connection \
            with its label and style (solid, dashed, color meaning).\n\
         2. Write the spec: zones for the groups, nodes with the original labels, edges in shorthand \
            (\"a -> b: label @style\"), named styles for edge colors. No coordinates at first.\n\
         3. Call render_diagram, compare the preview with the original and fix differences \
            (density, rotation: auto, zone arrange) in at most 3 rounds.\n\
         4. Reply with the output and spec paths.{notes}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_woven_into_the_messages() {
        let text = diagram_from_repo(&DiagramFromRepoArgs { focus: Some("payments".to_owned()) });
        assert!(text.contains("of: payments"));
        let text = animate_request_flow(&AnimateRequestFlowArgs {
            request: Some("checkout".to_owned()),
            spec_path: Some("diagrams/shop.diorama.yaml".to_owned()),
        });
        assert!(text.contains("how checkout flows") && text.contains("diagrams/shop.diorama.yaml"));
        assert!(redraw_image(&RedrawImageArgs { notes: Some("use midnight".to_owned()) }).ends_with("use midnight"));
    }

    #[test]
    fn defaults_read_well() {
        assert!(diagram_from_repo(&DiagramFromRepoArgs::default()).contains("whole runtime architecture"));
        assert!(animate_request_flow(&AnimateRequestFlowArgs::default()).contains("First draw the architecture"));
    }
}
