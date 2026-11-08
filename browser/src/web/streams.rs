// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
    ensure_served().await;
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
    // Only a new line reaches here (the other answers returned above): a
    // playlist that says nothing of its quality is read for it.
    describe_playlist(tab, &seen.url, seen.requester()).await;
    read_track(tab, &seen.url, seen.requester()).await;
}

/// A whole MP4 file a player fetched by ranges (`decide::whole_by_ranges`): its first bytes
/// read once, with the browser's own session, for whether it is a picture or a sound, which
/// its server's type may not say (Bilibili calls both `video/mp4`; gaps row 155). A failure
/// leaves it as its type said.
async fn read_track(tab: i64, url: &str, requester: &str) {
    if !STREAMS.with(|s| s.borrow_mut().wants_track_read(tab, url)) {
        return;
    }
    let page = PAGES.with(|p| p.borrow().now(tab));
    let bytes = match fetch_start(url, requester, decide::TRACK_READ_CAP, 4000.0).await {
        Ok(bytes) => bytes,
        Err(why) => {
            log(
                "debug",
                &format!("a file's first bytes were not read: {why}"),
            );
            return;
        }
    };
    if !PAGES.with(|p| p.borrow().still(tab, page)) {
        return;
    }
    let Some((sound, height)) = decide::mp4_track(&bytes) else {
        return;
    };
    if !STREAMS.with(|s| s.borrow_mut().read_track(tab, url, sound, height)) {
        return;
    }
    keep_streams(tab).await;
}

