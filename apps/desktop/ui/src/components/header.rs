//! Title bar: brand, breadcrumb of the open file (folders relative to the workspace), problem
//! summary, panel toggles and the appearance toggle. The window uses an overlay title bar, so this
//! strip is also the drag region.

use dioramic_desktop_ipc::Severity;
use leptos::prelude::*;

use crate::components::preview::plural;
use crate::components::sidebar::{middle_truncate, split_suffix};
use crate::icons::{Icon, IconView};
use crate::state::{AppState, UiTheme};

/// Longest file stem shown in the breadcrumb before it is cut in the middle (characters).
const CRUMB_STEM_BUDGET: usize = 40;
/// Longest folder name in the breadcrumb (characters).
const CRUMB_FOLDER_BUDGET: usize = 18;

/// Folders between the workspace root and the file, and the file name: `/w/a/b/x.yaml` → (`[a, b]`, `x.yaml`).
pub fn relative_parts(root: &str, path: &str) -> (Vec<String>, String) {
    let relative = path.strip_prefix(root).unwrap_or(path).trim_start_matches(['/', '\\']);
    let mut parts: Vec<String> =
        relative.split(['/', '\\']).filter(|part| !part.is_empty()).map(str::to_owned).collect();
    let name = parts.pop().unwrap_or_default();
    (parts, name)
}

/// Title bar.
#[component]
pub fn Header() -> impl IntoView {
    let state = expect_context::<AppState>();
    let layout = state.layout;
    let workspace_name = move || state.workspace.with(|workspace| workspace.as_ref().map(|w| w.name.clone()));
    let crumbs = move || {
        let path = state.open_path.get()?;
        let root = state.workspace.with(|workspace| workspace.as_ref().map(|w| w.root.clone())).unwrap_or_default();
        Some(relative_parts(&root, &path))
    };
    let dirty = move || state.is_dirty();
    let has_file = move || state.open_path.with(Option::is_some);

    view! {
        <header class="titlebar" data-tauri-drag-region>
            <div class="brand" data-tauri-drag-region>
                <img class="brand-mark only-light" src="dioramic-mark.svg" alt="" draggable="false" />
                <img class="brand-mark only-dark" src="dioramic-mark-dark.svg" alt="" draggable="false" />
                <img class="brand-word only-light" src="dioramic-wordmark.svg" alt="dioramic" draggable="false" />
                <img class="brand-word only-dark" src="dioramic-wordmark-dark.svg" alt="dioramic" draggable="false" />
            </div>
            <nav class="crumbs" aria-label="Open file" data-tauri-drag-region>
                {move || workspace_name().map(|name| view! { <span class="crumb">{name}</span> })}
                {move || {
                    crumbs()
                        .map(|(folders, name)| {
                            let stem = split_suffix(&name).0;
                            view! {
                                {folders
                                    .into_iter()
                                    .map(|folder| {
                                        view! {
                                            <span class="crumb-sep">"/"</span>
                                            <span class="crumb" title=folder.clone()>
                                                {middle_truncate(&folder, CRUMB_FOLDER_BUDGET)}
                                            </span>
                                        }
                                    })
                                    .collect_view()}
                                <span class="crumb-sep">"/"</span>
                                <span class="crumb current" title=name.clone()>
                                    {middle_truncate(&stem, CRUMB_STEM_BUDGET)}
                                </span>
                                <span class="dirty-dot" class:on=dirty title="Unsaved changes"></span>
                            }
                        })
                }}
            </nav>
            <div class="titlebar-actions">
                <ProblemSummary />
                <span class="tb-divider" aria-hidden="true"></span>
                <button
                    class="icon-btn"
                    class:active=move || layout.sidebar_visible()
                    aria-label="Show files"
                    aria-pressed=move || layout.sidebar_visible().to_string()
                    data-tip="Files"
                    data-kbd="⌘ B"
                    on:click=move |_| layout.toggle_sidebar()
                >
                    <IconView icon=Icon::Sidebar />
                </button>
                <button
                    class="icon-btn"
                    class:active=move || layout.editor_open.get()
                    disabled=move || !has_file()
                    aria-label="Show editor"
                    aria-pressed=move || layout.editor_open.get().to_string()
                    data-tip="Editor"
                    data-kbd="⌘ \\"
                    on:click=move |_| layout.toggle_editor()
                >
                    <IconView icon=Icon::EditorPanel />
                </button>
                <button
                    class="icon-btn"
                    aria-label=move || state.ui_theme.get().label()
                    data-tip=move || state.ui_theme.get().label()
                    on:click=move |_| state.ui_theme.update(|theme| *theme = theme.next())
                >
                    {move || {
                        let icon = match state.ui_theme.get() {
                            UiTheme::System => Icon::System,
                            UiTheme::Light => Icon::Sun,
                            UiTheme::Dark => Icon::Moon,
                        };
                        view! { <IconView icon=icon /> }
                    }}
                </button>
            </div>
        </header>
    }
}

#[component]
fn ProblemSummary() -> impl IntoView {
    let state = expect_context::<AppState>();
    let errors = move || state.severity_count(Severity::Error);
    let warnings = move || state.severity_count(Severity::Warning);
    let label = move || format!("{}, {}", plural(errors(), "error"), plural(warnings(), "warning"));
    view! {
        <Show when=move || state.open_path.with(Option::is_some)>
            <div class="problem-summary" class:clean=move || errors() + warnings() == 0 role="status" aria-label=label>
                <span class="ps-item error" class:zero=move || errors() == 0>
                    <IconView icon=Icon::Error />
                    {errors}
                </span>
                <span class="ps-item warning" class:zero=move || warnings() == 0>
                    <IconView icon=Icon::Warning />
                    {warnings}
                </span>
            </div>
        </Show>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breadcrumb_is_relative_to_the_workspace() {
        assert_eq!(
            relative_parts("/w/examples", "/w/examples/platform/payments/x.diorama.yaml"),
            (vec!["platform".to_owned(), "payments".to_owned()], "x.diorama.yaml".to_owned())
        );
        assert_eq!(
            relative_parts("/w/examples", "/w/examples/checkout.diorama.yaml"),
            (vec![], "checkout.diorama.yaml".to_owned())
        );
        assert_eq!(relative_parts("C:\\w", "C:\\w\\a\\b.yaml"), (vec!["a".to_owned()], "b.yaml".to_owned()));
    }
}
