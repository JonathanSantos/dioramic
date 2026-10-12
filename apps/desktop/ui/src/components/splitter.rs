//! Draggable divider between two panels: pointer drag, arrow keys (Shift = bigger steps) and
//! double click to restore the default size.

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, KeyboardEvent, PointerEvent};

use crate::layout::SPLITTER_STEP_PX;

/// Arrow keys move this many steps while Shift is held.
const BIG_STEP_FACTOR: f64 = 4.0;

/// Direction the divider moves in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// A vertical bar between columns (drags left/right).
    Columns,
    /// A horizontal bar between rows (drags up/down).
    Rows,
}

impl Axis {
    fn class(self) -> &'static str {
        match self {
            Self::Columns => "splitter columns",
            Self::Rows => "splitter rows",
        }
    }

    fn orientation(self) -> &'static str {
        match self {
            Self::Columns => "vertical",
            Self::Rows => "horizontal",
        }
    }

    fn coordinate(self, event: &PointerEvent) -> f64 {
        match self {
            Self::Columns => f64::from(event.client_x()),
            Self::Rows => f64::from(event.client_y()),
        }
    }

    /// −1 / +1 for the arrow keys along this axis.
    fn key_direction(self, key: &str) -> Option<f64> {
        match (self, key) {
            (Self::Columns, "ArrowLeft") | (Self::Rows, "ArrowUp") => Some(-1.0),
            (Self::Columns, "ArrowRight") | (Self::Rows, "ArrowDown") => Some(1.0),
            _ => None,
        }
    }

    /// Keys shown in the tooltip.
    fn keys(self) -> &'static str {
        match self {
            Self::Columns => "← →",
            Self::Rows => "↑ ↓",
        }
    }

    fn body_class(self) -> &'static str {
        match self {
            Self::Columns => "resizing-columns",
            Self::Rows => "resizing-rows",
        }
    }
}

/// Divider. `on_drag` receives the pointer position along the axis (client px), `on_step` a signed
/// keyboard step in px, `on_reset` fires on double click / Enter.
#[component]
pub fn Splitter(
    /// Direction.
    axis: Axis,
    /// Accessible name ("Resize editor").
    label: &'static str,
    /// Pointer position while dragging.
    #[prop(into)]
    on_drag: Callback<f64>,
    /// Keyboard step.
    #[prop(into)]
    on_step: Callback<f64>,
    /// Back to the default size.
    #[prop(into)]
    on_reset: Callback<()>,
) -> impl IntoView {
    let dragging = RwSignal::new(false);

    let start = move |event: PointerEvent| {
        if event.button() != 0 {
            return;
        }
        event.prevent_default();
        if let Some(target) = event.current_target().and_then(|target| target.dyn_into::<Element>().ok()) {
            let _ = target.set_pointer_capture(event.pointer_id());
        }
        dragging.set(true);
        set_body_class(axis.body_class(), true);
    };
    let stop = move |_: PointerEvent| {
        if dragging.get_untracked() {
            dragging.set(false);
            set_body_class(axis.body_class(), false);
        }
    };
    let on_key = move |event: KeyboardEvent| {
        let key = event.key();
        if key == "Enter" {
            on_reset.run(());
            event.prevent_default();
            return;
        }
        if let Some(direction) = axis.key_direction(&key) {
            let factor = if event.shift_key() { BIG_STEP_FACTOR } else { 1.0 };
            on_step.run(direction * factor * SPLITTER_STEP_PX);
            event.prevent_default();
        }
    };

    view! {
        <div
            class=axis.class()
            class:dragging=move || dragging.get()
            role="separator"
            tabindex="0"
            aria-orientation=axis.orientation()
            aria-label=label
            data-tip=format!("{label} · double click resets")
            data-kbd=axis.keys()
            on:pointerdown=start
            on:pointermove=move |event: PointerEvent| {
                if dragging.get_untracked() {
                    on_drag.run(axis.coordinate(&event));
                }
            }
            on:pointerup=stop
            on:pointercancel=stop
            on:dblclick=move |_| on_reset.run(())
            on:keydown=on_key
        ></div>
    }
}

/// Keeps the resize cursor (and no text selection) everywhere while a divider is dragged.
fn set_body_class(class: &str, on: bool) {
    let Some(body) = web_sys::window().and_then(|window| window.document()).and_then(|document| document.body()) else {
        return;
    };
    let list = body.class_list();
    let _ = if on { list.add_1(class) } else { list.remove_1(class) };
}
