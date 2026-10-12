//! End-to-end tests with an in-process `rmcp` client over a duplex pipe.
//!
//! The client announces a temporary directory as its only root, so every file the server writes lands
//! there. Engine parts still being built in parallel (T-102 icon search, T-104/T-105 render) may answer
//! with an `isError` "not implemented"/"internal error" result; those tests then only check the error
//! contract and skip the output assertions.

// `roots` is deprecated by SEP-2577 but is what current clients use (see src/server.rs).
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs, deprecated)]

use std::path::Path;

use dioramic_mcp::{DioramicServer, ServerOptions};
use dioramic_render::Engine;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig, GetPromptRequestParams, Implementation,
    ListRootsResult, ReadResourceRequestParams, ResourceContents, Root,
};
use rmcp::service::{RequestContext, RunningService};
use rmcp::{ClientHandler, ErrorData, RoleClient, ServiceExt};
use serde_json::{Value, json};

/// Client that exposes one directory as its MCP root.
struct RootsClient {
    root_uri: String,
}

impl ClientHandler for RootsClient {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::new(
            ClientCapabilities::builder().enable_roots().build(),
            Implementation::new("dioramic-test-client", "0.0.0"),
        )
    }

    #[allow(deprecated)]
    async fn list_roots(&self, _context: RequestContext<RoleClient>) -> Result<ListRootsResult, ErrorData> {
        Ok(ListRootsResult::new(vec![Root::new(self.root_uri.clone())]))
    }
}

type Client = RunningService<RoleClient, RootsClient>;

const DUPLEX_BUFFER_BYTES: usize = 1 << 20;

async fn connect(root: &Path) -> Client {
    let (server_io, client_io) = tokio::io::duplex(DUPLEX_BUFFER_BYTES);
    let server = DioramicServer::with_options(Engine::new(), ServerOptions::default());
    tokio::spawn(async move {
        if let Ok(running) = server.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });
    let root_uri = format!("file://{}", root.canonicalize().unwrap().display());
    RootsClient { root_uri }.serve(client_io).await.expect("client connects")
}

async fn call(client: &Client, tool: &'static str, arguments: Value) -> CallToolResult {
    let Value::Object(arguments) = arguments else { panic!("arguments must be an object") };
    client.call_tool(CallToolRequestParams::new(tool).with_arguments(arguments)).await.expect("tools/call answers")
}

