//! Live preview: the engine's SVG on an isometric drafting table, with zoom (pinch / ⌘-wheel),
//! pan (drag / two-finger scroll), fit, node notes on hover and diagnostic highlighting.

use dioramic_desktop_ipc::{NodeNote, PreviewStatus, Severity};
use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use web_sys::{AddEventListenerOptions, Element, PointerEvent, SvgsvgElement, WheelEvent};

use crate::components::toolbar::Toolbar;
use crate::dom::on_resize;
use crate::icons::{Icon, IconView};
use crate::state::AppState;

const MIN_ZOOM: f64 = 0.1;
const MAX_ZOOM: f64 = 8.0;
/// Step of the +/− buttons.
const ZOOM_STEP: f64 = 1.25;
/// Wheel delta → zoom factor sensitivity (pinch sends small deltas with ctrlKey).
const WHEEL_ZOOM_SENSITIVITY: f64 = 0.01;
/// Room left around the diagram when fitting (px).
const FIT_MARGIN_PX: f64 = 22.0;
/// Space reserved for the floating toolbar when fitting (px): its top offset, height and a gap.
const FIT_TOP_RESERVE_PX: f64 = 50.0;
/// An edit reframes only when the diagram's width or height changes more than this (15 %).
const EDIT_REFIT_THRESHOLD: f64 = 0.15;
/// Size differences below this are measurement noise (px).
const SIZE_EPSILON_PX: f64 = 0.5;
/// Length of the eased reframe; matches `--reframe` in the stylesheet.
const REFRAME_MS: u64 = 240;

/// How the preview frames the next SVG it receives. Ordered: a stronger pending intent wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FitIntent {
    /// Typing: keep the camera, unless the diagram changed size a lot and the user never moved it.
    Keep,
    /// Same diagram, maybe another size (theme, reload from disk): fit if the size changed.
    IfResized,
    /// Same file, new camera or density (⟲ ⟳, Auto, D): always fit, with a short glide.
    Always,
    /// Another file: fit before the first paint, no glide from the previous diagram's camera.
    Fresh,
}

/// Decides whether a new SVG of size `new` should be reframed. `old` is the size on screen before it.
pub fn needs_refit(intent: FitIntent, old: Option<(f64, f64)>, new: (f64, f64), user_moved: bool) -> bool {
    let Some(old) = old else { return true };
    match intent {
        FitIntent::Fresh | FitIntent::Always => true,
        FitIntent::IfResized => (old.0 - new.0).abs() > SIZE_EPSILON_PX || (old.1 - new.1).abs() > SIZE_EPSILON_PX,
        FitIntent::Keep => !user_moved && relative_change(old, new) > EDIT_REFIT_THRESHOLD,
    }
}

/// Largest relative change of width or height.
fn relative_change(old: (f64, f64), new: (f64, f64)) -> f64 {
    let ratio = |before: f64, after: f64| if before > 0.0 { (after - before).abs() / before } else { 1.0 };
    ratio(old.0, new.0).max(ratio(old.1, new.1))
}
/// Fit never enlarges past this (small specs stay crisp but not huge).
const FIT_MAX_ZOOM: f64 = 1.6;
/// One isometric grid tile of the background (px at zoom 1): 2:1 rhombus of true isometry.
const GRID_TILE_W: f64 = 48.0;
const GRID_TILE_H: f64 = 27.7128;

/// Pan/zoom of the canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Camera {
    x: f64,
    y: f64,
    zoom: f64,
}

impl Camera {
    /// Zooms by `factor` keeping the screen point (`at_x`, `at_y`) fixed.
    fn zoomed_at(self, factor: f64, at_x: f64, at_y: f64) -> Self {
        let zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let applied = zoom / self.zoom;
        Self { x: at_x - (at_x - self.x) * applied, y: at_y - (at_y - self.y) * applied, zoom }
    }

