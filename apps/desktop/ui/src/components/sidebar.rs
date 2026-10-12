//! File tree of the open folder. Specs show their stem only (`checkout`); a suffix other than the
//! usual `.diorama.yaml` becomes a small badge. Long names are cut in the middle so both ends stay
//! readable, with the full name in the tooltip.

use dioramic_desktop_ipc::{FileKind, FileNode};
use leptos::prelude::*;

use crate::components::splitter::{Axis, Splitter};
use crate::icons::{Icon, IconView};
use crate::layout::SIDEBAR_DEFAULT_PX;
use crate::state::AppState;

/// Extra left padding per tree level (px).
const INDENT_PX: usize = 14;
/// Left padding of a top-level row (px).
const ROW_PADDING_PX: usize = 10;
/// Row chrome besides the label: icon, gaps, dirty dot, paddings and the tree's own padding (px).
const ROW_CHROME_PX: f64 = 64.0;
/// Generous width of a character of a tree label (13 px Inter, semibold when active) for the budget.
const LABEL_CHAR_PX: f64 = 7.8;
/// The suffix every spec has; it is implied, so the tree hides it.
const DEFAULT_SUFFIX: &str = ".diorama.yaml";
/// Ellipsis used for middle truncation.
const ELLIPSIS: char = '…';

/// Left column.
#[component]
pub fn Sidebar() -> impl IntoView {
    let state = expect_context::<AppState>();
    let layout = state.layout;
    view! {
        <aside class="sidebar" aria-label="Files">
            <div class="sidebar-head">
                <span class="eyebrow">"Workspace"</span>
                <button
                    class="icon-btn small"
                    aria-label="Open folder"
                    data-tip="Open folder…"
                    data-kbd="⌘ O"
                    on:click=move |_| state.pick_workspace()
                >
                    <IconView icon=Icon::FolderOpen />
                </button>
            </div>
            {move || match state.workspace.get() {
                Some(workspace) => {
                    let count = workspace.spec_count();
                    let tree = workspace.tree.into_iter().map(|node| tree_node(state, node, 0)).collect_view();
                    view! {
                        <div class="ws-title" title=workspace.root.clone()>
                            <IconView icon=Icon::Folder class="ws-icon" />
                            <span class="ws-name">{workspace.name}</span>
                            <span class="ws-count" aria-label=format!("{count} diagrams")>{count}</span>
                        </div>
                        <nav class="tree">
                            {tree}
                            <Show when=move || count == 0>
                                <p class="tree-empty">"No " <code>"*.diorama.yaml"</code> " files here yet."</p>
                            </Show>
                        </nav>
                        <div class="sidebar-foot">
                            <span class="live-dot"></span>
                            "Watching for changes"
                        </div>
                    }
                        .into_any()
                }
                None => {
                    view! {
                        <div class="sidebar-empty">
                            <p>"No folder open."</p>
                            <p class="sidebar-empty-hint">
                                "Use the folder button above or " <kbd>"⌘"</kbd> <kbd>"O"</kbd> "."
                            </p>
                        </div>
                    }
                        .into_any()
                }
            }}
            <Splitter
                axis=Axis::Columns
                label="Resize sidebar"
                on_drag=move |x: f64| layout.set_sidebar_width(x)
                on_step=move |step: f64| layout.set_sidebar_width(layout.sidebar_width.get_untracked() + step)
                on_reset=move |()| layout.set_sidebar_width(SIDEBAR_DEFAULT_PX)
            />
        </aside>
    }
}

/// Characters of a label that fit a row at `depth` in a sidebar of `width` px.
fn label_budget(width: f64, depth: usize) -> usize {
    #[allow(clippy::cast_precision_loss)] // small numbers
    let indent = (depth * INDENT_PX) as f64;
    let room = (width - ROW_CHROME_PX - indent).max(LABEL_CHAR_PX * 6.0);
    // A budget in characters: small and positive.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let budget = (room / LABEL_CHAR_PX).floor() as usize;
    budget
}

