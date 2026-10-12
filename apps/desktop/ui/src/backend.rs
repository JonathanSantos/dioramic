//! Typed bridge to the Tauri backend (`window.__TAURI__`, enabled by `withGlobalTauri`).
//!
//! Outside Tauri (plain `trunk serve` in a browser) the calls fall back to [`crate::demo`], so the UI
//! can be designed and reviewed in any browser.

use dioramic_desktop_ipc::{
    ExportOutcome, ExportRequest, IconsChanged, PinRequest, PreviewRequest, PreviewResponse, SaveRequest, SpecChanged,
    Workspace, events,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use wasm_bindgen::prelude::*;

use crate::demo;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke, catch)]
    async fn tauri_invoke(command: &str, args: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], js_name = listen, catch)]
    async fn tauri_listen(event: &str, handler: &Closure<dyn FnMut(JsValue)>) -> Result<JsValue, JsValue>;
}

/// True inside the Tauri webview.
pub fn is_tauri() -> bool {
    web_sys::window()
        .and_then(|window| js_sys::Reflect::get(&window, &JsValue::from_str("__TAURI__")).ok())
        .is_some_and(|tauri| !tauri.is_undefined())
}

async fn call<R: DeserializeOwned>(command: &str, args: &impl Serialize) -> Result<R, String> {
    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    let args = args.serialize(&serializer).map_err(|error| error.to_string())?;
    let value =
        tauri_invoke(command, args).await.map_err(|error| error.as_string().unwrap_or_else(|| format!("{error:?}")))?;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

#[derive(Serialize)]
struct NoArgs {}

#[derive(Serialize)]
struct PathArg<'a> {
    path: &'a str,
}

#[derive(Serialize)]
struct RequestArg<'a, T> {
    request: &'a T,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JobArg {
    job_id: u32,
}

/// Folder given at startup (CLI argument or `DIORAMIC_WORKSPACE`).
pub async fn startup_folder() -> Option<String> {
    if !is_tauri() {
        return Some(demo::ROOT.to_owned());
    }
    call("startup_folder", &NoArgs {}).await.ok().flatten()
}

/// Native folder picker.
pub async fn pick_workspace() -> Result<Option<Workspace>, String> {
    if !is_tauri() {
        return Ok(Some(demo::workspace()));
    }
    call("pick_workspace", &NoArgs {}).await
}

/// Opens (scans and watches) a folder.
pub async fn open_workspace(path: &str) -> Result<Workspace, String> {
    if !is_tauri() {
        return Ok(demo::workspace());
    }
    call("open_workspace", &PathArg { path }).await
}

/// Reads a spec.
pub async fn read_spec(path: &str) -> Result<String, String> {
    if !is_tauri() {
        return demo::read(path);
    }
    call("read_spec", &PathArg { path }).await
}

/// Saves a spec.
pub async fn save_spec(request: &SaveRequest) -> Result<(), String> {
    if !is_tauri() {
        return Ok(());
    }
    call("save_spec", &RequestArg { request }).await
}

/// Live preview.
pub async fn preview(request: &PreviewRequest) -> Result<PreviewResponse, String> {
    if !is_tauri() {
        return Ok(demo::preview(request));
    }
    call("preview", &RequestArg { request }).await
}

/// Writes the view into the spec text.
pub async fn pin_view(request: &PinRequest) -> Result<String, String> {
    if !is_tauri() {
        return Ok(request.text.clone());
    }
    call("pin_view", &RequestArg { request }).await
}

/// Export (save dialog + render + write).
pub async fn export_diagram(request: &ExportRequest) -> Result<ExportOutcome, String> {
    if !is_tauri() {
        return Err("Export needs the desktop app.".to_owned());
    }
    call("export_diagram", &RequestArg { request }).await
}

/// Cancels an export.
pub async fn cancel_export(job_id: u32) {
    if is_tauri() {
        let _: Result<(), String> = call("cancel_export", &JobArg { job_id }).await;
    }
}

/// Reveals a file in the system file manager.
pub async fn reveal_path(path: &str) {
    if is_tauri() {
        let _: Result<(), String> = call("reveal_path", &PathArg { path }).await;
    }
}

/// Subscribes to a backend event for the lifetime of the app.
async fn listen<T: DeserializeOwned + 'static>(event: &str, handler: impl Fn(T) + 'static) {
    if !is_tauri() {
        return;
    }
    let closure = Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
        let payload = js_sys::Reflect::get(&event, &JsValue::from_str("payload")).unwrap_or(JsValue::NULL);
        if let Ok(value) = serde_wasm_bindgen::from_value(payload) {
            handler(value);
        }
    });
    if tauri_listen(event, &closure).await.is_ok() {
        // The subscription lives as long as the window: hand the closure over to JS.
        closure.forget();
    }
}

/// The open folder's tree changed.
pub async fn on_workspace_changed(handler: impl Fn(Workspace) + 'static) {
    listen(events::WORKSPACE_CHANGED, handler).await;
}

/// A spec changed on disk.
pub async fn on_spec_changed(handler: impl Fn(SpecChanged) + 'static) {
    listen(events::SPEC_CHANGED, handler).await;
}

/// An icon pack was installed or removed, or a project `icons/` folder changed.
pub async fn on_icons_changed(handler: impl Fn(IconsChanged) + 'static) {
    listen(events::ICONS_CHANGED, handler).await;
}