    /// Centers content of `content` size in a `stage` of the given size.
    fn fitted(content: (f64, f64), stage: (f64, f64)) -> Self {
        let available_w = (stage.0 - 2.0 * FIT_MARGIN_PX).max(1.0);
        let available_h = (stage.1 - 2.0 * FIT_MARGIN_PX - FIT_TOP_RESERVE_PX).max(1.0);
        let zoom = (available_w / content.0).min(available_h / content.1).clamp(MIN_ZOOM, FIT_MAX_ZOOM);
        Self {
            x: (stage.0 - content.0 * zoom) / 2.0,
            y: FIT_TOP_RESERVE_PX / 2.0 + (stage.1 - content.1 * zoom) / 2.0,
            zoom,
        }
    }
}

/// Hover card of a node note.
#[derive(Debug, Clone, PartialEq)]
struct HoverNote {
    note: NodeNote,
    x: f64,
    y: f64,
}

/// Right column.
#[component]
pub fn PreviewPane() -> impl IntoView {
    let state = expect_context::<AppState>();
    let stage: NodeRef<html::Div> = NodeRef::new();
    let canvas: NodeRef<html::Div> = NodeRef::new();
    let camera = RwSignal::new(Camera { x: 0.0, y: 0.0, zoom: 1.0 });
    let content_size = RwSignal::new(None::<(f64, f64)>);
    // The user panned or zoomed since the last fit: edits and panel resizes leave the camera alone.
    let user_moved = StoredValue::new(false);
    let unframed = RwSignal::new(false);
    let easing = RwSignal::new(false);
    let easing_timer = StoredValue::new(None::<TimeoutHandle>);
    let drag = StoredValue::new(None::<(f64, f64)>);
    let panning = RwSignal::new(false);
    let hover = RwSignal::new(None::<HoverNote>);

    let fit = move |glide: bool| {
        let (Some(element), Some(content)) = (stage.get_untracked(), content_size.get_untracked()) else { return };
        let size = (f64::from(element.client_width()), f64::from(element.client_height()));
        if size.0 <= 0.0 {
            return;
        }
        if glide {
            easing.set(true);
            if let Some(handle) = easing_timer.get_value() {
                handle.clear();
            }
            let handle =
                set_timeout_with_handle(move || easing.set(false), std::time::Duration::from_millis(REFRAME_MS)).ok();
            easing_timer.set_value(handle);
        }
        camera.set(Camera::fitted(content, size));
        user_moved.set_value(false);
        unframed.set(false);
    };
    let moved_by_user = move || {
        user_moved.set_value(true);
        easing.set(false);
    };

    Effect::new(move |_| state.zoom.set(camera.get().zoom));
    Effect::new(move |previous: Option<()>| {
        state.fit_request.track();
        if previous.is_some() {
            fit(true);
        }
    });
    // After every new SVG: normalise its size to its viewBox, then frame it as the pending intent says.
    Effect::new(move |_| {
        if state.last_svg.with(Option::is_none) {
            return;
        }
        if state.fit_intent.get_value() == FitIntent::Fresh {
            unframed.set(true);
        }
        request_animation_frame(move || {
            let Some(size) = canvas.get_untracked().and_then(|element| measure_svg(&element)) else { return };
            let intent = state.fit_intent.get_value();
            let previous = content_size.get_untracked();
            content_size.set(Some(size));
            state.diagram_size.set(Some(size));
            state.fit_intent.set_value(FitIntent::Keep);
            if needs_refit(intent, previous, size, user_moved.get_value()) {
                fit(intent != FitIntent::Fresh && previous.is_some());
            } else {
                unframed.set(false);
            }
        });
    });
    // Panels dragged or folded, window resized: follow with the fit unless the user framed it by hand.
    on_resize(stage, move || {
        if !user_moved.get_value() {
            fit(false);
        }
    });
    Effect::new(move |_| {
        let target = state.highlighted_target();
        state.last_svg.track();
        if let Some(element) = canvas.get() {
            highlight(&element, target.as_deref());
        }
    });
    install_wheel(stage, camera, moved_by_user);

    let on_pointer_down = move |event: PointerEvent| {
        if event.button() != 0 && event.button() != 1 {
            return;
        }
        drag.set_value(Some((f64::from(event.client_x()), f64::from(event.client_y()))));
        panning.set(true);
        hover.set(None);
        if let Some(element) = stage.get_untracked() {
            let _ = element.set_pointer_capture(event.pointer_id());
        }
    };
    let on_pointer_move = move |event: PointerEvent| {
        let (x, y) = (f64::from(event.client_x()), f64::from(event.client_y()));
        if let Some((last_x, last_y)) = drag.get_value() {
            moved_by_user();
            camera.update(|camera| {
                camera.x += x - last_x;
                camera.y += y - last_y;
            });
            drag.set_value(Some((x, y)));
            return;
        }
        hover.set(note_under_pointer(state, &event, stage));
    };
    let end_drag = move |_: PointerEvent| {
        drag.set_value(None);
        panning.set(false);
    };

    let grid_style = move || {
        let Camera { x, y, zoom } = camera.get();
        format!(
            "background-size: {:.2}px {:.2}px; background-position: {:.1}px {:.1}px",
            GRID_TILE_W * zoom,
            GRID_TILE_H * zoom,
            x,
            y
        )
    };
    let canvas_transform = move || {
        let Camera { x, y, zoom } = camera.get();
        format!("translate({x:.1}px, {y:.1}px) scale({zoom:.4})")
    };
    let is_stale = move || {
        state.last_svg.with(Option::is_some)
            && state.preview.with(|p| p.as_ref().is_some_and(|p| !matches!(p.status, PreviewStatus::Ready)))
    };

    let zoom_step = move |factor: f64| {
        moved_by_user();
        zoom_by(stage, camera, factor);
    };

    view! {
        <section class="pane preview-pane">
            <div
                class="stage"
                class:panning=move || panning.get()
                class:easing=move || easing.get()
                node_ref=stage
                style=grid_style
                on:pointerdown=on_pointer_down
                on:pointermove=on_pointer_move
                on:pointerup=end_drag
                on:pointercancel=end_drag
                on:pointerleave=move |_| hover.set(None)
                on:dblclick=move |_| fit(true)
            >
                <div class="render-progress" class:on=move || state.rendering.get()></div>
                <div
                    class="canvas"
                    class:stale=is_stale
                    class:unframed=move || unframed.get()
                    node_ref=canvas
                    style:transform=canvas_transform
                    inner_html=move || state.last_svg.get().unwrap_or_default()
                ></div>
                <EmptyState />
                <StaleNotice />
                {move || hover.get().map(|hover| view! { <NoteCard hover=hover /> })}
            </div>
            <Toolbar />
            <div class="zoom-controls">
                <button
                    class="icon-btn small"
                    aria-label="Zoom out"
                    data-tip="Zoom out"
                    data-kbd="⌘ −"
                    on:click=move |_| zoom_step(1.0 / ZOOM_STEP)
                >
                    <IconView icon=Icon::Minus />
                </button>
                <button
                    class="zoom-readout"
                    aria-label="Zoom level; click to fit"
                    data-tip="Fit to view"
                    data-kbd="0"
                    on:click=move |_| fit(true)
                >
                    {move || format!("{:.0}%", camera.get().zoom * 100.0)}
                </button>
                <button
                    class="icon-btn small"
                    aria-label="Zoom in"
                    data-tip="Zoom in"
                    data-kbd="⌘ +"
                    on:click=move |_| zoom_step(ZOOM_STEP)
                >
                    <IconView icon=Icon::Plus />
                </button>
                <span class="zc-sep"></span>
                <button
                    class="icon-btn small"
                    aria-label="Fit to view"
                    data-tip="Fit to view"
                    data-kbd="0"
                    on:click=move |_| fit(true)
                >
                    <IconView icon=Icon::Fit />
                </button>
            </div>
        </section>
    }
}

