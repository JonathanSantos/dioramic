//! `render_diagram`: spec → engine → files on disk → preview image + JSON summary.

use std::path::{Path, PathBuf};

use dioramic_core::icons::IconRegistry;
use dioramic_render::{
    AnimationRequest, Engine, OutputFormat, PreviewKind, PreviewMode, RenderRequest, RenderResult, RenderSummary,
    SpecSource,
};
use rmcp::model::CallToolResult;
use serde::Serialize;

use crate::outcome::{ToolError, png_block, run_blocking, success};
use crate::params::{AnimationParams, PreviewParam, RenderDiagramParams};
use crate::source::{self, LoadedSpec, SPEC_EXTENSION};
use crate::workspace::Workspace;

/// Accepted `scale` range (`docs/05-mcp.md`).
const SCALE_RANGE: std::ops::RangeInclusive<f64> = 1.0..=4.0;
/// Directory of outputs of inline specs, under the workspace base.
pub const DEFAULT_OUTPUT_DIR: &str = "diagrams";

/// Files a render writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputPlan {
    /// Rendered image.
    pub output: PathBuf,
    /// Saved spec (inline spec with `save_spec`).
    pub spec: Option<PathBuf>,
}

/// JSON summary of a render (`docs/05-mcp.md`).
#[derive(Debug, Clone, Serialize)]
struct RenderReport {
    output_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    spec_path: Option<String>,
    #[serde(flatten)]
    summary: RenderSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    preview: Option<PreviewKind>,
}

/// Runs the tool. `structured` toggles `structuredContent`.
pub async fn render_diagram(
    engine: Engine,
    workspace: &Workspace,
    params: RenderDiagramParams,
    icons: &(dyn Fn(&Path) -> IconRegistry + Sync),
    structured: bool,
) -> Result<CallToolResult, ToolError> {
    let format = params.format.output_format().map_err(ToolError::UnsupportedFormat)?;
    let loaded = source::load(&params.input, workspace)?;
    let mut request = engine_request(&params, &loaded, format)?;
    request.icons = icons(&source::spec_directory(&loaded, workspace));
    let plan = plan_outputs(workspace, &params, &loaded, format)?;
    let spec_yaml = plan.spec.as_ref().map(|_| loaded.to_yaml()).transpose()?;
    let written_plan = plan.clone();
    let result = run_blocking(move || {
        let result = engine.render(&request)?;
        write_file(&written_plan.output, &result.output)?;
        if let (Some(path), Some(yaml)) = (&written_plan.spec, &spec_yaml) {
            write_file(path, yaml.as_bytes())?;
        }
        Ok(result)
    })
    .await?;
    Ok(build_result(workspace, &plan, &loaded, result, structured))
}

fn engine_request(
    params: &RenderDiagramParams,
    loaded: &LoadedSpec,
    format: OutputFormat,
) -> Result<RenderRequest, ToolError> {
    let mut request = RenderRequest::new(SpecSource::Text(loaded.text.clone()));
    request.format = format;
    request.theme = params.theme;
    request.rotation = params.rotation;
    request.density = params.density;
    request.routing = params.routing;
    request.layout_algorithm = params.layout_algorithm;
    request.grid = params.grid.clone().map(Into::into).unwrap_or_default();
    request.icon_animation = params.icon_animation;
    if let Some(scale) = params.scale {
        if !SCALE_RANGE.contains(&scale) {
            return Err(ToolError::BadRequest(format!("scale must be between 1 and 4, got {scale}")));
        }
        request.scale = scale;
    }
    request.animation = params.animation.map(animation_request).unwrap_or_default();
    request.preview = match params.preview {
        PreviewParam::Image => PreviewMode::Image,
        PreviewParam::None => PreviewMode::None,
    };
    Ok(request)
}

fn animation_request(params: AnimationParams) -> AnimationRequest {
    AnimationRequest { preset: params.preset, duration_s: params.duration, fps: params.fps }
}

/// Output next to `spec_path`, at `output_path`, or in `./diagrams/<slug>.<ext>`; the saved spec
/// (inline only) goes next to the output with the same stem.
pub fn plan_outputs(
    workspace: &Workspace,
    params: &RenderDiagramParams,
    loaded: &LoadedSpec,
    format: OutputFormat,
) -> Result<OutputPlan, ToolError> {
    let extension = format.extension();
    let output = match (&params.output_path, &loaded.path) {
        (Some(requested), _) => {
            let resolved = workspace.resolve(Path::new(requested))?;
            if resolved.is_dir() { resolved.join(format!("{}.{extension}", loaded.slug())) } else { resolved }
        }
        (None, Some(spec_path)) => spec_path.with_file_name(format!("{}.{extension}", source::spec_stem(spec_path))),
        (None, None) => {
            workspace.resolve(&Path::new(DEFAULT_OUTPUT_DIR).join(format!("{}.{extension}", loaded.slug())))?
        }
    };
    let save_spec = loaded.is_inline() && params.save_spec.unwrap_or(true);
    let spec = save_spec
        .then(|| workspace.resolve(&output.with_file_name(format!("{}.{SPEC_EXTENSION}", source::spec_stem(&output)))))
        .transpose()?;
    Ok(OutputPlan { output, spec })
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), ToolError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| ToolError::io("create directory", parent, error))?;
    }
    std::fs::write(path, bytes).map_err(|error| ToolError::io("write", path, error))
}

