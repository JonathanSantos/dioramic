//! QA (T-301): MCP end to end over every example, focused on `rotation: auto` and the engine summary.
//!
//! The client announces a temporary directory as its only root, so every file the server writes lands
//! there. Unlike `server.rs`, nothing here tolerates an unfinished engine: every example must render.

// `roots` is deprecated by SEP-2577 but is what current clients use (see src/server.rs).
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs, deprecated)]

use std::path::Path;

use dioramic_mcp::{DioramicServer, ServerOptions};
use dioramic_render::Engine;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig, Implementation, ListRootsResult, Root,
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

fn example_names() -> Vec<String> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut names: Vec<String> = std::fs::read_dir(directory)
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().to_str()?.strip_suffix(".diorama.yaml").map(str::to_owned))
        .collect();
    names.sort();
    names
}

fn copy_example(root: &Path, name: &str) -> String {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../examples/{name}.diorama.yaml"));
    let relative = format!("{name}.diorama.yaml");
    std::fs::copy(source, root.join(&relative)).unwrap();
    relative
}

#[tokio::test]
async fn auto_rotation_reports_scores_that_match_the_engine_for_every_example() {
    let root = tempfile::tempdir().unwrap();
    let client = connect(root.path()).await;
    for name in example_names() {
        let spec_path = copy_example(root.path(), &name);
        let result = call(
            &client,
            "render_diagram",
            json!({ "spec_path": spec_path, "format": "svg", "rotation": "auto", "density": "spaced", "preview": "none" }),
        )
        .await;
        assert_eq!(result.is_error, Some(false), "{name}: {}", texts(&result));
        let summary = result.structured_content.clone().unwrap();
        let scores = summary["view"]["rotation_scores"].as_object().unwrap_or_else(|| panic!("{name}: {summary}"));
        assert_eq!(scores.len(), 4, "{name}: {scores:?}");

        let mut request = dioramic_render::RenderRequest::from_text(
            std::fs::read_to_string(root.path().join(format!("{name}.diorama.yaml"))).unwrap(),
        );
        request.format = dioramic_render::OutputFormat::Svg;
        request.rotation = Some(dioramic_spec::RotationSpec::Auto);
        request.density = Some(dioramic_spec::Density::Spaced);
        request.preview = dioramic_render::PreviewMode::None;
        let engine = Engine::shared().render(&request).unwrap().summary;
        let expected = engine.view.rotation_scores.unwrap();
        for (rotation, score) in &expected {
            let reported = scores.get(rotation).and_then(Value::as_f64).unwrap_or_else(|| panic!("{name}: {rotation}"));
            assert!((reported - score).abs() < 1e-6, "{name}: {rotation} mcp {reported} vs engine {score}");
        }
    }
}

#[tokio::test]
async fn bank_branch_auto_rotation_scores_are_not_all_zero_over_mcp() {
    let root = tempfile::tempdir().unwrap();
    let client = connect(root.path()).await;
    let spec_path = copy_example(root.path(), "bank-branch");
    let result = call(
        &client,
        "render_diagram",
        json!({ "spec_path": spec_path, "format": "svg", "rotation": "auto", "density": "spaced", "preview": "none" }),
    )
    .await;
    assert_eq!(result.is_error, Some(false), "{}", texts(&result));
    let summary = result.structured_content.unwrap();
    let scores = summary["view"]["rotation_scores"].as_object().unwrap();
    assert!(scores.values().any(|score| score.as_f64().unwrap_or(0.0) > 0.0), "{scores:?}");
}

#[tokio::test]
async fn validate_accepts_every_example_and_png_render_writes_files() {
    let root = tempfile::tempdir().unwrap();
    let client = connect(root.path()).await;
    for name in example_names() {
        let spec_path = copy_example(root.path(), &name);
        let validated = call(&client, "validate_diagram", json!({ "spec_path": spec_path })).await;
        assert_eq!(validated.is_error, Some(false), "{name}: {}", texts(&validated));
        let rendered = call(&client, "render_diagram", json!({ "spec_path": spec_path })).await;
        assert_eq!(rendered.is_error, Some(false), "{name}: {}", texts(&rendered));
        assert!(has_image(&rendered), "{name}: no preview image");
        assert!(root.path().join(format!("{name}.png")).is_file(), "{name}: PNG not written next to the spec");
    }
}
