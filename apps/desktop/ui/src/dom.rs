//! Small DOM helpers shared by components.

use leptos::html::ElementType;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use web_sys::ResizeObserver;

/// Calls `on_resize` whenever the element behind `node` changes size (panels dragged or folded,
/// window resized). The observer lives as long as the calling component.
pub fn on_resize<E>(node: NodeRef<E>, on_resize: impl Fn() + 'static)
where
    E: ElementType,
    E::Output: JsCast + Clone + 'static,
{
    let observer = StoredValue::new_local(None::<(ResizeObserver, Closure<dyn FnMut()>)>);
    let on_resize = std::rc::Rc::new(on_resize);
    Effect::new(move |_| {
        let Some(element) = node.get() else { return };
        let Ok(element) = element.dyn_into::<web_sys::Element>() else { return };
        let callback = on_resize.clone();
        // The entries are ignored: callers measure what they need themselves.
        let closure = Closure::<dyn FnMut()>::new(move || callback());
        let Ok(resize_observer) = ResizeObserver::new(closure.as_ref().unchecked_ref()) else { return };
        resize_observer.observe(&element);
        if let Some((previous, _)) =
            observer.try_update_value(|slot| slot.replace((resize_observer, closure))).flatten()
        {
            previous.disconnect();
        }
    });
    on_cleanup(move || {
        if let Some((resize_observer, _)) = observer.try_update_value(Option::take).flatten() {
            resize_observer.disconnect();
        }
    });
}
