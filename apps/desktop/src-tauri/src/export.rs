//! Export: render through the engine and write the bytes where the user chose.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use dioramic_desktop_ipc::{ExportFormat, ExportRequest};
use dioramic_render::{Engine, OutputFormat, PreviewMode, RenderRequest};

use crate::error::CommandError;
use crate::icons::IconLibrary;
use crate::preview::run_guarded;
use crate::view::render_request;
use crate::workspace::SPEC_SUFFIXES;

/// PNG pixel density bounds accepted by the engine.
const MIN_SCALE: f64 = 1.0;
const MAX_SCALE: f64 = 4.0;

/// Ids of export jobs the user cancelled. The engine has no cancellation hook, so a cancelled job
/// finishes rendering in the background and its bytes are simply never written.
#[derive(Debug, Default)]
pub struct ExportJobs {
    cancelled: Mutex<BTreeSet<u32>>,
}

impl ExportJobs {
    /// Marks a job as cancelled.
    pub fn cancel(&self, job_id: u32) {
        if let Ok(mut cancelled) = self.cancelled.lock() {
            cancelled.insert(job_id);
        }
    }

    /// True (once) if the job was cancelled; forgets the id.
    pub fn take_cancelled(&self, job_id: u32) -> bool {
        self.cancelled.lock().map(|mut cancelled| cancelled.remove(&job_id)).unwrap_or(false)
    }
}

/// Engine request for an export, with the icons of the installed packs and of the spec's `icons/` folder
/// (the same ones the preview shows).
pub fn export_request(icons: &IconLibrary, request: &ExportRequest) -> RenderRequest {
    let mut render = render_request(&request.text, &request.view, output_format(request.format));
    render.icons = icons.icons_for(request.spec_path.as_deref().map(Path::new)).registry;
    render.scale = request.scale.clamp(MIN_SCALE, MAX_SCALE);
    render.preview = PreviewMode::None;
    render
}

fn output_format(format: ExportFormat) -> OutputFormat {
    match format {
        ExportFormat::Png => OutputFormat::Png,
        ExportFormat::Svg => OutputFormat::Svg,
        ExportFormat::Gif => OutputFormat::Gif,
    }
}

/// Renders the export bytes.
pub fn render_export(engine: &Engine, icons: &IconLibrary, request: &ExportRequest) -> Result<Vec<u8>, CommandError> {
    let render = export_request(icons, request);
    match run_guarded(|| engine.render(&render)) {
        Ok(Ok(result)) => Ok(result.output),
        Ok(Err(error)) => Err(CommandError::Engine(error.to_string())),
        Err(panic) => Err(CommandError::Engine(format!("the engine stopped: {panic}"))),
    }
}

/// Writes the bytes, creating the parent folder if needed.
pub fn write_export(path: &Path, bytes: &[u8]) -> Result<u64, CommandError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| CommandError::io(parent, &error))?;
    }
    fs::write(path, bytes).map_err(|error| CommandError::io(path, &error))?;
    Ok(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
}

/// Default output path: next to the spec, `checkout.diorama.yaml` → `checkout.png`.
pub fn suggested_path(spec_path: Option<&str>, format: ExportFormat) -> (Option<PathBuf>, String) {
    let Some(spec_path) = spec_path.map(Path::new) else {
        return (None, format!("diagram.{}", format.extension()));
    };
    let file_name = spec_path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = SPEC_SUFFIXES
        .iter()
        .find_map(|suffix| {
            file_name.to_ascii_lowercase().ends_with(suffix).then(|| &file_name[..file_name.len() - suffix.len()])
        })
        .filter(|stem| !stem.is_empty())
        .unwrap_or("diagram");
    (spec_path.parent().map(Path::to_path_buf), format!("{stem}.{}", format.extension()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icons::test_packs::install_aws;
    use dioramic_desktop_ipc::ViewState;
    use dioramic_packs::PackStore;

    fn request(format: ExportFormat, scale: f64) -> ExportRequest {
        ExportRequest {
            job_id: 1,
            text: "diorama: 1\nnodes:\n  - { id: api }\n".into(),
            view: ViewState::default(),
            format,
            scale,
            spec_path: None,
        }
    }

    #[test]
    fn export_request_maps_format_and_clamps_scale() {
        let gif = export_request(&IconLibrary::default(), &request(ExportFormat::Gif, 9.0));
        assert_eq!(gif.format, OutputFormat::Gif);
        assert!((gif.scale - MAX_SCALE).abs() < f64::EPSILON);
        assert_eq!(gif.preview, PreviewMode::None);
        let svg = export_request(&IconLibrary::default(), &request(ExportFormat::Svg, 0.1));
        assert_eq!(svg.format, OutputFormat::Svg);
        assert!(svg.svg_text_as_outlines, "exported SVG must not depend on installed fonts");
        assert!((svg.scale - MIN_SCALE).abs() < f64::EPSILON);
    }

    #[test]
    fn suggested_path_sits_next_to_the_spec() {
        let (folder, name) = suggested_path(Some("/w/examples/checkout.diorama.yaml"), ExportFormat::Gif);
        assert_eq!(folder, Some(PathBuf::from("/w/examples")));
        assert_eq!(name, "checkout.gif");
        assert_eq!(suggested_path(None, ExportFormat::Png).1, "diagram.png");
    }

    #[test]
    fn render_export_either_renders_or_explains() {
        match render_export(Engine::shared(), &IconLibrary::default(), &request(ExportFormat::Png, 2.0)) {
            Ok(bytes) => assert!(bytes.starts_with(b"\x89PNG")),
            Err(error) => assert!(!error.to_string().is_empty()),
        }
    }

    #[test]
    fn export_uses_the_installed_packs() -> Result<(), Box<dyn std::error::Error>> {
        let store = tempfile::tempdir()?;
        install_aws(store.path())?;
        let icons = IconLibrary::with_store(Some(PackStore::at(store.path())));
        let mut export = request(ExportFormat::Svg, 1.0);
        export.text = "diorama: 1\nnodes:\n  - { id: fn, icon: aws/lambda }\n".into();
        let without = String::from_utf8(render_export(Engine::shared(), &IconLibrary::default(), &export)?)?;
        let with = String::from_utf8(render_export(Engine::shared(), &icons, &export)?)?;
        assert!(!without.to_ascii_uppercase().contains("ED7100"));
        assert!(with.to_ascii_uppercase().contains("ED7100"));
        export.format = ExportFormat::Png;
        assert!(render_export(Engine::shared(), &icons, &export)?.starts_with(b"\x89PNG"));
        Ok(())
    }

    #[test]
    fn write_export_creates_folders() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("out/sub/checkout.png");
        assert_eq!(write_export(&path, b"png")?, 3);
        assert_eq!(fs::read(path)?, b"png");
        Ok(())
    }

    #[test]
    fn cancelled_jobs_are_reported_once() {
        let jobs = ExportJobs::default();
        jobs.cancel(7);
        assert!(jobs.take_cancelled(7));
        assert!(!jobs.take_cancelled(7));
        assert!(!jobs.take_cancelled(8));
    }
}