fn tree_node(state: AppState, node: FileNode, depth: usize) -> AnyView {
    let padding = format!("{}px", ROW_PADDING_PX + depth * INDENT_PX);
    let width = state.layout.sidebar_width;
    match node.kind {
        FileKind::Folder => {
            let expanded = RwSignal::new(true);
            let children = node.children.into_iter().map(|child| tree_node(state, child, depth + 1)).collect_view();
            let name = node.name.clone();
            view! {
                <div class="tree-folder" class:collapsed=move || !expanded.get()>
                    <button
                        class="tree-row folder"
                        style:padding-left=padding
                        aria-expanded=move || expanded.get().to_string()
                        title=node.name.clone()
                        on:click=move |_| expanded.update(|open| *open = !*open)
                    >
                        <IconView icon=Icon::Chevron class="chevron" />
                        <span class="tree-label">{move || middle_truncate(&name, label_budget(width.get(), depth))}</span>
                    </button>
                    <div class="tree-children">{children}</div>
                </div>
            }
            .into_any()
        }
        FileKind::Spec => {
            let path = node.path.clone();
            let is_open = {
                let path = path.clone();
                move || state.open_path.with(|open| open.as_deref() == Some(path.as_str()))
            };
            let is_dirty = {
                let is_open = is_open.clone();
                move || is_open() && state.is_dirty()
            };
            let (stem, suffix) = split_suffix(&node.name);
            let badge = badge_for(&suffix);
            let has_badge = badge.is_some();
            let label = move || {
                let budget = label_budget(width.get(), depth).saturating_sub(if has_badge { 5 } else { 0 });
                middle_truncate(&stem, budget)
            };
            view! {
                <button
                    class="tree-row spec"
                    class:active=is_open.clone()
                    aria-current=move || if is_open() { Some("page") } else { None }
                    style:padding-left=padding
                    title=node.name.clone()
                    on:click=move |_| state.open_file(path.clone())
                >
                    <IconView icon=Icon::Cube class="spec-icon" />
                    <span class="tree-label">{label}</span>
                    {badge.map(|badge| view! { <span class="suffix-badge">{badge}</span> })}
                    <span class="dirty-dot" class:on=is_dirty></span>
                </button>
            }
            .into_any()
        }
    }
}

/// `checkout.diorama.yaml` → (`checkout`, `.diorama.yaml`), `api.yml` → (`api`, `.yml`).
pub fn split_suffix(name: &str) -> (String, String) {
    let lower = name.to_ascii_lowercase();
    let at = lower.find(".diorama.").or_else(|| lower.rfind('.')).filter(|&index| index > 0);
    match at {
        Some(index) => (name[..index].to_owned(), name[index..].to_owned()),
        None => (name.to_owned(), String::new()),
    }
}

/// Badge for an unusual suffix (`json`, `yml`); `None` for the default `.diorama.yaml`.
fn badge_for(suffix: &str) -> Option<String> {
    if suffix.is_empty() || suffix.eq_ignore_ascii_case(DEFAULT_SUFFIX) {
        return None;
    }
    suffix.rsplit('.').next().filter(|extension| !extension.is_empty()).map(str::to_ascii_lowercase)
}

/// Cuts the middle of a name longer than `budget` characters: `payments-recon…platform`.
pub fn middle_truncate(name: &str, budget: usize) -> String {
    let length = name.chars().count();
    if length <= budget || budget < 3 {
        return name.to_owned();
    }
    let kept = budget - 1;
    let head = kept.div_ceil(2);
    let tail = kept - head;
    let mut truncated: String = name.chars().take(head).collect();
    truncated.push(ELLIPSIS);
    truncated.extend(name.chars().skip(length - tail));
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffix_is_split_off() {
        assert_eq!(split_suffix("checkout.diorama.yaml"), ("checkout".into(), ".diorama.yaml".into()));
        assert_eq!(split_suffix("x.yaml"), ("x".into(), ".yaml".into()));
        assert_eq!(split_suffix("api.diorama.json"), ("api".into(), ".diorama.json".into()));
        assert_eq!(split_suffix(".hidden"), (".hidden".into(), String::new()));
        assert_eq!(split_suffix("plain"), ("plain".into(), String::new()));
    }

    #[test]
    fn only_unusual_suffixes_get_a_badge() {
        assert_eq!(badge_for(".diorama.yaml"), None);
        assert_eq!(badge_for(".DIORAMA.YAML"), None);
        assert_eq!(badge_for(".diorama.json").as_deref(), Some("json"));
        assert_eq!(badge_for(".yml").as_deref(), Some("yml"));
        assert_eq!(badge_for(""), None);
    }

    #[test]
    fn middle_truncation_keeps_both_ends() {
        assert_eq!(middle_truncate("checkout", 20), "checkout");
        let long = "payments-reconciliation-and-settlement-platform-for-europe-v2";
        let cut = middle_truncate(long, 21);
        assert_eq!(cut.chars().count(), 21);
        assert!(cut.starts_with("payments-r"));
        assert!(cut.ends_with("europe-v2"));
        assert!(cut.contains(ELLIPSIS));
        assert_eq!(middle_truncate("abcdef", 2), "abcdef");
    }

    #[test]
    fn the_six_examples_fit_the_default_sidebar() {
        for name in ["bank-branch", "checkout", "event-driven", "kubernetes-platform", "minimal", "ml-pipeline"] {
            assert!(name.chars().count() <= label_budget(SIDEBAR_DEFAULT_PX, 0), "{name}");
        }
    }
}
