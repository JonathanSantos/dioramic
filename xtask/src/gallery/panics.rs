//! Turns panics of unfinished stages (`todo!("T-104")`) into one-line errors instead of aborting the
//! whole gallery or flooding the terminal with 96 identical backtraces.

use std::any::Any;
use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, PanicHookInfo, catch_unwind};

thread_local! {
    static LAST_PANIC: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Replaces the default hook: the message and location are kept for [`guarded`], nothing is printed.
pub fn install_quiet_hook() {
    std::panic::set_hook(Box::new(remember_panic));
}

/// Restores the default hook (printing) once the gallery is done.
pub fn restore_default_hook() {
    let _ = std::panic::take_hook();
}

/// Runs `stage`, turning a panic into `Err("<message> (<file>:<line>)")`.
pub fn guarded<T>(stage: impl FnOnce() -> T) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(stage))
        .map_err(|payload| LAST_PANIC.with(|slot| slot.borrow_mut().take()).unwrap_or_else(|| payload_text(&*payload)))
}

fn remember_panic(info: &PanicHookInfo<'_>) {
    let location =
        info.location().map(|location| format!(" ({}:{})", location.file(), location.line())).unwrap_or_default();
    let message = format!("{}{location}", payload_text(info.payload()));
    LAST_PANIC.with(|slot| *slot.borrow_mut() = Some(message));
}

fn payload_text(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panicked".to_owned())
}