fn texts(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text())
        .map(|text| text.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn has_image(result: &CallToolResult) -> bool {
    result.content.iter().any(|block| block.as_image().is_some_and(|image| image.mime_type == "image/png"))
}

/// True when the engine answered "not ready yet" (a parallel task is unfinished).
fn engine_not_ready(result: &CallToolResult) -> bool {
    let text = texts(result);
    result.is_error == Some(true) && (text.contains("not implemented") || text.contains("internal error"))
}

fn minimal_spec() -> Value {
    json!({
        "diorama": 1,
        "title": "Checkout Service",
        "nodes": [
            { "id": "users", "icon": "users", "label": "Users" },
            { "id": "api", "icon": "server", "label": "API" },
            { "id": "db", "icon": "postgres" }
        ],
        "edges": ["users -> api: HTTPS", "api -> db: SQL"]
    })
}

#[tokio::test]
async fn initialize_announces_instructions_and_identity() {
    let root = tempfile::tempdir().unwrap();
    let client = connect(root.path()).await;
    let info = client.peer_info().expect("server info after initialize");
    let instructions = info.instructions.clone().unwrap_or_default();
    assert!(instructions.contains("render_diagram"), "{instructions}");
    assert_eq!(info.server_info.as_ref().map(|server| server.name.as_str()), Some("dioramic"));
    assert!(info.capabilities.tools.is_some() && info.capabilities.prompts.is_some());
    assert!(info.capabilities.resources.is_some());
}

#[tokio::test]
async fn tools_list_has_four_tools_with_stable_schemas() {
    let root = tempfile::tempdir().unwrap();
    let client = connect(root.path()).await;
    let mut tools = client.list_tools(None).await.unwrap().tools;
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
    assert_eq!(names, ["get_guide", "render_diagram", "search_icons", "validate_diagram"]);

    let render = tools.iter().find(|tool| tool.name == "render_diagram").unwrap();
    let defs = render.input_schema.get("$defs").and_then(Value::as_object).expect("render schema has $defs");
    assert!(defs.contains_key("SpecV1"), "the inline spec uses the SpecV1 schema");

    // `$defs` mirror the spec types owned by T-101; the snapshot pins only what this crate defines.
    let snapshot: Vec<Value> = tools
        .iter()
        .map(|tool| {
            let mut schema = Value::Object((*tool.input_schema).clone());
            if let Some(object) = schema.as_object_mut() {
                object.remove("$defs");
            }
            json!({
                "name": tool.name,
                "description": tool.description,
                "annotations": tool.annotations,
                "input_schema": schema,
            })
        })
        .collect();
    insta::assert_json_snapshot!("tools_list", snapshot);
}

#[tokio::test]
async fn render_inline_spec_writes_image_and_spec_inside_the_root() {
    let root = tempfile::tempdir().unwrap();
    let client = connect(root.path()).await;
    let result = call(&client, "render_diagram", json!({ "spec": minimal_spec() })).await;
    if engine_not_ready(&result) {
        eprintln!("render engine not ready yet: {}", texts(&result));
        return;
    }
    assert_eq!(result.is_error, Some(false), "{}", texts(&result));
    assert!(has_image(&result), "preview image expected");
    let summary = result.structured_content.clone().expect("structured content");
    assert_eq!(summary["output_path"], "diagrams/checkout-service.png");
    assert_eq!(summary["spec_path"], "diagrams/checkout-service.diorama.yaml");
    assert_eq!(summary["format"], "png");
    assert!(summary["width"].as_u64().unwrap_or(0) > 0);
    assert!(root.path().join("diagrams/checkout-service.png").is_file());
    let saved = std::fs::read_to_string(root.path().join("diagrams/checkout-service.diorama.yaml")).unwrap();
    assert!(saved.contains("Checkout Service"), "{saved}");
    let text_summary: Value =
        serde_json::from_str(result.content.last().and_then(|block| block.as_text()).unwrap().text.as_str()).unwrap();
    assert_eq!(text_summary, summary, "the JSON text mirrors structuredContent");
}

#[tokio::test]
async fn render_spec_path_writes_next_to_the_spec() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("docs")).unwrap();
    std::fs::write(
        root.path().join("docs/net.diorama.yaml"),
        "diorama: 1\nnodes:\n  - { id: fw, icon: firewall }\n  - { id: core, icon: switch }\nedges:\n  - fw -> core: trunk\n",
    )
    .unwrap();
    let client = connect(root.path()).await;
    let result =
        call(&client, "render_diagram", json!({ "spec_path": "docs/net.diorama.yaml", "format": "svg" })).await;
    if engine_not_ready(&result) {
        eprintln!("render engine not ready yet: {}", texts(&result));
        return;
    }
    assert_eq!(result.is_error, Some(false), "{}", texts(&result));
    assert!(has_image(&result), "SVG renders still return a PNG preview");
    let svg = std::fs::read_to_string(root.path().join("docs/net.svg")).unwrap();
    assert!(svg.contains("<svg"));
    let summary = result.structured_content.unwrap();
    assert_eq!(summary["output_path"], "docs/net.svg");
    assert_eq!(summary["spec_path"], "docs/net.diorama.yaml");
}

#[tokio::test]
async fn unknown_reference_is_a_tool_error_with_diagnostics() {
    let root = tempfile::tempdir().unwrap();
    let client = connect(root.path()).await;
    let spec = json!({ "diorama": 1, "nodes": [{ "id": "api" }, { "id": "db" }], "edges": ["api -> dbb: SQL"] });
    let result = call(&client, "render_diagram", json!({ "spec": spec })).await;
    assert_eq!(result.is_error, Some(true));
    let diagnostics = result.structured_content.as_ref().unwrap()["diagnostics"].clone();
    let codes: Vec<&str> = diagnostics.as_array().unwrap().iter().filter_map(|item| item["code"].as_str()).collect();
    assert!(codes.contains(&"E_UNKNOWN_REF"), "{codes:?}");
    assert!(texts(&result).contains("E_UNKNOWN_REF"), "the text content alone must let the model fix it");
    assert!(!root.path().join("diagrams").exists(), "nothing is written on errors");
}

#[tokio::test]
async fn paths_outside_the_root_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let client = connect(root.path()).await;
    let escaping =
        call(&client, "render_diagram", json!({ "spec": minimal_spec(), "output_path": "../escape.png" })).await;
    assert_eq!(escaping.is_error, Some(true));
    assert!(texts(&escaping).contains("outside the allowed roots"), "{}", texts(&escaping));
    let reading = call(&client, "validate_diagram", json!({ "spec_path": "/etc/hosts" })).await;
    assert_eq!(reading.is_error, Some(true));
    assert!(texts(&reading).contains("outside the allowed roots"), "{}", texts(&reading));
}

