//! Live preview: editor text + view → SVG and diagnostics.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::time::Instant;

use dioramic_desktop_ipc::{DiagnosticView, PreviewRequest, PreviewResponse, PreviewStatus, Severity};
use dioramic_render::{Engine, RenderError, RenderResult};

use crate::diagnostics::{SpecOutline, syntax_diagnostics};
use crate::icons::{IconLibrary, problem_views};
use crate::view::{preview_request, resolved_view};

/// Renders the preview with the icon packs of `icons`. Never fails: every problem becomes a status the
/// UI can show (icon files that could not be loaded are warnings next to the spec diagnostics).
pub fn render_preview(engine: &Engine, icons: &IconLibrary, request: &PreviewRequest) -> PreviewResponse {
    let started = Instant::now();
    let loaded = icons.icons_for(request.spec_path.as_deref().map(Path::new));
    let mut render_request = preview_request(&request.text, &request.view, request.motion);
    render_request.icons = loaded.registry;
    let outcome = run_guarded(|| engine.render(&render_request));
    let outline = SpecOutline::of(&request.text);
    let mut response = match outcome {
        Ok(Ok(result)) => ready(result, &outline),
        Ok(Err(RenderError::Invalid(diagnostics))) => PreviewResponse {
            status: PreviewStatus::Invalid,
            svg: None,
            diagnostics: outline.views(&diagnostics),
            view: None,
            notes: Vec::new(),
            elapsed_ms: 0,
        },
        // Only a stage that does not exist yet is "unavailable"; any other failure is a real render
        // error the user must see as such (QA-T-203 P3-1).
        Ok(Err(RenderError::NotImplemented(stage))) => {
            unavailable(format!("not implemented yet ({stage})"), &request.text)
        }
        Ok(Err(error)) => render_failed(render_error_diagnostic(&error)),
        Err(panic) => render_failed(failure_diagnostic(RENDER_FAILED_CODE, format!("the renderer crashed: {panic}"))),
    };
    response.diagnostics.extend(problem_views(&loaded.problems));
    response.notes = outline.into_notes();
    response.elapsed_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
    response
}

fn ready(result: RenderResult, outline: &SpecOutline) -> PreviewResponse {
    match String::from_utf8(result.output) {
        Ok(svg) => PreviewResponse {
            status: PreviewStatus::Ready,
            svg: Some(svg),
            diagnostics: outline.views(&result.summary.diagnostics),
            view: Some(resolved_view(&result.summary.view)),
            notes: Vec::new(),
            elapsed_ms: 0,
        },
        Err(_) => PreviewResponse {
            status: PreviewStatus::EngineUnavailable { reason: "the engine returned an SVG that is not UTF-8".into() },
            svg: None,
            diagnostics: outline.views(&result.summary.diagnostics),
            view: None,
            notes: Vec::new(),
            elapsed_ms: 0,
        },
    }
}

/// The engine cannot render yet: keep the editor useful with a syntax check.
fn unavailable(reason: String, text: &str) -> PreviewResponse {
    PreviewResponse {
        status: PreviewStatus::EngineUnavailable { reason },
        svg: None,
        diagnostics: syntax_diagnostics(text),
        view: None,
        notes: Vec::new(),
        elapsed_ms: 0,
    }
}

/// Code of the diagnostic reporting a render failure that is not a spec error (desktop only).
pub const RENDER_FAILED_CODE: &str = "E_RENDER";

/// Code of the spec diagnostics for a hard limit (same as `dioramic_spec`).
const LIMIT_CODE: &str = "E_LIMIT";

/// The engine ran and failed: report it as an error the user can read in the diagnostics panel.
/// `PreviewStatus` has no dedicated variant yet (changing it would break the UI crate), so the
/// failure travels as `Invalid` with one error diagnostic carrying the engine's reason.
fn render_failed(diagnostic: DiagnosticView) -> PreviewResponse {
    PreviewResponse {
        status: PreviewStatus::Invalid,
        svg: None,
        diagnostics: vec![diagnostic],
        view: None,
        notes: Vec::new(),
        elapsed_ms: 0,
    }
}

fn render_error_diagnostic(error: &RenderError) -> DiagnosticView {
    match error {
        RenderError::Limit { .. } => failure_diagnostic(LIMIT_CODE, error.to_string()),
        _ => failure_diagnostic(RENDER_FAILED_CODE, format!("the diagram could not be rendered: {error}")),
    }
}

fn failure_diagnostic(code: &str, message: String) -> DiagnosticView {
    DiagnosticView {
        code: code.to_owned(),
        severity: Severity::Error,
        path: String::new(),
        message,
        hint: Some("Simplify the spec or report the problem if it persists.".to_owned()),
        line: None,
        column: None,
        target_id: None,
    }
}