/// Sets the SVG's CSS size to its viewBox so zoom 1 = 1 SVG unit per px; returns that size.
fn measure_svg(canvas: &Element) -> Option<(f64, f64)> {
    let svg = canvas.query_selector("svg").ok()??.dyn_into::<SvgsvgElement>().ok()?;
    let view_box = svg.view_box().base_val()?;
    let (width, height) = (f64::from(view_box.width()), f64::from(view_box.height()));
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let _ = svg.set_attribute("width", &format!("{width}"));
    let _ = svg.set_attribute("height", &format!("{height}"));
    Some((width, height))
}

fn highlight(canvas: &Element, target: Option<&str>) {
    const CLASS: &str = "is-highlighted";
    if let Ok(previous) = canvas.query_selector_all(&format!(".{CLASS}")) {
        for index in 0..previous.length() {
            if let Some(element) = previous.item(index).and_then(|node| node.dyn_into::<Element>().ok()) {
                let _ = element.class_list().remove_1(CLASS);
            }
        }
    }
    let Some(target) = target else { return };
    let selector = format!("[data-id=\"{}\"]", target.replace('\\', "\\\\").replace('"', "\\\""));
    if let Ok(matches) = canvas.query_selector_all(&selector) {
        for index in 0..matches.length() {
            if let Some(element) = matches.item(index).and_then(|node| node.dyn_into::<Element>().ok()) {
                let _ = element.class_list().add_1(CLASS);
            }
        }
    }
}

