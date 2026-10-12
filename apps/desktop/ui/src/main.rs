//! Dioramic desktop frontend: Leptos (CSR) running in the Tauri webview.
//!
//! Layout: title bar · [files | YAML editor + problems | live preview with floating toolbar] · status bar.
//! All rendering happens in the backend (`dioramic_render::Engine`) through [`backend`].

mod app;
mod backend;
mod components;
#[cfg(test)]
mod contrast;
mod demo;
mod dom;
mod highlight;
mod icons;
mod layout;
mod locate;
mod motion;
mod state;
mod storage;
mod text_pos;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
