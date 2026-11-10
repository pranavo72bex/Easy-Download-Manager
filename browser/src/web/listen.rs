// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The background's listeners, registered from here (gaps row 169).
//!
//! A Manifest V3 background must register its listeners on its first turn, or a browser that
//! woke it for an event finds nobody listening (decisions, 2026-10-12). They were registered by
//! `background.js`, which forwarded each event once the WebAssembly had loaded. The WebAssembly is
//! now written into the background's script and started at once (`initSync`, `scripts/package.sh`),
//! so this registers them itself on that first turn, and the background has no JavaScript of its
//! own.

use super::*;
use wasm_bindgen::closure::Closure;

/// Every listener the background has, in the order `background.js` registered them.
#[wasm_bindgen]
pub fn background_start() {
    let all = ["<all_urls>"];
    // A download the browser has begun.
    listen(
        "downloads.onCreated",
        Closure::<dyn FnMut(JsValue)>::new(|item| {
            spawn(super::downloads::on_download_created(item))
        }),
        &[],
    );
    // Chrome only: its choice of the file's name, held while dm decides, so a download dm takes
    // never reaches the browser's Save dialog. `true` says the answer comes later.
    listen(
        "downloads.onDeterminingFilename",
        Closure::<dyn FnMut(JsValue, JsValue) -> JsValue>::new(|item, suggest| {
            super::downloads::on_determining_filename(item, suggest);
            JsValue::TRUE
        }),
        &[],
    );
    listen(
        "contextMenus.onClicked",
        Closure::<dyn FnMut(JsValue, JsValue)>::new(|info, tab| spawn(on_menu_clicked(info, tab))),
        &[],
    );
    listen(
        "action.onClicked",
        Closure::<dyn FnMut(JsValue)>::new(|tab| spawn(on_action_clicked(tab))),
        &[],
    );
    listen(
        "runtime.onInstalled",
        Closure::<dyn FnMut(JsValue)>::new(|details| spawn(on_installed(details))),
        &[],
    );
    listen(
        "runtime.onStartup",
        Closure::<dyn FnMut()>::new(|| spawn(on_startup())),
        &[],
    );
    // A closed tab's streams are forgotten, in memory and in session storage.
    listen(
        "tabs.onRemoved",
        Closure::<dyn FnMut(JsValue, JsValue)>::new(|tab, info| spawn(on_tab_removed(tab, info))),
        &[],
    );
    // Media a page's player loads, for the panel's list (read, never changed).
    listen(
        "webRequest.onResponseStarted",
        Closure::<dyn FnMut(JsValue)>::new(|details| spawn(on_response_started(details))),
        &[
            filter(&all, &["main_frame", "media", "xmlhttprequest", "other"]),
            words(&["responseHeaders"]),
        ],
    );
    // The headers of the requests a page makes, read only (a Manifest V3 worker cannot change
    // them), kept by address for the download a request begins. `extraHeaders` is Chrome's word
    // for the ones it hides unless asked (Authorization, Origin); Firefox refuses it.
    let sent = filter(
        &all,
        &[
            "main_frame",
            "sub_frame",
            "media",
            "xmlhttprequest",
            "other",
        ],
    );
    let headers = Closure::<dyn FnMut(JsValue)>::new(on_send_headers).into_js_value();
    let event = "webRequest.onSendHeaders";
    if add(
        event,
        &headers,
        &[sent.clone(), words(&["requestHeaders", "extraHeaders"])],
    )
    .is_err()
    {
        if let Err(why) = add(event, &headers, &[sent, words(&["requestHeaders"])]) {
            log("error", &format!("dm: {event}: {why}"));
        }
    }
    // The body of a form a page sends: a download that is its answer is fetched by dm sending the
    // same form (`decide::posted_body`).
    listen(
        "webRequest.onBeforeRequest",
        Closure::<dyn FnMut(JsValue)>::new(on_before_request),
        &[
            filter(&all, &["main_frame", "sub_frame"]),
            words(&["requestBody"]),
        ],
    );
    // Every message of the extension's own pages and page scripts, answered once Rust has; `true`
    // keeps the answer's channel open until then.
    listen(
        "runtime.onMessage",
        Closure::<dyn FnMut(JsValue, JsValue, JsValue) -> JsValue>::new(
            |message, sender, reply: JsValue| {
                spawn(async move {
                    let answer = on_page_message(message, sender).await;
                    if let Some(reply) = reply.dyn_ref::<Function>() {
                        let _ = reply.call1(&JsValue::UNDEFINED, &answer);
                    }
                });
                JsValue::TRUE
            },
        ),
        &[],
    );
}

/// Registers `listener` on the event at `path` (`downloads.onCreated`), with the event's own
/// further arguments. An event this browser lacks (Firefox has no `onDeterminingFilename`) is
/// passed over, as `background.js`'s `?.` did.
fn listen<T: ?Sized + wasm_bindgen::closure::WasmClosure>(
    path: &str,
    listener: Closure<T>,
    extra: &[JsValue],
) {
    let event = api(path);
    if event.is_undefined() || event.is_null() {
        return;
    }
    if let Err(why) = add(path, &listener.into_js_value(), extra) {
        log("error", &format!("dm: {path}: {why}"));
    }
}

fn add(path: &str, listener: &JsValue, extra: &[JsValue]) -> Result<JsValue, String> {
    let mut args = vec![listener.clone()];
    args.extend_from_slice(extra);
    call_on(&api(path), "addListener", &args)
}

/// A `webRequest` filter: these addresses, these kinds of request.
fn filter(urls: &[&str], types: &[&str]) -> JsValue {
    to_js(&serde_json::json!({ "urls": urls, "types": types }))
}

/// A list of words, as an event's extra information is asked for.
fn words(list: &[&str]) -> JsValue {
    to_js(&serde_json::json!(list))
}

/// Runs a handler's future to its end; its own failures are its own (each says so in the log).
fn spawn(work: impl std::future::Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(work);
}
