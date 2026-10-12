//! Status bar: engine state, resolved view, caret and diagram size (the zoom lives on the canvas).

use dioramic_desktop_ipc::PreviewStatus;
use leptos::prelude::*;

use crate::state::AppState;

/// Bottom strip.
#[component]
pub fn StatusBar() -> impl IntoView {
    let state = expect_context::<AppState>();
    let engine = move || {
        if state.rendering.get() {
            return ("busy", "Rendering…".to_owned());
        }
        state.preview.with(|preview| match preview.as_ref().map(|p| (&p.status, p.elapsed_ms)) {
            None => ("idle", "Ready".to_owned()),
            Some((PreviewStatus::Ready, elapsed)) => ("ok", format!("Rendered in {elapsed} ms")),
            Some((PreviewStatus::Invalid, _)) => ("error", "Spec has errors".to_owned()),
            Some((PreviewStatus::EngineUnavailable { .. }, _)) => ("pending", "No renderer".to_owned()),
        })
    };
    let view_summary = move || {
        let view = state.preview.with(|preview| preview.as_ref().and_then(|p| p.view))?;
        let auto = state.view.with(|v| v.rotation == Some(dioramic_desktop_ipc::ViewRotation::Auto));
        Some(format!(
            "{}°{} · {} · {}",
            view.rotation_degrees,
            if auto { " auto" } else { "" },
            view.density.spec_value(),
            view.theme.spec_value()
        ))
    };
    let has_file = move || state.open_path.with(Option::is_some);

    view! {
        <footer class="statusbar">
            <span class=move || format!("sb-item engine {}", engine().0)>
                <span class="sb-dot"></span>
                {move || engine().1}
            </span>
            {move || view_summary().map(|summary| view! { <span class="sb-item">{summary}</span> })}
            <span class="spacer"></span>
            <Show when=has_file>
                <span class="sb-item">{move || {
                    let (line, column) = state.cursor.get();
                    format!("Ln {line}, Col {column}")
                }}</span>
                <span class="sb-item">"YAML"</span>
                {move || {
                    state.diagram_size.get().map(|(width, height)| {
                        view! { <span class="sb-item" title="Diagram size">{format!("{width:.0} × {height:.0} px")}</span> }
                    })
                }}
            </Show>
        </footer>
    }
}
