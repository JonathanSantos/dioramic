//! YAML editor: a transparent `<textarea>` over a highlighted copy of the text, with a gutter,
//! diagnostic lenses and YAML-aware Enter/Tab. The textarea stays the source of truth, so native
//! undo, selection, IME and accessibility all keep working.
//!
//! The highlighted copy is measured after every render (row tops and heights, where each marked
//! line ends), so the gutter, the active line and the lenses follow soft-wrapped lines too.

use std::collections::BTreeMap;

use dioramic_desktop_ipc::Severity;
use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlDocument, HtmlElement, HtmlTextAreaElement, KeyboardEvent};

use crate::components::diagnostics::DiagnosticsPanel;
use crate::components::sidebar::{middle_truncate, split_suffix};
use crate::dom::on_resize;
use crate::highlight::{LineMark, escape, render_html};
use crate::icons::{Icon, IconView};
use crate::state::AppState;
use crate::text_pos::{indent_for_newline, line_col_at, line_range};

/// Must match `--code-line` in the stylesheet.
const LINE_HEIGHT_PX: f64 = 20.0;
/// Must match the top padding of `.hl` / `.code-input`.
const CODE_TOP_PX: f64 = 14.0;
/// Longest stem shown in the file tab before it is cut in the middle (characters).
const TAB_STEM_BUDGET: usize = 26;
/// Indentation inserted by Tab.
const INDENT: &str = "  ";
/// Gap between the end of a line and its lens (px).
const LENS_GAP_PX: f64 = 22.0;
/// Room kept free at the right edge of the code (px).
const LENS_EDGE_PX: f64 = 14.0;
/// A lens shorter than this after the text is anchored to the right edge instead (px).
const LENS_MIN_INLINE_PX: f64 = 150.0;
/// Rough width of a lens character (11 px Inter) and its padding, to guess whether it fits.
const LENS_CHAR_PX: f64 = 6.1;
/// Horizontal padding of a lens plus room for "+N" (px).
const LENS_CHROME_PX: f64 = 22.0;
/// An anchored lens covers at most this share of the code width.
const LENS_ANCHORED_SHARE: f64 = 0.62;

/// Middle column: file tab, editor, problems.
#[component]
pub fn EditorPane() -> impl IntoView {
    let state = expect_context::<AppState>();
    let stem = move || {
        state.open_path.with(|path| {
            let name = path.as_deref().and_then(|p| p.rsplit(['/', '\\']).next()).unwrap_or("");
            middle_truncate(&split_suffix(name).0, TAB_STEM_BUDGET)
        })
    };
    let full_name = move || {
        state.open_path.with(|path| path.as_deref().and_then(|p| p.rsplit(['/', '\\']).next()).unwrap_or("").to_owned())
    };
    let dirty = move || state.is_dirty();
    let wrap = state.layout.soft_wrap;

    view! {
        <section class="pane editor-pane">
            <div class="pane-head">
                <div class="file-tab" title=full_name>
                    <IconView icon=Icon::Cube class="spec-icon" />
                    <span class="file-name">{stem}</span>
                    <span class="save-state" class:dirty=dirty>
                        <span class="dirty-dot" class:on=dirty></span>
                        {move || if dirty() { "Edited" } else { "Saved" }}
                    </span>
                </div>
                <div class="spacer"></div>
                <button
                    class="icon-btn small"
                    class:active=move || wrap.get()
                    aria-pressed=move || wrap.get().to_string()
                    aria-label="Soft wrap"
                    data-tip="Soft wrap"
                    data-kbd="⌥ Z"
                    on:click=move |_| state.layout.toggle_soft_wrap()
                >
                    <IconView icon=Icon::Wrap />
                </button>
                <button
                    class="btn ghost small"
                    disabled=move || !dirty()
                    data-tip="Save"
                    data-kbd="⌘ S"
                    on:click=move |_| state.save()
                >
                    "Save"
                </button>
            </div>
            <Show when=move || state.disk_conflict.get()>
                <div class="banner">
                    <IconView icon=Icon::Reload />
                    <span>"This file changed on disk."</span>
                    <div class="spacer"></div>
                    <button class="btn subtle small" on:click=move |_| state.reload_from_disk()>
                        "Load disk version"
                    </button>
                    <button class="btn ghost small" on:click=move |_| state.disk_conflict.set(false)>
                        "Keep mine"
                    </button>
                </div>
            </Show>
            <CodeEditor />
            <DiagnosticsPanel />
        </section>
    }
}

