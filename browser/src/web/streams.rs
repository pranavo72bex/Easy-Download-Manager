//! The media each tab has loaded, read from responses and kept for the panel's list.

use super::*;

/// Where a tab's streams are kept between the worker's naps: the session's
/// storage, which the browser clears when it closes.
pub(super) fn streams_key(tab: i64) -> String {
    format!("dm.streams.{tab}")
}

/// A response the browser received, read for media a page's player loads,
/// to find a stream on sites no page reader knows.
#[wasm_bindgen]
pub async fn on_response_started(details: JsValue) {
    let seen: decide::Seen = match serde_json::from_value(from_js(&details)) {
        Ok(seen) => seen,
        Err(_) => return,
    };
    let tab = seen.tab_id;
    if tab < 0 {
        return;
    }
    let kept = match decide::keep(&seen) {
        decide::Keep::Nothing => return,
        // A new page: the old one's streams go from memory and from the
        // kept copy, which a worker woken by this navigation never read.
        decide::Keep::Forget => {
            PAGES.with(|p| p.borrow_mut().next(tab));
            STREAMS.with(|s| s.borrow_mut().forget(tab));
            forget_kept(tab).await;
            return;
        }
        decide::Keep::Add => {
            // What was kept before the worker slept, merged in first, so the
            // copy written back adds to it rather than replacing it.
            let page = PAGES.with(|p| p.borrow().now(tab));
            if !STREAMS.with(|s| s.borrow().knows(tab)) {
                let kept = kept_streams(tab).await;
                // The tab moved on while the copy was read: this response
                // and the copy are the old page's.
                if !PAGES.with(|p| p.borrow().still(tab, page)) {
                    return;
                }
                STREAMS.with(|s| s.borrow_mut().restore(tab, kept));
            }
            let before = STREAMS.with(|s| s.borrow().raw(tab));
            STREAMS.with(|s| s.borrow_mut().absorb(&seen));
            let after = STREAMS.with(|s| s.borrow().raw(tab));
            if after == before {
                return;
            }
            let item = to_js(&serde_json::json!({ streams_key(tab): after }));
            call("storage.session", "set", &[item]).await
        }
    };
    if let Err(why) = kept {
        log("debug", &format!("a tab's streams were not kept: {why}"));
    }
}

/// A tab's streams as kept in the session's storage.
pub(super) async fn kept_streams(tab: i64) -> Vec<decide::Stream> {
    let key = streams_key(tab);
    match call("storage.session", "get", &[JsValue::from_str(&key)]).await {
        Ok(got) => serde_json::from_value(from_js(&field(&got, &key))).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Removes the kept copy of a tab's streams.
pub(super) async fn forget_kept(tab: i64) {
    let key = streams_key(tab);
    if let Err(why) = call("storage.session", "remove", &[JsValue::from_str(&key)]).await {
        log(
            "debug",
            &format!("a tab's streams were not forgotten: {why}"),
        );
    }
}

/// A tab closed: what it loaded is forgotten, here and in the kept copy.
#[wasm_bindgen]
pub async fn on_tab_removed(tab: JsValue, _info: JsValue) {
    let Some(tab) = tab.as_f64().map(|t| t as i64).filter(|t| *t >= 0) else {
        return;
    };
    PAGES.with(|p| p.borrow_mut().next(tab));
    STREAMS.with(|s| s.borrow_mut().forget(tab));
    forget_kept(tab).await;
}

/// What a tab has loaded, from this worker or, after it stopped, from the
/// session's storage.
pub(super) async fn streams_of(tab: i64) -> Vec<decide::Stream> {
    if tab >= 0 && !STREAMS.with(|s| s.borrow().knows(tab)) {
        let kept = kept_streams(tab).await;
        STREAMS.with(|s| s.borrow_mut().restore(tab, kept));
    }
    STREAMS.with(|s| s.borrow().of(tab))
}
