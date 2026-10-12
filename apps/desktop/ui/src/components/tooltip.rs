//! One tooltip layer for the whole app. Any element with `data-tip` gets a tooltip after a short
//! pause (hover or keyboard focus); `data-kbd="⌘ E"` adds the shortcut as `<kbd>` keys. The layer
//! is `position: fixed` and kept inside the window, so tips are never clipped by a panel.

use leptos::ev;
use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::Element;

/// Pause before a tooltip appears.
const SHOW_DELAY: std::time::Duration = std::time::Duration::from_millis(450);
/// Gap between the element and its tooltip (px).
const GAP_PX: f64 = 8.0;
/// Closest a tooltip gets to the window edge (px).
const EDGE_PX: f64 = 8.0;
/// Room a tooltip needs below its element before it flips above (px).
const FLIP_ROOM_PX: f64 = 64.0;

/// What is shown and where.
#[derive(Debug, Clone, PartialEq)]
struct Tip {
    text: String,
    keys: Vec<String>,
    /// Horizontal centre of the element (px).
    x: f64,
    /// Bottom (below) or top (above) of the element (px).
    y: f64,
    above: bool,
}

fn tip_for(element: &Element) -> Option<Tip> {
    let text = element.get_attribute("data-tip").filter(|text| !text.is_empty())?;
    let keys = element
        .get_attribute("data-kbd")
        .map(|keys| keys.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default();
    let rect = element.get_bounding_client_rect();
    let viewport = web_sys::window().and_then(|window| window.inner_height().ok()).and_then(|h| h.as_f64());
    let above = element.get_attribute("data-tip-pos").as_deref() == Some("above")
        || viewport.is_some_and(|height| rect.bottom() + FLIP_ROOM_PX > height);
    Some(Tip {
        text,
        keys,
        x: rect.left() + rect.width() / 2.0,
        y: if above { rect.top() - GAP_PX } else { rect.bottom() + GAP_PX },
        above,
    })
}

/// The element that owns the tooltip for an event target, unless its menu is open.
fn tip_owner(target: Option<web_sys::EventTarget>) -> Option<Element> {
    let element = target?.dyn_into::<Element>().ok()?;
    let owner = element.closest("[data-tip]").ok()??;
    (owner.get_attribute("aria-expanded").as_deref() != Some("true")).then_some(owner)
}

/// Shift that keeps a box of `width` centred at `x` inside a window of `viewport` px.
pub fn edge_shift(x: f64, width: f64, viewport: f64) -> f64 {
    let left = x - width / 2.0;
    let right = x + width / 2.0;
    if left < EDGE_PX {
        EDGE_PX - left
    } else if right > viewport - EDGE_PX {
        (viewport - EDGE_PX - right).max(EDGE_PX - left)
    } else {
        0.0
    }
}

/// The tooltip layer.
#[component]
pub fn Tooltips() -> impl IntoView {
    let tip = RwSignal::new(None::<Tip>);
    let shift = RwSignal::new(0.0_f64);
    let owner = StoredValue::new_local(None::<Element>);
    let timer = StoredValue::new(None::<TimeoutHandle>);
    let layer: NodeRef<html::Div> = NodeRef::new();

    let hide = move || {
        if let Some(handle) = timer.get_value() {
            handle.clear();
        }
        timer.set_value(None);
        owner.set_value(None);
        tip.set(None);
    };
    let schedule = move |element: Element| {
        let same = owner.with_value(|current| current.as_ref() == Some(&element));
        if same {
            return;
        }
        hide();
        owner.set_value(Some(element.clone()));
        let handle = set_timeout_with_handle(
            move || {
                let still_owner = owner.with_value(|current| current.as_ref() == Some(&element));
                if still_owner && element.is_connected() {
                    shift.set(0.0);
                    tip.set(tip_for(&element));
                }
            },
            SHOW_DELAY,
        )
        .ok();
        timer.set_value(handle);
    };

    let listeners = [
        window_event_listener(ev::pointerover, move |event| match tip_owner(event.target()) {
            Some(element) => schedule(element),
            None => hide(),
        }),
        window_event_listener(ev::focusin, move |event| {
            let keyboard = event
                .target()
                .and_then(|target| target.dyn_into::<Element>().ok())
                .is_some_and(|element| element.matches(":focus-visible").unwrap_or(false));
            if keyboard && let Some(element) = tip_owner(event.target()) {
                schedule(element);
            }
        }),
        window_event_listener(ev::focusout, move |_| hide()),
        window_event_listener(ev::pointerdown, move |_| hide()),
        window_event_listener(ev::keydown, move |_| hide()),
        window_event_listener(ev::wheel, move |_| hide()),
        window_event_listener(ev::blur, move |_| hide()),
    ];
    on_cleanup(move || {
        for listener in listeners {
            listener.remove();
        }
    });

    // Keep the tip inside the window once its width is known.
    Effect::new(move |_| {
        let Some(current) = tip.get() else { return };
        request_animation_frame(move || {
            let (Some(element), Some(window)) = (layer.get_untracked(), web_sys::window()) else { return };
            let viewport = window.inner_width().ok().and_then(|width| width.as_f64()).unwrap_or(f64::MAX);
            let width = element.get_bounding_client_rect().width();
            shift.set(edge_shift(current.x, width, viewport));
        });
    });

    move || {
        tip.get().map(|tip| {
            let transform = if tip.above { "translate(-50%, -100%)" } else { "translate(-50%, 0)" };
            let x = tip.x;
            view! {
                <div
                    class="tooltip"
                    class:above=tip.above
                    role="tooltip"
                    node_ref=layer
                    style:left=move || format!("{:.1}px", x + shift.get())
                    style:top=format!("{:.1}px", tip.y)
                    style:transform=transform
                >
                    <span class="tooltip-text">{tip.text}</span>
                    {(!tip.keys.is_empty())
                        .then(|| {
                            view! {
                                <span class="tooltip-keys">
                                    {tip.keys.into_iter().map(|key| view! { <kbd>{key}</kbd> }).collect_view()}
                                </span>
                            }
                        })}
                </div>
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tips_stay_inside_the_window() {
        assert!((edge_shift(500.0, 100.0, 1000.0)).abs() < 1e-9);
        assert!((edge_shift(20.0, 100.0, 1000.0) - (EDGE_PX + 30.0)).abs() < 1e-9);
        assert!((edge_shift(990.0, 100.0, 1000.0) - (1000.0 - EDGE_PX - 1040.0)).abs() < 1e-9);
    }
}
