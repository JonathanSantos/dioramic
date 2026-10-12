//! Dioramic desktop backend (Tauri 2). The UI (Leptos, `../ui`) talks to it only through the
//! commands in [`commands`]; every render goes through `dioramic_render::Engine` (T-203, doc 08).

pub mod commands;
pub mod diagnostics;
pub mod error;
pub mod export;
pub mod icons;
pub mod pin;
pub mod preview;
pub mod view;
pub mod watcher;
pub mod workspace;

use std::sync::Arc;

use commands::{DesktopState, emit_icons_changed};
use icons::{IconLibrary, monitor_store};

/// Builds and runs the app.
pub fn run() -> Result<(), tauri::Error> {
    let icons = Arc::new(IconLibrary::from_env());
    let monitored = Arc::clone(&icons);
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(DesktopState::new(icons))
        .setup(move |app| {
            if let Some(root) = monitored.store_root().map(std::path::Path::to_path_buf) {
                let handle = app.handle().clone();
                monitor_store(monitored, move || emit_icons_changed(&handle, &root))?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::startup_folder,
            commands::pick_workspace,
            commands::open_workspace,
            commands::read_spec,
            commands::save_spec,
            commands::preview,
            commands::pin_view,
            commands::export_diagram,
            commands::cancel_export,
            commands::reveal_path,
        ])
        .run(tauri::generate_context!())
}