/// Where the highlighted rows ended up after layout.
#[derive(Debug, Clone, Default, PartialEq)]
struct Metrics {
    /// Top and height of every line (px, in text coordinates, top padding included).
    rows: Vec<(f64, f64)>,
    /// End of the text (left, top of its last visual row) of every marked line.
    ends: BTreeMap<u32, (f64, f64)>,
    /// Visible width of the code area (px).
    body_width: f64,
}

impl Metrics {
    fn row(&self, line: u32) -> (f64, f64) {
        let index = line.saturating_sub(1) as usize;
        self.rows
            .get(index)
            .copied()
            .unwrap_or((CODE_TOP_PX + f64::from(line.saturating_sub(1)) * LINE_HEIGHT_PX, LINE_HEIGHT_PX))
    }
}

/// Placement of one lens in the visible code area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LensPlacement {
    /// Right after the text, at `left`, at most `max_width` wide.
    Inline {
        /// Left edge (px from the code area's left).
        left: f64,
        /// Room up to the right edge (px).
        max_width: f64,
    },
    /// The text runs too far: pinned to the right edge, over the text, at most `max_width` wide.
    Anchored {
        /// Width cap (px).
        max_width: f64,
    },
}

/// Puts a lens after the line's text when it fits (fully, or at least [`LENS_MIN_INLINE_PX`]),
/// otherwise against the right edge. `text_end` is where the text ends in the visible area.
pub fn place_lens(text_end: f64, natural_width: f64, body_width: f64) -> LensPlacement {
    let left = text_end + LENS_GAP_PX;
    let room = body_width - left - LENS_EDGE_PX;
    if room >= natural_width.min(LENS_MIN_INLINE_PX) {
        LensPlacement::Inline { left, max_width: room }
    } else {
        LensPlacement::Anchored { max_width: (body_width * LENS_ANCHORED_SHARE).min(natural_width) }
    }
}

fn lens_natural_width(mark: &LineMark) -> f64 {
    #[allow(clippy::cast_precision_loss)] // display estimate
    let characters = mark.message.chars().count() as f64;
    characters * LENS_CHAR_PX + LENS_CHROME_PX + if mark.others > 0 { 24.0 } else { 0.0 }
}

fn severity_class(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Hint => "hint",
    }
}

/// HTML of the lens layer for the current scroll position.
fn lenses_html(marks: &BTreeMap<u32, LineMark>, metrics: &Metrics, scroll_left: f64) -> String {
    let mut html = String::new();
    for (line, mark) in marks {
        let Some(&(end_left, end_top)) = metrics.ends.get(line) else { continue };
        let natural = lens_natural_width(mark);
        let placement = place_lens(end_left - scroll_left, natural, metrics.body_width);
        let (position, cut) = match placement {
            LensPlacement::Inline { left, max_width } => {
                (format!("left:{left:.1}px;max-width:{max_width:.1}px"), max_width < natural)
            }
            LensPlacement::Anchored { max_width } => {
                (format!("right:{LENS_EDGE_PX}px;max-width:{max_width:.1}px"), max_width < natural)
            }
        };
        let anchored = matches!(placement, LensPlacement::Anchored { .. });
        let more =
            if mark.others > 0 { format!("<span class=\"lens-more\">+{}</span>", mark.others) } else { String::new() };
        let tip = if cut || anchored { format!(" data-tip=\"{}\"", escape(&mark.message)) } else { String::new() };
        html.push_str(&format!(
            "<div class=\"lens {}{}\" style=\"top:{end_top:.1}px;{position}\"{tip}><span class=\"lens-text\">{}</span>{more}</div>",
            severity_class(mark.severity),
            if anchored { " anchored" } else { "" },
            escape(&mark.message),
        ));
    }
    html
}