fn note_under_pointer(state: AppState, event: &PointerEvent, stage: NodeRef<html::Div>) -> Option<HoverNote> {
    let target = event.target()?.dyn_into::<Element>().ok()?;
    let node = target.closest("[data-role=\"node\"], [data-role=\"node-label\"]").ok()??;
    let id = node.get_attribute("data-id")?;
    let note =
        state.preview.with_untracked(|preview| preview.as_ref()?.notes.iter().find(|note| note.id == id).cloned())?;
    let rect = stage.get_untracked()?.get_bounding_client_rect();
    Some(HoverNote { note, x: f64::from(event.client_x()) - rect.left(), y: f64::from(event.client_y()) - rect.top() })
}

fn zoom_by(stage: NodeRef<html::Div>, camera: RwSignal<Camera>, factor: f64) {
    let Some(element) = stage.get_untracked() else { return };
    let (center_x, center_y) = (f64::from(element.client_width()) / 2.0, f64::from(element.client_height()) / 2.0);
    camera.update(|camera| *camera = camera.zoomed_at(factor, center_x, center_y));
}

/// Wheel needs a non-passive listener to stop the webview from scrolling or zooming the page.
fn install_wheel(stage: NodeRef<html::Div>, camera: RwSignal<Camera>, moved_by_user: impl Fn() + Copy + 'static) {
    let listener = StoredValue::new_local(None::<Closure<dyn FnMut(WheelEvent)>>);
    Effect::new(move |_| {
        let Some(element) = stage.get() else { return };
        let closure = Closure::<dyn FnMut(WheelEvent)>::new(move |event: WheelEvent| {
            event.prevent_default();
            moved_by_user();
            let rect = element_rect(&event);
            let (delta_x, delta_y) = (event.delta_x(), event.delta_y());
            if event.ctrl_key() || event.meta_key() {
                let factor = (-delta_y * WHEEL_ZOOM_SENSITIVITY).exp();
                camera.update(|camera| *camera = camera.zoomed_at(factor, rect.0, rect.1));
            } else {
                camera.update(|camera| {
                    camera.x -= delta_x;
                    camera.y -= delta_y;
                });
            }
        });
        let options = AddEventListenerOptions::new();
        options.set_passive(false);
        let _ = element.add_event_listener_with_callback_and_add_event_listener_options(
            "wheel",
            closure.as_ref().unchecked_ref(),
            &options,
        );
        listener.set_value(Some(closure));
    });
}

/// Pointer position relative to the element under the wheel event.
fn element_rect(event: &WheelEvent) -> (f64, f64) {
    event
        .current_target()
        .and_then(|target| target.dyn_into::<Element>().ok())
        .map(|element| {
            let rect = element.get_bounding_client_rect();
            (f64::from(event.client_x()) - rect.left(), f64::from(event.client_y()) - rect.top())
        })
        .unwrap_or_default()
}

