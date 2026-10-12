//! Root component: layout, startup, global shortcuts and the UI theme.

use leptos::ev;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use crate::backend;
use crate::components::editor::EditorPane;
use crate::components::header::Header;
use crate::components::preview::PreviewPane;
use crate::components::sidebar::Sidebar;
use crate::components::splitter::{Axis, Splitter};
use crate::components::statusbar::StatusBar;
use crate::components::toasts::Toasts;
use crate::components::tooltip::Tooltips;
use crate::components::welcome::Welcome;
use crate::layout::{EDITOR_DEFAULT_RATIO, ratio_at, viewport_width};
use crate::state::{AppState, ExportState};
use crate::storage;

/// The app.
#[component]
pub fn App() -> impl IntoView {
    let state = AppState::new();
    provide_context(state);

    Effect::new(move |_| apply_ui_theme(state.ui_theme.get().attribute()));
    crate::motion::watch_reduced_motion(move |reduced| state.set_reduced_motion(reduced));
    start(state);
    let shortcuts = window_event_listener(ev::keydown, move |event| handle_shortcut(state, &event));
    let resize = window_event_listener(ev::resize, move |_| state.layout.viewport.set(viewport_width()));
    on_cleanup(move || {
        shortcuts.remove();
        resize.remove();
    });

    let layout = state.layout;
    let has_file = move || state.open_path.with(Option::is_some);
    let columns = move || layout.columns(has_file());
    let grid = move || {
        let split = columns();
        if has_file() {
            format!("grid-template-columns: {:.1}px {:.1}px minmax(0, 1fr)", split.sidebar, split.editor)
        } else {
            format!("grid-template-columns: {:.1}px minmax(0, 1fr)", split.sidebar)
        }
    };
    view! {
        <div class="app" class:has-file=has_file>
            <Header />
            <div class="workbench" style=grid>
                <div class="column sidebar-column" class:folded=move || !layout.sidebar_visible()>
                    <Sidebar />
                </div>
                <Show when=has_file fallback=|| view! { <Welcome /> }>
                    <div class="column editor-column" class:folded=move || !layout.editor_open.get()>
                        <EditorPane />
                        <Splitter
                            axis=Axis::Columns
                            label="Resize editor"
                            on_drag=move |x: f64| {
                                let split = columns();
                                layout.set_editor_ratio(ratio_at(x, layout.viewport.get_untracked(), split.sidebar));
                            }
                            on_step=move |step: f64| {
                                let split = columns();
                                let edge = split.sidebar + split.editor + step;
                                layout.set_editor_ratio(ratio_at(edge, layout.viewport.get_untracked(), split.sidebar));
                            }
                            on_reset=move |()| layout.set_editor_ratio(EDITOR_DEFAULT_RATIO)
                        />
                    </div>
                    <PreviewPane />
                </Show>
            </div>
            <StatusBar />
            <Toasts />
            <Tooltips />
        </div>
    }
}

/// Subscribes to backend events and opens the startup (or last) folder.
fn start(state: AppState) {
    spawn_local(async move {
        backend::on_workspace_changed(move |workspace| state.refresh_workspace(workspace)).await;
        backend::on_spec_changed(move |change| state.on_disk_change(change.path)).await;
        // Icons are read by the backend at each render; a new pack or logo only needs a fresh preview.
        backend::on_icons_changed(move |_| state.schedule_preview()).await;
        let folder = match backend::startup_folder().await {
            Some(folder) => Some(folder),
            None => storage::get(storage::LAST_WORKSPACE),
        };
        if let Some(folder) = folder {
            state.open_workspace(folder);
        }
    });
}

fn apply_ui_theme(attribute: Option<&str>) {
    let Some(root) =
        web_sys::window().and_then(|window| window.document()).and_then(|document| document.document_element())
    else {
        return;
    };
    match attribute {
        Some(value) => {
            let _ = root.set_attribute("data-theme", value);
            storage::set(storage::UI_THEME, value);
        }
        None => {
            let _ = root.remove_attribute("data-theme");
            storage::remove(storage::UI_THEME);
        }
    }
}

fn is_typing_target(event: &web_sys::KeyboardEvent) -> bool {
    event
        .target()
        .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
        .is_some_and(|element| matches!(element.tag_name().as_str(), "TEXTAREA" | "INPUT" | "SELECT"))
}

/// Global shortcuts. Single-key ones (`[ ] A D R G M`) only fire outside the editor.
fn handle_shortcut(state: AppState, event: &web_sys::KeyboardEvent) {
    let command = event.meta_key() || event.ctrl_key();
    let key = event.key();
    let handled = if command {
        match key.to_ascii_lowercase().as_str() {
            "s" => {
                state.save();
                true
            }
            "e" if state.open_path.with_untracked(Option::is_some) => {
                state.close_popovers();
                state.export.set(ExportState::Choosing);
                true
            }
            "o" => {
                state.pick_workspace();
                true
            }
            "b" => {
                state.layout.toggle_sidebar();
                true
            }
            "\\" if state.open_path.with_untracked(Option::is_some) => {
                state.layout.toggle_editor();
                true
            }
            _ => false,
        }
    } else if event.alt_key() && event.code() == "KeyZ" {
        state.layout.toggle_soft_wrap();
        true
    } else if key == "Escape" {
        state.close_popovers();
        state.selected_diagnostic.set(None);
        false
    } else if is_typing_target(event) || event.alt_key() || state.open_path.with_untracked(Option::is_none) {
        false
    } else {
        match key.as_str() {
            "[" => state.rotate(-1),
            "]" => state.rotate(1),
            "a" | "A" => state.toggle_auto(),
            "d" | "D" => state.toggle_density(),
            "r" | "R" => state.toggle_routing(),
            "l" | "L" => state.toggle_layout(),
            "g" | "G" => state.toggle_grid_popover(),
            "m" | "M" => state.toggle_motion(),
            "0" => state.fit_request.update(|count| *count += 1),
            _ => return,
        }
        true
    };
    if handled {
        event.prevent_default();
    }
}