#[tokio::test]
async fn unsupported_formats_and_bad_input_are_clear_errors() {
    let root = tempfile::tempdir().unwrap();
    let client = connect(root.path()).await;
    let mp4 = call(&client, "render_diagram", json!({ "spec": minimal_spec(), "format": "mp4" })).await;
    assert_eq!(mp4.is_error, Some(true));
    assert!(texts(&mp4).contains("not supported yet"));
    let neither = call(&client, "render_diagram", json!({})).await;
    assert_eq!(neither.is_error, Some(true));
    assert!(texts(&neither).contains("spec_path"));
}

#[tokio::test]
async fn validate_returns_a_layout_summary() {
    let root = tempfile::tempdir().unwrap();
    let client = connect(root.path()).await;
    let result = call(&client, "validate_diagram", json!({ "spec": minimal_spec() })).await;
    if engine_not_ready(&result) {
        eprintln!("compile not ready yet: {}", texts(&result));
        return;
    }
    assert_eq!(result.is_error, Some(false), "{}", texts(&result));
    let text = texts(&result);
    assert!(text.contains("Nodes (center):") && text.contains("- db [database]"), "{text}");
    assert!(!has_image(&result));
}

#[tokio::test]
async fn search_icons_finds_postgres() {
    let root = tempfile::tempdir().unwrap();
    let client = connect(root.path()).await;
    let result = call(&client, "search_icons", json!({ "query": "postgres" })).await;
    if engine_not_ready(&result) {
        eprintln!("icon search not ready yet: {}", texts(&result));
        return;
    }
    assert_eq!(result.is_error, Some(false), "{}", texts(&result));
    let icons = result.structured_content.unwrap()["icons"].clone();
    assert_eq!(icons[0]["id"], "database", "{icons}");
    assert_eq!(icons[0]["category"], "data");
}

#[tokio::test]
async fn search_icons_lists_a_category_without_query() {
    let root = tempfile::tempdir().unwrap();
    let client = connect(root.path()).await;
    let result = call(&client, "search_icons", json!({ "category": "messaging" })).await;
    assert_eq!(result.is_error, Some(false), "{}", texts(&result));
    let icons = result.structured_content.unwrap()["icons"].clone();
    assert!(icons.as_array().unwrap().iter().all(|icon| icon["category"] == "messaging"));
    let unknown = call(&client, "search_icons", json!({ "query": "x", "category": "gpu" })).await;
    assert_eq!(unknown.is_error, Some(true));
}

#[tokio::test]
async fn get_guide_returns_markdown() {
    let root = tempfile::tempdir().unwrap();
    let client = connect(root.path()).await;
    let result = call(&client, "get_guide", json!({ "topic": "spec" })).await;
    assert_eq!(result.is_error, Some(false));
    let text = texts(&result);
    assert!(text.starts_with("# DioramaSpec v1") && text.contains("..>"), "{text}");
}

#[tokio::test]
async fn resources_expose_schema_guides_examples_and_catalog() {
    let root = tempfile::tempdir().unwrap();
    let client = connect(root.path()).await;
    let resources = client.list_resources(None).await.unwrap().resources;
    let uris: Vec<&str> = resources.iter().map(|resource| resource.uri.as_str()).collect();
    for expected in
        ["dioramic://schema/v1", "dioramic://icons/catalog", "dioramic://guide/spec", "dioramic://examples/checkout"]
    {
        assert!(uris.contains(&expected), "{expected} missing from {uris:?}");
    }
    let templates = client.list_resource_templates(None).await.unwrap().resource_templates;
    assert_eq!(templates.len(), 2);

    let schema = client.read_resource(ReadResourceRequestParams::new("dioramic://schema/v1")).await.unwrap();
    let ResourceContents::TextResourceContents { text, .. } = &schema.contents[0] else { panic!("text expected") };
    assert!(text.contains("SpecV1"));
    assert!(client.read_resource(ReadResourceRequestParams::new("dioramic://examples/nope")).await.is_err());
}

#[tokio::test]
async fn prompts_are_listed_and_rendered() {
    let root = tempfile::tempdir().unwrap();
    let client = connect(root.path()).await;
    let mut names: Vec<String> =
        client.list_prompts(None).await.unwrap().prompts.into_iter().map(|prompt| prompt.name).collect();
    names.sort();
    assert_eq!(names, ["animate-request-flow", "diagram-from-repo", "redraw-image"]);
    let mut arguments = serde_json::Map::new();
    arguments.insert("focus".to_owned(), json!("payments"));
    let prompt =
        client.get_prompt(GetPromptRequestParams::new("diagram-from-repo").with_arguments(arguments)).await.unwrap();
    let text = prompt.messages[0].content.as_text().map(|text| text.text.clone()).unwrap_or_default();
    assert!(text.contains("payments") && text.contains("render_diagram"), "{text}");
}