/// A form a page sent (its body, as `background.js` read it): kept a moment
/// by address, in memory, for a download that is its answer.
#[wasm_bindgen]
pub fn on_form_sent(details: JsValue) {
    let got = from_js(&details);
    let Some(url) = got.get("url").and_then(|u| u.as_str()) else {
        return;
    };
    let text = |key: &str| {
        got.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let post = dm_protocol::Posted {
        form: text("form"),
        raw: text("raw"),
        content_type: String::new(),
    };
    if post.form.is_empty() && post.raw.is_empty() {
        return;
    }
    FORMS.with(|f| f.borrow_mut().keep(url, post, now()));
}

/// A request the browser is sending, read for its headers (never changed:
/// a Manifest V3 worker may not). Kept by address, in memory, for the
/// hand-off that address begins.
#[wasm_bindgen]
pub fn on_send_headers(details: JsValue) {
    if let Ok(sent) = serde_json::from_value::<decide::Sent>(from_js(&details)) {
        SENT.with(|s| s.borrow_mut().keep(&sent));
        if matches!(sent.kind.as_str(), "media" | "xmlhttprequest" | "other") {
            offer_renewals();
        }
        // Written to the session's storage a moment later, once for a burst of requests, so a
        // worker that sleeps before the download is handed over has not forgotten them.
        if !SENT_WRITING.with(|w| w.replace(true)) {
            wasm_bindgen_futures::spawn_local(async {
                sleep(1000.0).await;
                SENT_WRITING.with(|w| w.set(false));
                // What an earlier worker kept is read back first: written without it, a worker
                // just woken would replace it with the little it has heard itself.
                let _ = sent_to("").await;
                let all = SENT.with(|s| s.borrow().export());
                let reached = SENT.with(|s| s.borrow().export_reached());
                let item = to_js(&serde_json::json!({ SENT_KEY: all, REACHED_KEY: reached }));
                if let Err(why) = call("storage.session", "set", &[item]).await {
                    log("debug", &format!("request headers were not kept: {why}"));
                }
            });
        }
    }
}

/// Where the request headers are kept between the worker's naps.
const SENT_KEY: &str = "dm.sent";

/// Where the hosts each tab's requests went to are kept between the worker's naps.
const REACHED_KEY: &str = "dm.sent.reached";

/// Where the streams handed to dm are kept between the worker's naps.
const HANDED_KEY: &str = "dm.handed";

/// The least time between two looks for renewed addresses: every request the
/// browser sends calls here, and a look compares each handed stream with
/// every address heard.
const LOOK_EVERY_MS: f64 = 2_000.0;

/// Reads back what an earlier worker handed to dm, once in this worker's life.
pub(super) async fn load_handed() {
    if HANDED_LOADED.with(|l| l.replace(true)) {
        return;
    }
    if let Ok(got) = call("storage.session", "get", &[JsValue::from_str(HANDED_KEY)]).await {
        if let Some(older) = field(&got, HANDED_KEY).as_string() {
            HANDED.with(|h| h.borrow_mut().merge(&older));
        }
    }
}

/// Writes what was handed to dm to the session's storage.
pub(super) async fn store_handed() {
    let all = HANDED.with(|h| h.borrow().export());
    let item = to_js(&serde_json::json!({ HANDED_KEY: all }));
    if let Err(why) = call("storage.session", "set", &[item]).await {
        log(
            "debug",
            &format!("the streams handed to dm were not kept: {why}"),
        );
    }
}

/// A newer signed address for a stream handed to dm, as the page's player asked for it (gaps row
/// 121), offered to an app that says it takes them: a row whose address ran out goes on with it,
/// and the app drops it otherwise. Looked for at most every [`LOOK_EVERY_MS`].
fn offer_renewals() {
    let at = now();
    if at - HANDED_LOOKED.with(|l| l.get()) < LOOK_EVERY_MS {
        return;
    }
    HANDED_LOOKED.with(|l| l.set(at));
    wasm_bindgen_futures::spawn_local(async move {
        load_handed().await;
        let _ = sent_to("").await;
        let due = SENT.with(|s| HANDED.with(|h| h.borrow_mut().due(&s.borrow(), at)));
        if due.is_empty() {
            return;
        }
        store_handed().await;
        let (app, mut port) = app().await;
        let App::Present { hello, .. } = &app else {
            return;
        };
        if !hello.renews {
            return;
        }
        for (n, renewal) in due.into_iter().enumerate() {
            let headers = sent_to(&renewal.new).await;
            let start = Start {
                id: format!("renew-{}-{n}", at as u64),
                url: renewal.new,
                referrer: renewal.referrer,
                headers,
                renews: true,
                ..Start::default()
            };
            if let Err(why) = hand(port.take(), start, None).await {
                log(
                    "debug",
                    &format!(
                        "a renewed address for {} was not offered: {why}",
                        renewal.old
                    ),
                );
            }
        }
    });
}

/// The headers the browser sent to `url`, from this worker or, after it slept, the session's
/// storage.
pub(super) async fn sent_to(url: &str) -> Vec<(String, String)> {
    if !SENT_LOADED.with(|l| l.replace(true)) {
        let keys = to_js(&serde_json::json!([SENT_KEY, REACHED_KEY]));
        if let Ok(got) = call("storage.session", "get", &[keys]).await {
            if let Ok(older) = serde_json::from_value(from_js(&field(&got, SENT_KEY))) {
                SENT.with(|s| s.borrow_mut().merge(older));
            }
            if let Ok(older) = serde_json::from_value(from_js(&field(&got, REACHED_KEY))) {
                SENT.with(|s| s.borrow_mut().merge_reached(older));
            }
        }
    }
    SENT.with(|s| s.borrow().of(url))
}

/// The names of the cookies the browser itself sent `url`'s host from `tab`
/// (`RequestHeaders::sent_cookies`), from this worker or the session's storage.
pub(super) async fn sent_cookies(tab: Option<i64>, url: &str) -> Vec<String> {
    let _ = sent_to("").await;
    SENT.with(|s| s.borrow().sent_cookies(tab, url))
}

/// The newest address `tab`'s player asked for that is the same stream as `url`, when a signed
/// address has been asked for again since (`RequestHeaders::renewed`); `url` itself otherwise.
pub(super) async fn renewed(url: &str, tab: Option<i64>) -> String {
    // Read back from the session's storage first, as `sent_to` does.
    let _ = sent_to(url).await;
    SENT.with(|s| s.borrow().renewed(url, tab))
        .unwrap_or_else(|| url.to_string())
}

/// A playlist or manifest the tab just loaded that is named by nothing but
/// its kind: read once, within a few seconds and a few hundred kilobytes,
/// with the browser's own session; what it says of its renditions names the
/// line, and a master playlist's renditions stop being lines of their own.
/// A failure leaves the line as it was.
async fn describe_playlist(tab: i64, url: &str, requester: &str) {
    if !STREAMS.with(|s| s.borrow_mut().wants_description(tab, url)) {
        return;
    }
    let page = PAGES.with(|p| p.borrow().now(tab));
    let body = match fetch_text(url, requester, decide::PLAYLIST_READ_CAP, 4000.0).await {
        Ok(body) => body,
        Err(why) => {
            log("debug", &format!("a playlist was not read: {why}"));
            return;
        }
    };
    // The tab moved on while it was read: the playlist is the old page's.
    if !PAGES.with(|p| p.borrow().still(tab, page)) {
        return;
    }
    if !STREAMS.with(|s| s.borrow_mut().describe(tab, url, &body)) {
        return;
    }
    // A master playlist says nothing of how long the video plays; one of its renditions does,
    // and the line says it, so two videos of one page are told apart (gaps row 155).
    if let Some(rendition) = STREAMS.with(|s| s.borrow().rendition_for_length(tab, url)) {
        match fetch_text(&rendition, url, decide::PLAYLIST_READ_CAP, 4000.0).await {
            Ok(media) if PAGES.with(|p| p.borrow().still(tab, page)) => {
                if let Some(seconds) = decide::hls_length(&media) {
                    STREAMS.with(|s| s.borrow_mut().lengthen(tab, url, seconds));
                }
            }
            Ok(_) => return,
            Err(why) => log(
                "debug",
                &format!("a rendition was not read for its length: {why}"),
            ),
        }
    }
    keep_streams(tab).await;
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

/// Writes a tab's streams as this worker knows them to the session's storage, which outlives
/// the worker; a failure is only logged.
async fn keep_streams(tab: i64) {
    let after = STREAMS.with(|s| s.borrow().raw(tab));
    let item = to_js(&serde_json::json!({ streams_key(tab): after }));
    if let Err(why) = call("storage.session", "set", &[item]).await {
        log("debug", &format!("a tab's streams were not kept: {why}"));
    }
}
