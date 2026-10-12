//! Tauri commands: thin async wrappers that move the work off the UI thread and delegate to the
//! pure modules (`workspace`, `preview`, `export`, `pin`), which carry the tests.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use dioramic_desktop_ipc::{
    ExportOutcome, ExportRequest, IconsChanged, PinRequest, PreviewRequest, PreviewResponse, SaveRequest, SpecChanged,
    Workspace, events,
};
use dioramic_render::Engine;
use notify::RecommendedWatcher;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

use crate::error::CommandError;
use crate::export::{ExportJobs, render_export, suggested_path, write_export};
use crate::icons::IconLibrary;
use crate::preview::render_preview;
use crate::watcher::{WatchBatch, watch};
use crate::{pin, workspace};

/// Environment variable naming a folder to open at startup (handy for `cargo tauri dev`).
pub const WORKSPACE_ENV: &str = "DIORAMIC_WORKSPACE";

/// State shared by the commands.
pub struct DesktopState {
    watcher: Mutex<Option<RecommendedWatcher>>,
    exports: ExportJobs,
    icons: Arc<IconLibrary>,
}

impl DesktopState {
    /// State rendering with the icon packs of `icons`.
    pub fn new(icons: Arc<IconLibrary>) -> Self {
        Self { watcher: Mutex::new(None), exports: ExportJobs::default(), icons }
    }
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, CommandError> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(|error| CommandError::Task(error.to_string()))
}

/// Folder passed on the command line or in `DIORAMIC_WORKSPACE`, if any.
#[tauri::command]
pub fn startup_folder() -> Option<String> {
    let from_args = std::env::args().skip(1).find(|argument| Path::new(argument).is_dir());
    from_args.or_else(|| std::env::var(WORKSPACE_ENV).ok()).filter(|path| Path::new(path).is_dir())
}

/// Shows the native folder picker and opens the chosen folder.
#[tauri::command]
pub async fn pick_workspace(app: AppHandle, state: State<'_, DesktopState>) -> Result<Option<Workspace>, CommandError> {
    let dialog_app = app.clone();
    let picked =
        blocking(move || dialog_app.dialog().file().set_title("Open a folder with diagrams").blocking_pick_folder())
            .await?;
    let Some(folder) = picked.and_then(|folder| folder.into_path().ok()) else {
        return Ok(None);
    };
    open_folder(&app, &state, folder).await.map(Some)
}

/// Opens a folder: scans it and starts watching it, off the main thread (a big folder takes a while).
#[tauri::command]
pub async fn open_workspace(
    app: AppHandle,
    state: State<'_, DesktopState>,
    path: String,
) -> Result<Workspace, CommandError> {
    open_folder(&app, &state, PathBuf::from(path)).await
}

async fn open_folder(app: &AppHandle, state: &DesktopState, folder: PathBuf) -> Result<Workspace, CommandError> {
    let events_app = app.clone();
    let (workspace, watcher) = blocking(move || -> Result<_, CommandError> {
        let workspace = workspace::scan(&folder)?;
        let root = folder.clone();
        let watcher = watch(&folder, move |batch| emit_batch(&events_app, &root, &batch))?;
        Ok((workspace, watcher))
    })
    .await??;
    if let Ok(mut current) = state.watcher.lock() {
        // Replacing the old watcher drops it, which also ends its thread.
        *current = Some(watcher);
    }
    Ok(workspace)
}

fn emit_batch(app: &AppHandle, root: &Path, batch: &WatchBatch) {
    if batch.tree_changed
        && let Ok(workspace) = workspace::scan(root)
    {
        let _ = app.emit(events::WORKSPACE_CHANGED, &workspace);
    }
    for path in &batch.changed_specs {
        let _ = app.emit(events::SPEC_CHANGED, &SpecChanged { path: path.display().to_string() });
    }
    for path in &batch.changed_icon_dirs {
        emit_icons_changed(app, path);
    }
}

/// Tells the UI to render again because icons changed under `path`.
pub fn emit_icons_changed(app: &AppHandle, path: &Path) {
    let _ = app.emit(events::ICONS_CHANGED, &IconsChanged { path: path.display().to_string() });
}

/// Reads a spec file.
#[tauri::command]
pub async fn read_spec(path: String) -> Result<String, CommandError> {
    blocking(move || workspace::read_spec(Path::new(&path))).await?
}

/// Saves the editor text verbatim.
#[tauri::command]
pub async fn save_spec(request: SaveRequest) -> Result<(), CommandError> {
    blocking(move || workspace::save_spec(Path::new(&request.path), &request.text)).await?
}

/// Live preview (SVG + diagnostics), rendered off the UI thread.
#[tauri::command]
pub async fn preview(state: State<'_, DesktopState>, request: PreviewRequest) -> Result<PreviewResponse, CommandError> {
    let icons = Arc::clone(&state.icons);
    blocking(move || render_preview(Engine::shared(), &icons, &request)).await
}

/// Writes the view into the spec text and returns the new text (the editor decides when to save).
/// Fails, leaving the editor untouched, when the spec cannot be edited safely.
#[tauri::command]
pub async fn pin_view(request: PinRequest) -> Result<String, CommandError> {
    blocking(move || pin::try_pin_view(&request.text, &request.view)).await?.map_err(CommandError::from)
}

/// Asks where to save, renders and writes the export.
#[tauri::command]
pub async fn export_diagram(
    app: AppHandle,
    state: State<'_, DesktopState>,
    request: ExportRequest,
) -> Result<ExportOutcome, CommandError> {
    let Some(target) = ask_export_path(&app, &request).await? else {
        return Ok(ExportOutcome::Cancelled);
    };
    let job_id = request.job_id;
    let icons = Arc::clone(&state.icons);
    let bytes = blocking(move || render_export(Engine::shared(), &icons, &request)).await??;
    if state.exports.take_cancelled(job_id) {
        return Ok(ExportOutcome::Cancelled);
    }
    let written = write_export(&target, &bytes)?;
    Ok(ExportOutcome::Saved { path: target.display().to_string(), bytes: written })
}

async fn ask_export_path(app: &AppHandle, request: &ExportRequest) -> Result<Option<PathBuf>, CommandError> {
    let (folder, file_name) = suggested_path(request.spec_path.as_deref(), request.format);
    let format = request.format;
    let dialog_app = app.clone();
    let picked = blocking(move || {
        let mut dialog = dialog_app
            .dialog()
            .file()
            .set_title("Export diagram")
            .set_file_name(file_name)
            .add_filter(format.label(), &[format.extension()]);
        if let Some(folder) = folder {
            dialog = dialog.set_directory(folder);
        }
        dialog.blocking_save_file()
    })
    .await?;
    Ok(picked.and_then(|path| path.into_path().ok()))
}

/// Cancels an export in flight (its result is discarded).
#[tauri::command]
pub fn cancel_export(state: State<'_, DesktopState>, job_id: u32) {
    state.exports.cancel(job_id);
}

/// Shows a file in the system file manager.
#[tauri::command]
pub async fn reveal_path(path: String) -> Result<(), CommandError> {
    let path = PathBuf::from(path);
    let mut command = reveal_command(&path);
    command.spawn().map(drop).map_err(|error| CommandError::io(&path, &error))
}

fn reveal_command(path: &Path) -> Command {
    if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg("-R").arg(path);
        command
    } else if cfg!(target_os = "windows") {
        let mut command = Command::new("explorer");
        command.arg(format!("/select,{}", path.display()));
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(path.parent().unwrap_or(path));
        command
    }
}
