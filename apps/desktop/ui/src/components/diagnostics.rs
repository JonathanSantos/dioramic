//! Problems panel under the editor. Each row reads top to bottom: the message (full width), a line
//! of metadata (code · path · line) and the hint. Clicking a row reveals the line and highlights
//! the element. The panel opens by itself only when there is an error.

use dioramic_desktop_ipc::{DiagnosticView, Severity};
use leptos::html;
use leptos::prelude::*;

use crate::components::preview::plural;
use crate::components::splitter::{Axis, Splitter};
use crate::icons::{Icon, IconView};
use crate::layout::{PROBLEMS_DEFAULT_SHARE, clamp_problems, problems_height_at};
use crate::state::AppState;

/// Spec keys worth setting in code font when a message mentions `key: value`.
const SPEC_KEYS: [&str; 22] = [
    "diorama",
    "title",
    "subtitle",
    "theme",
    "view",
    "rotation",
    "layout",
    "density",
    "route",
    "at",
    "id",
    "icon",
    "label",
    "zone",
    "zones",
    "nodes",
    "edges",
    "color",
    "style",
    "animation",
    "preset",
    "note",
];

fn severity_icon(severity: Severity) -> Icon {
    match severity {
        Severity::Error => Icon::Error,
        Severity::Warning => Icon::Warning,
        Severity::Hint => Icon::Hint,
    }
}

fn severity_class(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Hint => "hint",
    }
}

fn severity_noun(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Hint => "hint",
    }
}

/// A piece of a message: prose, or a spec token shown in code font.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessagePart {
    /// Plain words.
    Text(String),
    /// `'db'`, `view.rotation: auto`, `edges[3]`, `{ id: api }`.
    Code(String),
}

/// Splits a diagnostic message or hint into prose and spec tokens: quoted text (`'x'`, `"x"`,
/// `` `x` ``), flow maps (`{ … }`), paths (`edges[3].from`) and `key: value` pairs of spec keys.
pub fn message_parts(text: &str) -> Vec<MessagePart> {
    let mut parts = Vec::new();
    let mut prose = String::new();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some((code, after)) = leading_code(rest, prose.is_empty() || prose.ends_with([' ', '('])) {
            if !prose.is_empty() {
                parts.push(MessagePart::Text(std::mem::take(&mut prose)));
            }
            parts.push(MessagePart::Code(code.to_owned()));
            rest = after;
        } else {
            let mut characters = rest.chars();
            if let Some(character) = characters.next() {
                prose.push(character);
            }
            rest = characters.as_str();
        }
    }
    if !prose.is_empty() {
        parts.push(MessagePart::Text(prose));
    }
    parts
}

/// The code token at the start of `text`, if any, and what follows it. Tokens only start at a word
/// boundary, so apostrophes inside words ("don't") stay prose.
fn leading_code(text: &str, at_boundary: bool) -> Option<(&str, &str)> {
    if !at_boundary {
        return None;
    }
    let first = text.chars().next()?;
    match first {
        '\'' | '"' | '`' => {
            let close = text[1..].find(first)? + 1;
            let token = if first == '`' { &text[1..close] } else { &text[..=close] };
            Some((token, &text[close + 1..]))
        }
        '{' => {
            let close = text.find('}')?;
            Some((&text[..=close], &text[close + 1..]))
        }
        _ => path_or_pair(text),
    }
}

/// `edges[3].from`, `view.rotation: auto`, `at:`, `density: spaced`.
fn path_or_pair(text: &str) -> Option<(&str, &str)> {
    let word_end =
        text.find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '[' | ']'))).unwrap_or(text.len());
    let word = &text[..word_end];
    if word.is_empty() || !word.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let is_path = word.contains('[') || (word.contains('.') && !word.ends_with('.'));
    let first_key = word.split(['.', '[']).next().unwrap_or(word);
    let is_spec_key = is_path || SPEC_KEYS.contains(&first_key);
    let after_word = &text[word_end..];
    if is_spec_key && let Some(after_colon) = after_word.strip_prefix(':') {
        let value = after_colon.strip_prefix(' ').map(leading_value).unwrap_or_default();
        let end = if value.is_empty() { word_end + 1 } else { word_end + 2 + value.len() };
        return Some((&text[..end], &text[end..]));
    }
    is_path.then_some((word, after_word))
}

/// The value word after `key: ` (stops at spaces and closing punctuation).
fn leading_value(text: &str) -> &str {
    let end = text.find(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ')')).unwrap_or(text.len());
    text[..end].trim_end_matches(['.', '?'])
}

fn render_message(text: String) -> impl IntoView {
    message_parts(&text)
        .into_iter()
        .map(|part| match part {
            MessagePart::Text(text) => text.into_any(),
            MessagePart::Code(code) => view! { <code class="inline-code">{code}</code> }.into_any(),
        })
        .collect_view()
}

