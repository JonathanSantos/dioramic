//! Toolbar popovers and menus: keep them inside the preview pane (shift sideways, cap the height) and
//! close them on a click outside. The arrow keeps pointing at its button after a shift (`--fit-x`).

use leptos::ev;
use leptos::html::ElementType;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlElement;

use crate::dom::on_resize;

/// Closest a popover gets to the preview pane's edges (px).
pub const PANE_MARGIN_PX: f64 = 12.0;
/// A popover never gets shorter than this, even in a very short window (px); it scrolls instead.
const MIN_HEIGHT_PX: f64 = 160.0;

/// Horizontal shift that moves a box spanning `left..right` inside `min..max`. When the box is wider
/// than the room, its left edge wins (the title and the first options stay readable).
pub fn inside_shift(left: f64, right: f64, min: f64, max: f64) -> f64 {
    if right > max {
        (max - right).max(min - left)
    } else if left < min {
        min - left
    } else {
        0.0
    }
}

/// Room below a popover's top edge before the pane's bottom margin (px).
pub fn room_below(top: f64, pane_bottom: f64) -> f64 {
    (pane_bottom - PANE_MARGIN_PX - top).max(MIN_HEIGHT_PX)
}

/// Keeps the popover behind `node` inside the preview pane: measured when it opens, when it changes
/// size and when the window does. The shift goes to `--fit-x` (read by `translate` in the CSS, so the
/// opening animation and the shift do not fight over `transform`).
pub fn keep_inside_preview<E>(node: NodeRef<E>)
where
    E: ElementType,
    E::Output: JsCast + Clone + 'static,
{
    let fit = move || {
        let Some(element) = node.get_untracked().and_then(|element| element.dyn_into::<HtmlElement>().ok()) else {
            return;
        };
        fit_inside_preview(&element);
    };
    on_resize(node, fit);
    let resize = window_event_listener(ev::resize, move |_| fit());
    on_cleanup(move || resize.remove());
}

fn fit_inside_preview(element: &HtmlElement) {
    let Some(pane) = element.closest(".preview-pane").ok().flatten() else { return };
    let style = element.style();
    // Measure the natural position first: the previous shift must not feed the next one.
    let _ = style.set_property("--fit-x", "0px");
    let _ = style.remove_property("max-height");
    let _ = style.remove_property("overflow-y");
    let bounds = pane.get_bounding_client_rect();
    let rect = element.get_bounding_client_rect();
    let shift =
        inside_shift(rect.left(), rect.right(), bounds.left() + PANE_MARGIN_PX, bounds.right() - PANE_MARGIN_PX);
    let _ = style.set_property("--fit-x", &format!("{shift:.1}px"));
    if rect.bottom() > bounds.bottom() - PANE_MARGIN_PX {
        // Only a too-short pane scrolls the popover (scrolling clips its arrow).
        let _ = style.set_property("max-height", &format!("{:.0}px", room_below(rect.top(), bounds.bottom())));
        let _ = style.set_property("overflow-y", "auto");
    }
}

/// Closes `open` on a pointer press outside the element matching `scope` (the picker: button and
/// popover together, so the button can still toggle it).
pub fn close_on_outside_press(open: RwSignal<bool>, scope: &'static str) {
    let outside = window_event_listener(ev::pointerdown, move |event| {
        if !open.get_untracked() {
            return;
        }
        let inside = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
            .and_then(|element| element.closest(scope).ok().flatten())
            .is_some();
        if !inside {
            open.set(false);
        }
    });
    on_cleanup(move || outside.remove());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_inside_stays_put() {
        assert!(inside_shift(100.0, 300.0, 12.0, 588.0).abs() < 1e-9);
    }

    #[test]
    fn a_box_past_the_right_edge_moves_left() {
        assert!((inside_shift(774.0, 1134.0, 528.0, 1088.0) - -46.0).abs() < 1e-9);
    }

    #[test]
    fn a_box_past_the_left_edge_moves_right() {
        assert!((inside_shift(480.0, 680.0, 528.0, 1088.0) - 48.0).abs() < 1e-9);
    }

    #[test]
    fn a_box_wider_than_the_room_keeps_its_left_edge_in() {
        assert!((inside_shift(500.0, 1200.0, 528.0, 1088.0) - 28.0).abs() < 1e-9);
    }

    #[test]
    fn height_is_capped_but_never_collapses() {
        assert!((room_below(68.0, 700.0) - 620.0).abs() < 1e-9);
        assert!((room_below(600.0, 640.0) - MIN_HEIGHT_PX).abs() < 1e-9);
    }
}