#[component]
fn CodeEditor() -> impl IntoView {
    let state = expect_context::<AppState>();
    let textarea: NodeRef<html::Textarea> = NodeRef::new();
    let highlight_layer: NodeRef<html::Div> = NodeRef::new();
    let body: NodeRef<html::Div> = NodeRef::new();
    let scroll = RwSignal::new((0_i32, 0_i32));
    let metrics = RwSignal::new(Metrics::default());
    let overflow = RwSignal::new((false, false));
    let wrap = state.layout.soft_wrap;

    let marks = Memo::new(move |_| state.line_marks());
    let highlighted = Memo::new(move |_| marks.with(|marks| state.text.with(|text| render_html(text, marks))));

    let measure = move || {
        let (Some(layer), Some(input)) = (highlight_layer.get_untracked(), textarea.get_untracked()) else { return };
        let soft = wrap.get_untracked();
        let style = HtmlElement::style(&layer);
        if soft {
            let _ = style.set_property("width", &format!("{}px", input.client_width()));
        } else {
            let _ = style.remove_property("width");
        }
        metrics.set(measure_rows(&layer, f64::from(input.client_width())));
        overflow.set(horizontal_overflow(&input, soft));
    };
    // After every render of the highlighted text (and on wrap changes / resizes): measure the rows.
    Effect::new(move |_| {
        highlighted.track();
        wrap.track();
        request_animation_frame(measure);
    });
    on_resize(body, measure);

    let gutter = Memo::new(move |_| {
        let marks = marks.get();
        let lines = state.text.with(|text| text.split('\n').count());
        metrics.with(|metrics| gutter_html(lines, &marks, metrics))
    });
    let lenses = move || {
        let (left, _) = scroll.get();
        marks.with(|marks| metrics.with(|metrics| lenses_html(marks, metrics, f64::from(left))))
    };

    // External changes (open, reload, pin, AI edits) flow into the textarea; typing does not loop back.
    Effect::new(move |_| {
        let text = state.text.get();
        if let Some(element) = textarea.get()
            && element.value() != text
        {
            element.set_value(&text);
        }
    });

    Effect::new(move |_| {
        let Some(line) = state.jump_to_line.get() else { return };
        if let Some(element) = textarea.get() {
            let (top, _) = metrics.with_untracked(|metrics| metrics.row(line));
            reveal_line(&element, line, top);
            update_cursor(state, &element);
        }
        state.jump_to_line.set(None);
    });

    let translate = move || {
        let (left, top) = scroll.get();
        format!("translate({}px, {}px)", -left, -top)
    };
    let vertical = move || format!("translateY({}px)", -scroll.get().1);
    let active_line = move || {
        let (line, _) = state.cursor.get();
        let (top, height) = metrics.with(|metrics| metrics.row(line));
        format!("transform: translateY({:.1}px); height: {height:.1}px", top - f64::from(scroll.get().1))
    };
    let on_scroll = move |event: web_sys::Event| {
        if let Some(element) = event_textarea(&event) {
            scroll.set((element.scroll_left(), element.scroll_top()));
            overflow.set(horizontal_overflow(&element, wrap.get_untracked()));
        }
    };

    view! {
        <div class="code" class:wrap=move || wrap.get()>
            <div class="gutter">
                <div class="gutter-inner" style:transform=vertical inner_html=move || gutter.get()></div>
            </div>
            <div
                class="code-body"
                class:fade-left=move || overflow.get().0
                class:fade-right=move || overflow.get().1
                node_ref=body
            >
                <div class="active-line" style=active_line></div>
                <div
                    class="hl"
                    aria-hidden="true"
                    node_ref=highlight_layer
                    style:transform=translate
                    inner_html=move || highlighted.get()
                ></div>
                <textarea
                    node_ref=textarea
                    class="code-input"
                    spellcheck="false"
                    autocomplete="off"
                    autocapitalize="off"
                    wrap=move || if wrap.get() { "soft" } else { "off" }
                    aria-label="Diagram spec (YAML)"
                    on:input=move |event| state.edit(event_target_value(&event))
                    on:scroll=on_scroll
                    on:keydown=move |event| on_editor_key(&event)
                    on:keyup=move |event| {
                        if let Some(element) = event_textarea(&event) {
                            update_cursor(state, &element);
                        }
                    }
                    on:click=move |event| {
                        if let Some(element) = event_textarea(&event) {
                            update_cursor(state, &element);
                        }
                    }
                ></textarea>
                <div class="edge-fade left" aria-hidden="true"></div>
                <div class="edge-fade right" aria-hidden="true"></div>
                <div class="lenses" style:transform=vertical inner_html=lenses></div>
            </div>
        </div>
    }
}

/// Reads row geometry from the highlighted layer.
fn measure_rows(layer: &HtmlElement, body_width: f64) -> Metrics {
    let children = layer.children();
    let mut rows = Vec::with_capacity(children.length() as usize);
    let mut ends = BTreeMap::new();
    for index in 0..children.length() {
        let Some(row) = children.item(index).and_then(|row| row.dyn_into::<HtmlElement>().ok()) else { continue };
        rows.push((f64::from(row.offset_top()), f64::from(row.offset_height())));
        let end = row.query_selector(".eol").ok().flatten().and_then(|end| end.dyn_into::<HtmlElement>().ok());
        if let Some(end) = end {
            ends.insert(index + 1, (f64::from(end.offset_left()), f64::from(end.offset_top())));
        }
    }
    Metrics { rows, ends, body_width }
}