/// Collapsible list of diagnostics with a draggable top edge.
#[component]
pub fn DiagnosticsPanel() -> impl IntoView {
    let state = expect_context::<AppState>();
    let panel: NodeRef<html::Div> = NodeRef::new();
    let diagnostics = Memo::new(move |_| state.diagnostics());
    let count =
        move |severity: Severity| diagnostics.with(|list| list.iter().filter(|d| d.severity == severity).count());
    let has_errors = Memo::new(move |_| count(Severity::Error) > 0);
    let expanded = RwSignal::new(false);
    // Errors open the panel; it closes again when they are fixed unless the user opened it.
    let opened_by_user = StoredValue::new(false);
    Effect::new(move |_| {
        if has_errors.get() {
            expanded.set(true);
        } else if !opened_by_user.get_value() {
            expanded.set(false);
        }
    });
    let toggle = move |_| {
        let open = !expanded.get_untracked();
        opened_by_user.set_value(open);
        expanded.set(open);
    };
    let height_style = move || {
        if !expanded.get() {
            return String::new();
        }
        match state.layout.problems_height.get() {
            Some(height) => format!("height: {height:.0}px"),
            None => format!("max-height: {:.0}%", PROBLEMS_DEFAULT_SHARE * 100.0),
        }
    };
    let column_bounds = move || {
        let column = panel.get_untracked()?.parent_element()?.get_bounding_client_rect();
        Some((column.top(), column.bottom()))
    };
    let current_height = move || panel.get_untracked().map_or(0.0, |element| f64::from(element.offset_height()));

    view! {
        <div class="problems" class:collapsed=move || !expanded.get() node_ref=panel style=height_style>
            <Show when=move || expanded.get()>
                <Splitter
                    axis=Axis::Rows
                    label="Resize problems"
                    on_drag=move |y: f64| {
                        if let Some((top, bottom)) = column_bounds() {
                            state.layout.set_problems_height(Some(problems_height_at(y, top, bottom)));
                        }
                    }
                    on_step=move |step: f64| {
                        if let Some((top, bottom)) = column_bounds() {
                            let height = clamp_problems(current_height() - step, bottom - top);
                            state.layout.set_problems_height(Some(height));
                        }
                    }
                    on_reset=move |()| state.layout.set_problems_height(None)
                />
            </Show>
            <button
                class="problems-head"
                aria-expanded=move || expanded.get().to_string()
                on:click=toggle
            >
                <IconView icon=Icon::Chevron class="chevron" />
                <span class="eyebrow">"Problems"</span>
                {[Severity::Error, Severity::Warning, Severity::Hint]
                    .into_iter()
                    .map(|severity| {
                        let label = move || plural(count(severity), severity_noun(severity));
                        view! {
                            <span
                                class=format!("count-chip {}", severity_class(severity))
                                class:zero=move || count(severity) == 0
                                aria-label=label
                                title=label
                            >
                                <IconView icon=severity_icon(severity) />
                                {move || count(severity)}
                            </span>
                        }
                    })
                    .collect_view()}
            </button>
            <div class="problems-list">
                {move || {
                    let list = diagnostics.get();
                    if list.is_empty() {
                        view! {
                            <div class="problems-empty">
                                <IconView icon=Icon::Check />
                                "No problems in this spec"
                            </div>
                        }
                            .into_any()
                    } else {
                        list.into_iter()
                            .enumerate()
                            .map(|(index, diagnostic)| diagnostic_row(state, index, diagnostic))
                            .collect_view()
                            .into_any()
                    }
                }}
            </div>
        </div>
    }
}

fn diagnostic_row(state: AppState, index: usize, diagnostic: DiagnosticView) -> impl IntoView {
    let line = state.line_of(&diagnostic);
    let mut meta = vec![diagnostic.code.clone()];
    if !diagnostic.path.is_empty() {
        meta.push(diagnostic.path.clone());
    }
    if let Some(line) = line {
        meta.push(format!("Ln {line}"));
    }
    let class = severity_class(diagnostic.severity);
    view! {
        <button
            class=format!("problem {class}")
            class:selected=move || state.selected_diagnostic.get() == Some(index)
            on:click=move |_| state.select_diagnostic(index)
        >
            <IconView icon=severity_icon(diagnostic.severity) class="problem-icon" />
            <span class="problem-main">
                <span class="problem-message">{render_message(diagnostic.message)}</span>
                <span class="problem-meta">
                    {meta.into_iter().map(|part| view! { <span>{part}</span> }).collect_view()}
                </span>
                {diagnostic.hint.map(|hint| view! { <span class="problem-hint">{render_message(hint)}</span> })}
            </span>
        </button>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(text: &str) -> MessagePart {
        MessagePart::Code(text.to_owned())
    }

    fn prose(text: &str) -> MessagePart {
        MessagePart::Text(text.to_owned())
    }

    #[test]
    fn quoted_names_become_code() {
        assert_eq!(message_parts("unknown icon 'loadbalancr'"), vec![prose("unknown icon "), code("'loadbalancr'")]);
        assert_eq!(message_parts("did you mean 'db'?"), vec![prose("did you mean "), code("'db'"), prose("?")]);
    }

    #[test]
    fn spec_pairs_and_bare_keys_become_code() {
        assert_eq!(
            message_parts("try route: orthogonal, view.rotation: auto, or move the nodes with at:"),
            vec![
                prose("try "),
                code("route: orthogonal"),
                prose(", "),
                code("view.rotation: auto"),
                prose(", or move the nodes with "),
                code("at:"),
            ]
        );
    }

    #[test]
    fn paths_backticks_and_flow_maps_become_code() {
        assert_eq!(
            message_parts("edge edges[3].from points nowhere"),
            vec![prose("edge "), code("edges[3].from"), prose(" points nowhere")]
        );
        assert_eq!(
            message_parts("start the spec with `diorama: 1`"),
            vec![prose("start the spec with "), code("diorama: 1")]
        );
        assert_eq!(
            message_parts("add at least one node: { id: api, icon: server }"),
            vec![prose("add at least one node: "), code("{ id: api, icon: server }")]
        );
    }

    #[test]
    fn plain_english_stays_prose() {
        assert_eq!(message_parts("use one of: blue, red"), vec![prose("use one of: blue, red")]);
        assert_eq!(message_parts("don't do that. e.g. this"), vec![prose("don't do that. e.g. this")]);
        assert_eq!(message_parts(""), Vec::<MessagePart>::new());
    }
}
