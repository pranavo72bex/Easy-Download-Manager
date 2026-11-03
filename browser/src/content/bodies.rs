//! The answers the page's own script kept about media (`globalThis.__dmBodies`).

use super::*;

/// The kept bodies, oldest first.
pub(super) fn kept_texts() -> Vec<String> {
    let Ok(kept) = Reflect::get(&js_sys::global(), &"__dmBodies".into()) else {
        return Vec::new();
    };
    kept.dyn_ref::<Array>()
        .map(|list| list.iter().filter_map(|b| b.as_string()).collect())
        .unwrap_or_default()
}