/// (content hidden on the left, content hidden on the right) of a non-wrapping editor.
fn horizontal_overflow(element: &HtmlTextAreaElement, soft_wrap: bool) -> (bool, bool) {
    if soft_wrap {
        return (false, false);
    }
    let left = element.scroll_left();
    let hidden_right = element.scroll_width() - element.client_width() - left;
    (left > 0, hidden_right > 1)
}

/// Line numbers; a soft-wrapped line keeps its number on its first visual row only.
fn gutter_html(lines: usize, marks: &BTreeMap<u32, LineMark>, metrics: &Metrics) -> String {
    let mut html = String::with_capacity(lines * 48);
    for number in 1..=lines {
        let Ok(line) = u32::try_from(number) else { break };
        let class = marks.get(&line).map_or("gl", |mark| match mark.severity {
            Severity::Error => "gl has-error",
            Severity::Warning => "gl has-warning",
            Severity::Hint => "gl has-hint",
        });
        let (_, height) = metrics.row(line);
        if (height - LINE_HEIGHT_PX).abs() < 0.5 {
            html.push_str(&format!("<div class=\"{class}\">{number}</div>"));
        } else {
            html.push_str(&format!("<div class=\"{class}\" style=\"height:{height:.1}px\">{number}</div>"));
        }
    }
    html
}

fn event_textarea(event: &web_sys::Event) -> Option<HtmlTextAreaElement> {
    event.target()?.dyn_into::<HtmlTextAreaElement>().ok()
}

fn update_cursor(state: AppState, element: &HtmlTextAreaElement) {
    let offset = element.selection_start().ok().flatten().unwrap_or(0);
    let position = line_col_at(&element.value(), offset);
    state.cursor.set(position);
}

/// Selects a line and scrolls it to the upper third of the editor.
fn reveal_line(element: &HtmlTextAreaElement, line: u32, top: f64) {
    let (start, end) = line_range(&element.value(), line);
    let _ = element.focus();
    let _ = element.set_selection_range(start, end);
    let target = top - CODE_TOP_PX - f64::from(element.client_height()) / 3.0;
    // Pixel offsets fit comfortably in i32.
    #[allow(clippy::cast_possible_truncation)]
    element.set_scroll_top(target.max(0.0) as i32);
}

/// Tab indents, Enter keeps (and after `key:` deepens) the indentation.
fn on_editor_key(event: &KeyboardEvent) {
    if event.meta_key() || event.ctrl_key() || event.alt_key() {
        return;
    }
    let Some(element) = event.target().and_then(|target| target.dyn_into::<HtmlTextAreaElement>().ok()) else {
        return;
    };
    let insertion = match event.key().as_str() {
        "Tab" if !event.shift_key() => INDENT.to_owned(),
        "Enter" => {
            let offset = element.selection_start().ok().flatten().unwrap_or(0);
            format!("\n{}", indent_for_newline(&element.value(), offset))
        }
        _ => return,
    };
    if insert_text(&insertion) {
        event.prevent_default();
    }
}

/// Inserts text at the caret through the browser's editing command, which keeps native undo.
fn insert_text(text: &str) -> bool {
    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.dyn_into::<HtmlDocument>().ok())
        .and_then(|document| document.exec_command_with_show_ui_and_value("insertText", false, text).ok())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_lines_keep_the_lens_after_the_text() {
        let placement = place_lens(200.0, 180.0, 600.0);
        assert_eq!(placement, LensPlacement::Inline { left: 222.0, max_width: 600.0 - 222.0 - LENS_EDGE_PX });
    }

    #[test]
    fn some_room_gives_a_truncated_inline_lens() {
        let LensPlacement::Inline { max_width, .. } = place_lens(380.0, 400.0, 600.0) else {
            panic!("expected inline");
        };
        assert!((LENS_MIN_INLINE_PX..400.0).contains(&max_width));
    }

    #[test]
    fn long_lines_anchor_the_lens_to_the_right_edge() {
        assert_eq!(place_lens(560.0, 200.0, 600.0), LensPlacement::Anchored { max_width: 200.0 });
        assert_eq!(place_lens(900.0, 900.0, 600.0), LensPlacement::Anchored { max_width: 600.0 * LENS_ANCHORED_SHARE });
    }

    #[test]
    fn rows_fall_back_to_fixed_lines_before_measuring() {
        let metrics = Metrics::default();
        assert_eq!(metrics.row(1), (CODE_TOP_PX, LINE_HEIGHT_PX));
        assert_eq!(metrics.row(3), (CODE_TOP_PX + 2.0 * LINE_HEIGHT_PX, LINE_HEIGHT_PX));
    }
}