#[component]
fn NoteCard(hover: HoverNote) -> impl IntoView {
    view! {
        <div class="note-card" style:left=format!("{}px", hover.x + 16.0) style:top=format!("{}px", hover.y + 16.0)>
            <div class="note-title">{hover.note.title}</div>
            <div class="note-body">{hover.note.note}</div>
        </div>
    }
}

/// What the stage shows when there is no diagram to draw.
#[component]
fn EmptyState() -> impl IntoView {
    let state = expect_context::<AppState>();
    let has_svg = move || state.last_svg.with(Option::is_some);
    let errors = move || state.severity_count(Severity::Error);

    move || {
        if has_svg() {
            return None;
        }
        let view = if let Some(reason) = state.engine_unavailable() {
            view! {
                <div class="empty-state">
                    <div class="pending-art" aria-hidden="true">
                        <img class="pending-mark only-light" src="dioramic-mark.svg" alt="" draggable="false" />
                        <img class="pending-mark only-dark" src="dioramic-mark-dark.svg" alt="" draggable="false" />
                        <div class="pending-shadow"></div>
                    </div>
                    <h2 class="empty-title">"No renderer here"</h2>
                    <p class="empty-body">
                        "This window cannot draw the diagram. Keep editing: open the spec in the Dioramic app and it appears here, live."
                    </p>
                    <div class="empty-meta">
                        <SyntaxStatus />
                        <span class="reason-chip" title="Engine status">
                            <span class="reason-dot"></span>
                            {reason}
                        </span>
                    </div>
                </div>
            }
            .into_any()
        } else if errors() > 0 {
            view! {
                <div class="empty-state">
                    <div class="empty-icon error"><IconView icon=Icon::Error /></div>
                    <h2 class="empty-title">"This spec needs a fix"</h2>
                    <p class="empty-body">
                        {move || {
                            let count = errors();
                            if count == 1 {
                                "1 error stops the render. Pick it below the editor to jump to it.".to_owned()
                            } else {
                                format!("{count} errors stop the render. Pick one below the editor to jump to it.")
                            }
                        }}
                    </p>
                    <button class="btn subtle" on:click=move |_| state.select_diagnostic(0)>"Show first error"</button>
                </div>
            }
            .into_any()
        } else {
            view! {
                <div class="empty-state quiet">
                    <div class="loader" aria-hidden="true"><span></span><span></span><span></span></div>
                    <p class="empty-body">"Rendering…"</p>
                </div>
            }
            .into_any()
        };
        Some(view)
    }
}

#[component]
fn SyntaxStatus() -> impl IntoView {
    let state = expect_context::<AppState>();
    move || {
        let errors = state.severity_count(Severity::Error);
        if errors == 0 {
            view! {
                <span class="syntax-chip ok">
                    <IconView icon=Icon::Check />
                    "YAML looks good"
                </span>
            }
            .into_any()
        } else {
            view! {
                <button class="syntax-chip bad" on:click=move |_| state.select_diagnostic(0)>
                    <IconView icon=Icon::Error />
                    {format!("{errors} syntax problem{}", if errors == 1 { "" } else { "s" })}
                </button>
            }
            .into_any()
        }
    }
}

/// Banner over a diagram that is older than the text (the spec is currently broken): a neutral
/// "paused" note plus the error count in the error colour, which jumps to the first error.
#[component]
fn StaleNotice() -> impl IntoView {
    let state = expect_context::<AppState>();
    let visible = move || {
        state.last_svg.with(Option::is_some)
            && state.preview.with(|p| p.as_ref().is_some_and(|p| !matches!(p.status, PreviewStatus::Ready)))
    };
    let errors = move || state.severity_count(Severity::Error);
    view! {
        <Show when=visible>
            <div class="stale-notice" role="status">
                <IconView icon=Icon::Pause class="stale-icon" />
                <span>"Showing the last good render"</span>
                <Show when=move || { errors() > 0 }>
                    <button class="stale-errors" on:click=move |_| state.select_diagnostic(0)>
                        <IconView icon=Icon::Error />
                        {move || plural(errors(), "error")}
                    </button>
                </Show>
            </div>
        </Show>
    }
}

