//! Per-user conveniences in `localStorage` (last folder, last file, UI theme, panel sizes). Always optional:
//! every access tolerates storage being unavailable.

/// Last opened folder.
pub const LAST_WORKSPACE: &str = "dioramic.last-workspace";
/// Last opened spec.
pub const LAST_FILE: &str = "dioramic.last-file";
/// UI theme (`light` / `dark`; absent = system). Also read by the inline script in `index.html`.
pub const UI_THEME: &str = "dioramic.ui-theme";
/// Sidebar shown on wide windows (`1` / `0`).
pub const SIDEBAR_OPEN: &str = "dioramic.sidebar-open";
/// Sidebar width (px).
pub const SIDEBAR_WIDTH: &str = "dioramic.sidebar-width";
/// Editor shown (`1` / `0`; `0` = preview mode).
pub const EDITOR_OPEN: &str = "dioramic.editor-open";
/// Editor share of the room right of the sidebar.
pub const EDITOR_RATIO: &str = "dioramic.editor-ratio";
/// Problems panel height (px).
pub const PROBLEMS_HEIGHT: &str = "dioramic.problems-height";
/// Soft wrap in the editor (`1` / `0`).
pub const SOFT_WRAP: &str = "dioramic.soft-wrap";
/// Preview in motion (`1` / `0`).
pub const MOTION: &str = "dioramic.motion";

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// Reads a value.
pub fn get(key: &str) -> Option<String> {
    storage()?.get_item(key).ok().flatten()
}

/// Writes a value.
pub fn set(key: &str, value: &str) {
    if let Some(storage) = storage() {
        let _ = storage.set_item(key, value);
    }
}

/// Removes a value.
pub fn remove(key: &str) {
    if let Some(storage) = storage() {
        let _ = storage.remove_item(key);
    }
}
