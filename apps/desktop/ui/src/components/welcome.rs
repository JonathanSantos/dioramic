//! Shown in place of editor and preview until a spec is open.

use leptos::prelude::*;

use crate::icons::{Icon, IconView};
use crate::state::AppState;

/// Welcome screen.
#[component]
pub fn Welcome() -> impl IntoView {
    let state = expect_context::<AppState>();
    let has_workspace = move || state.workspace.with(Option::is_some);
    view! {
        <section class="welcome">
            <div class="welcome-card">
                <img class="welcome-mark only-light" src="dioramic-mark.svg" alt="" draggable="false" />
                <img class="welcome-mark only-dark" src="dioramic-mark-dark.svg" alt="" draggable="false" />
                <h1 class="welcome-title">"System design, in isometric."</h1>
                <p class="welcome-body">
                    "Open a folder with " <code>"*.diorama.yaml"</code>
                    " specs. Edit by hand or let your AI write them through MCP — this window follows every change, live."
                </p>
                <Show
                    when=has_workspace
                    fallback=move || {
                        view! {
                            <button class="btn primary large" on:click=move |_| state.pick_workspace()>
                                <IconView icon=Icon::FolderOpen />
                                "Open folder…"
                            </button>
                        }
                    }
                >
                    <p class="welcome-hint">"Pick a diagram on the left to start."</p>
                </Show>
                <dl class="shortcuts">
                    <div><dt><kbd>"⌘"</kbd><kbd>"O"</kbd></dt><dd>"Open folder"</dd></div>
                    <div><dt><kbd>"["</kbd><kbd>"]"</kbd></dt><dd>"Rotate camera"</dd></div>
                    <div><dt><kbd>"A"</kbd></dt><dd>"Auto angle"</dd></div>
                    <div><dt><kbd>"D"</kbd></dt><dd>"Spaced / compact"</dd></div>
                    <div><dt><kbd>"L"</kbd></dt><dd>"Flow / hub layout"</dd></div>
                    <div><dt><kbd>"⌘"</kbd><kbd>"E"</kbd></dt><dd>"Export"</dd></div>
                </dl>
            </div>
        </section>
    }
}