/// Runs engine code, turning a panic (a stage still under construction, a bug) into an error
/// message instead of taking the whole app down.
pub fn run_guarded<T>(work: impl FnOnce() -> T) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(work)).map_err(|payload| {
        payload
            .downcast_ref::<&str>()
            .map(|message| (*message).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icons::test_packs::{ORANGE_SQUARE, install_aws};
    use dioramic_desktop_ipc::ViewState;
    use dioramic_packs::PackStore;

    const AWS_SPEC: &str = "diorama: 1\nnodes:\n  - { id: fn, icon: aws/lambda }\n  - { id: pay, icon: local/payments }\nedges:\n  - fn -> pay\n";

    fn request(text: &str, spec_path: Option<&Path>) -> PreviewRequest {
        PreviewRequest {
            text: text.into(),
            view: ViewState::default(),
            spec_path: spec_path.map(|path| path.display().to_string()),
            motion: false,
        }
    }

    fn codes(response: &PreviewResponse) -> Vec<&str> {
        response.diagnostics.iter().map(|diagnostic| diagnostic.code.as_str()).collect()
    }

    const SPEC: &str = "diorama: 1\nnodes:\n  - { id: api, note: Stateless }\n";

    #[test]
    fn preview_never_fails_and_reports_notes() {
        let response = render_preview(Engine::shared(), &IconLibrary::default(), &request(SPEC, None));
        match &response.status {
            PreviewStatus::Ready => assert!(response.svg.as_deref().is_some_and(|svg| svg.contains("<svg"))),
            PreviewStatus::Invalid => assert!(!response.diagnostics.is_empty()),
            PreviewStatus::EngineUnavailable { reason } => assert!(!reason.is_empty()),
        }
        assert_eq!(response.notes.len(), 1);
    }

    #[test]
    fn broken_yaml_is_reported_whatever_the_engine_state() {
        let response =
            render_preview(Engine::shared(), &IconLibrary::default(), &request("diorama: 1\nnodes: [\n", None));
        assert!(response.svg.is_none());
        assert!(response.diagnostics.iter().any(|diagnostic| diagnostic.code == "E_PARSE"));
    }

    #[test]
    fn missing_pack_is_a_warning_with_the_install_command() {
        let lambda_only = "diorama: 1\nnodes:\n  - { id: fn, icon: aws/lambda }\n";
        let response = render_preview(Engine::shared(), &IconLibrary::default(), &request(lambda_only, None));
        assert_eq!(response.status, PreviewStatus::Ready, "{:?}", response.diagnostics);
        let missing: Vec<_> =
            response.diagnostics.iter().filter(|diagnostic| diagnostic.code == "W_ICON_PACK_MISSING").collect();
        assert_eq!(missing.len(), 1, "{:?}", response.diagnostics);
        assert_eq!(missing[0].severity, Severity::Warning);
        assert_eq!(missing[0].target_id.as_deref(), Some("fn"));
        assert!(missing[0].hint.as_deref().is_some_and(|hint| hint.contains("`dioramic icons install aws`")));
    }

    #[test]
    fn installed_and_project_icons_reach_the_preview() -> Result<(), Box<dyn std::error::Error>> {
        let store = tempfile::tempdir()?;
        install_aws(store.path())?;
        let project = tempfile::tempdir()?;
        std::fs::create_dir_all(project.path().join("icons"))?;
        std::fs::write(project.path().join("icons/payments.svg"), ORANGE_SQUARE.replace("ED7100", "7AA116"))?;
        let icons = IconLibrary::with_store(Some(PackStore::at(store.path())));
        let spec_path = project.path().join("app.diorama.yaml");

        let response = render_preview(Engine::shared(), &icons, &request(AWS_SPEC, Some(&spec_path)));
        assert_eq!(response.status, PreviewStatus::Ready, "{:?}", response.diagnostics);
        assert!(response.diagnostics.is_empty(), "{:?}", response.diagnostics);
        let svg = response.svg.unwrap_or_default();
        assert!(svg.to_ascii_uppercase().contains("ED7100") && svg.to_ascii_uppercase().contains("7AA116"));
        Ok(())
    }

    #[test]
    fn unreadable_icon_files_are_reported_as_warnings() -> Result<(), Box<dyn std::error::Error>> {
        let project = tempfile::tempdir()?;
        std::fs::create_dir_all(project.path().join("icons"))?;
        std::fs::write(project.path().join("icons/broken.svg"), "<nope")?;
        let spec_path = project.path().join("app.diorama.yaml");
        let response = render_preview(Engine::shared(), &IconLibrary::default(), &request(SPEC, Some(&spec_path)));
        assert_eq!(response.status, PreviewStatus::Ready);
        assert_eq!(codes(&response), [crate::icons::ICON_LOAD_CODE]);
        Ok(())
    }

    #[test]
    fn real_engine_errors_are_errors_not_warming_up() {
        let limit = RenderError::Limit { what: "width", actual: 9000, limit: 8192 };
        let response = render_failed(render_error_diagnostic(&limit));
        assert_eq!(response.status, PreviewStatus::Invalid);
        assert_eq!(response.diagnostics[0].code, "E_LIMIT");
        assert!(response.diagnostics[0].message.contains("9000"));
        let raster = render_error_diagnostic(&RenderError::Raster("bad svg".into()));
        assert_eq!((raster.code.as_str(), raster.severity), (RENDER_FAILED_CODE, Severity::Error));
        assert!(raster.message.contains("bad svg"));
    }

    #[test]
    fn panics_become_messages() {
        let outcome: Result<(), String> = run_guarded(|| panic!("not yet"));
        assert_eq!(outcome, Err("not yet".to_owned()));
    }
}
