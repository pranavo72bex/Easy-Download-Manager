//! The page's window and document, the extension API, and timers.

use super::*;

pub(super) fn window() -> Option<Window> {
    web_sys::window()
}

/// The viewport's width and height.
pub(super) fn window_size() -> (f64, f64) {
    let Some(window) = window() else {
        return (0.0, 0.0);
    };
    let size = |v: Result<JsValue, JsValue>| v.ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
    (size(window.inner_width()), size(window.inner_height()))
}

pub(super) fn document() -> Option<Document> {
    window()?.document()
}

pub(super) fn now() -> f64 {
    js_sys::Date::now()
}

thread_local! {
    /// The extension API, as the loader handed it over. In Firefox a module
    /// a page script imports does not see the page script's `browser`.
    pub(super) static EXT: RefCell<JsValue> = const { RefCell::new(JsValue::UNDEFINED) };
}

/// The extension API object: the one the loader handed over, else
/// `browser` where it exists, else `chrome`.
pub(super) fn ext() -> JsValue {
    let given = EXT.with(|cell| cell.borrow().clone());
    if given.is_object() {
        return given;
    }
    let global: JsValue = js_sys::global().into();
    let browser = Reflect::get(&global, &"browser".into()).unwrap_or(JsValue::UNDEFINED);
    if browser.is_object() {
        browser
    } else {
        Reflect::get(&global, &"chrome".into()).unwrap_or(JsValue::UNDEFINED)
    }
}

pub(super) fn set_timeout(callback: impl FnOnce() + 'static, ms: i32) {
    let closure = Closure::once_into_js(callback);
    if let (Some(window), Ok(function)) = (window(), closure.dyn_into::<Function>()) {
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&function, ms);
    }
}