fn build_result(
    workspace: &Workspace,
    plan: &OutputPlan,
    loaded: &LoadedSpec,
    result: RenderResult,
    structured: bool,
) -> CallToolResult {
    let spec_path = plan.spec.as_deref().or(loaded.path.as_deref()).map(|path| workspace.display_path(path));
    let report = RenderReport {
        output_path: workspace.display_path(&plan.output),
        spec_path,
        summary: result.summary,
        preview: result.preview.as_ref().map(|preview| preview.kind),
    };
    let blocks = result.preview.map(|preview| vec![png_block(&preview.png)]).unwrap_or_default();
    let payload = serde_json::to_value(&report).unwrap_or_default();
    tracing::info!(output = %report.output_path, "rendered diagram");
    success(blocks, payload, structured)
}

#[cfg(test)]
mod tests {
    use dioramic_render::{SourceText, TextFormat};

    use super::*;

    fn workspace() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::from_dirs([dir.path().to_path_buf()]).unwrap();
        (dir, workspace)
    }

    fn inline_spec() -> LoadedSpec {
        let text = r#"{"diorama":1,"title":"Shop API","nodes":[{"id":"a"}]}"#.to_owned();
        LoadedSpec { text: SourceText { text, format: Some(TextFormat::Json) }, path: None }
    }

    #[test]
    fn inline_specs_go_to_the_diagrams_dir_with_the_spec_beside() {
        let (_dir, workspace) = workspace();
        let plan =
            plan_outputs(&workspace, &RenderDiagramParams::default(), &inline_spec(), OutputFormat::Gif).unwrap();
        assert_eq!(plan.output, workspace.base().join("diagrams/shop-api.gif"));
        assert_eq!(plan.spec, Some(workspace.base().join("diagrams/shop-api.diorama.yaml")));
    }

    #[test]
    fn save_spec_false_skips_the_spec() {
        let (_dir, workspace) = workspace();
        let params = RenderDiagramParams { save_spec: Some(false), ..RenderDiagramParams::default() };
        let plan = plan_outputs(&workspace, &params, &inline_spec(), OutputFormat::Png).unwrap();
        assert_eq!(plan.spec, None);
    }

    #[test]
    fn spec_files_render_next_to_themselves() {
        let (_dir, workspace) = workspace();
        let spec_path = workspace.base().join("docs/net.diorama.yaml");
        let loaded = LoadedSpec { text: inline_spec().text, path: Some(spec_path) };
        let plan = plan_outputs(&workspace, &RenderDiagramParams::default(), &loaded, OutputFormat::Svg).unwrap();
        assert_eq!(plan.output, workspace.base().join("docs/net.svg"));
        assert_eq!(plan.spec, None);
    }

    #[test]
    fn explicit_outputs_are_checked_and_directories_get_a_file_name() {
        let (dir, workspace) = workspace();
        std::fs::create_dir(dir.path().join("out")).unwrap();
        let into_dir = RenderDiagramParams { output_path: Some("out".to_owned()), ..RenderDiagramParams::default() };
        let plan = plan_outputs(&workspace, &into_dir, &inline_spec(), OutputFormat::Png).unwrap();
        assert_eq!(plan.output, workspace.base().join("out/shop-api.png"));
        assert_eq!(plan.spec, Some(workspace.base().join("out/shop-api.diorama.yaml")));
        let outside =
            RenderDiagramParams { output_path: Some("../x.png".to_owned()), ..RenderDiagramParams::default() };
        assert!(matches!(
            plan_outputs(&workspace, &outside, &inline_spec(), OutputFormat::Png),
            Err(ToolError::Path(_))
        ));
    }

    #[test]
    fn view_overrides_reach_the_engine_request() {
        let params = RenderDiagramParams {
            routing: Some(dioramic_spec::Routing::Rails),
            grid: Some(crate::params::GridParams { size: Some(0.5), ..Default::default() }),
            icon_animation: Some(false),
            layout_algorithm: Some(dioramic_spec::LayoutAlgorithm::Hub),
            ..RenderDiagramParams::default()
        };
        let request = engine_request(&params, &inline_spec(), OutputFormat::Gif).unwrap();
        assert_eq!(request.layout_algorithm, Some(dioramic_spec::LayoutAlgorithm::Hub));
        assert_eq!(request.routing, Some(dioramic_spec::Routing::Rails));
        assert_eq!(request.grid.size, Some(0.5));
        assert_eq!(request.icon_animation, Some(false));
    }

    #[test]
    fn scale_out_of_range_is_refused() {
        let params = RenderDiagramParams { scale: Some(9.0), ..RenderDiagramParams::default() };
        assert!(matches!(engine_request(&params, &inline_spec(), OutputFormat::Png), Err(ToolError::BadRequest(_))));
    }
}
