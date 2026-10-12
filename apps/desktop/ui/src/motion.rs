//! The system "reduce motion" setting. The motion preview plays SMIL and CSS animations that the
//! stylesheet cannot pause, so the app simply asks the backend for the static SVG while it is on.

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use web_sys::MediaQueryList;

const REDUCED_MOTION_QUERY: &str = "(prefers-reduced-motion: reduce)";

fn reduced_motion_query() -> Option<MediaQueryList> {
    web_sys::window()?.match_media(REDUCED_MOTION_QUERY).ok().flatten()
}

/// True when the system asks for reduced motion.
pub fn prefers_reduced_motion() -> bool {
    reduced_motion_query().is_some_and(|query| query.matches())
}

/// Calls `on_change` with the new value whenever the setting changes, for the life of the app.
pub fn watch_reduced_motion(on_change: impl Fn(bool) + 'static) {
    let Some(query) = reduced_motion_query() else { return };
    let watched = query.clone();
    let closure = Closure::<dyn FnMut()>::new(move || on_change(watched.matches()));
    if query.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref()).is_ok() {
        // The listener lives as long as the window: the app never unmounts.
        closure.forget();
    }
}
