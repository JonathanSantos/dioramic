//! Toast stack (bottom right).

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::backend;
use crate::icons::{Icon, IconView};
use crate::state::{AppState, ToastTone};

/// Toasts.
#[component]
pub fn Toasts() -> impl IntoView {
    let state = expect_context::<AppState>();
    view! {
        <div class="toasts" aria-live="polite">
            <For each=move || state.toasts.get() key=|toast| toast.id let:toast>
                {
                    let (class, icon) = match toast.tone {
                        ToastTone::Success => ("toast success", Icon::Check),
                        ToastTone::Error => ("toast error", Icon::Error),
                        ToastTone::Info => ("toast info", Icon::Hint),
                    };
                    let id = toast.id;
                    view! {
                        <div class=class>
                            <IconView icon=icon class="toast-icon" />
                            <span class="toast-message">{toast.message}</span>
                            {toast.reveal.map(|path| view! {
                                <button class="btn ghost small" on:click=move |_| {
                                    let path = path.clone();
                                    spawn_local(async move { backend::reveal_path(&path).await });
                                }>
                                    <IconView icon=Icon::Reveal />
                                    "Show"
                                </button>
                            })}
                            <button class="icon-btn small" on:click=move |_| state.dismiss_toast(id)>
                                <IconView icon=Icon::Close />
                            </button>
                        </div>
                    }
                }
            </For>
        </div>
    }
}