/// `1 error`, `3 errors`.
pub fn plural(count: usize, noun: &str) -> String {
    if count == 1 { format!("1 {noun}") } else { format!("{count} {noun}s") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_keeps_the_anchor_point_fixed() {
        let camera = Camera { x: 10.0, y: 20.0, zoom: 1.0 };
        let zoomed = camera.zoomed_at(2.0, 110.0, 120.0);
        // The content point under (110, 120) was (100, 100); it must still be there.
        assert!(((110.0 - zoomed.x) / zoomed.zoom - 100.0).abs() < 1e-9);
        assert!(((120.0 - zoomed.y) / zoomed.zoom - 100.0).abs() < 1e-9);
    }

    #[test]
    fn zoom_is_clamped() {
        let camera = Camera { x: 0.0, y: 0.0, zoom: 1.0 };
        assert!((camera.zoomed_at(1000.0, 0.0, 0.0).zoom - MAX_ZOOM).abs() < 1e-9);
        assert!((camera.zoomed_at(0.0001, 0.0, 0.0).zoom - MIN_ZOOM).abs() < 1e-9);
    }

    #[test]
    fn first_svg_and_new_cameras_always_fit() {
        assert!(needs_refit(FitIntent::Keep, None, (100.0, 100.0), true));
        assert!(needs_refit(FitIntent::Always, Some((100.0, 100.0)), (100.0, 100.0), true));
        assert!(needs_refit(FitIntent::Fresh, Some((100.0, 100.0)), (300.0, 80.0), true));
    }

    #[test]
    fn theme_or_reload_fits_only_when_the_size_changed() {
        assert!(!needs_refit(FitIntent::IfResized, Some((100.0, 100.0)), (100.2, 100.0), false));
        assert!(needs_refit(FitIntent::IfResized, Some((100.0, 100.0)), (104.0, 100.0), true));
    }

    #[test]
    fn typing_keeps_the_camera_unless_the_shape_jumps_and_nobody_moved_it() {
        assert!(!needs_refit(FitIntent::Keep, Some((1000.0, 600.0)), (1100.0, 640.0), false));
        assert!(needs_refit(FitIntent::Keep, Some((1000.0, 600.0)), (1200.0, 600.0), false));
        assert!(!needs_refit(FitIntent::Keep, Some((1000.0, 600.0)), (1600.0, 900.0), true));
    }

    #[test]
    fn intents_are_ordered_by_strength() {
        assert!(FitIntent::Keep < FitIntent::IfResized);
        assert!(FitIntent::IfResized < FitIntent::Always);
        assert!(FitIntent::Always < FitIntent::Fresh);
    }

    #[test]
    fn fitted_content_is_centred_below_the_toolbar() {
        let stage = (800.0, 700.0);
        let content = (1000.0, 500.0);
        let camera = Camera::fitted(content, stage);
        let center_x = camera.x + content.0 * camera.zoom / 2.0;
        let center_y = camera.y + content.1 * camera.zoom / 2.0;
        assert!((center_x - stage.0 / 2.0).abs() < 1e-9);
        assert!((center_y - (FIT_TOP_RESERVE_PX + (stage.1 - FIT_TOP_RESERVE_PX) / 2.0)).abs() < 1e-9);
        assert!(camera.x >= FIT_MARGIN_PX - 1e-9);
        assert!(camera.y >= FIT_TOP_RESERVE_PX + FIT_MARGIN_PX - 1e-9);
    }

    #[test]
    fn plural_counts() {
        assert_eq!(plural(1, "error"), "1 error");
        assert_eq!(plural(3, "warning"), "3 warnings");
    }

    #[test]
    fn fit_centers_and_never_blows_up_small_diagrams() {
        let camera = Camera::fitted((100.0, 50.0), (1000.0, 800.0));
        assert!((camera.zoom - FIT_MAX_ZOOM).abs() < 1e-9);
        assert!((camera.x - (1000.0 - 100.0 * camera.zoom) / 2.0).abs() < 1e-9);
        let large = Camera::fitted((4000.0, 1000.0), (1000.0, 800.0));
        assert!(large.zoom < 1.0);
    }
}
