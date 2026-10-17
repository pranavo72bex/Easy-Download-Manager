//! The toolbar popup: how the dm app is, the switches a person reaches for
//! in the browser, and a button that brings the app up.
//!
//! - **Catch downloads**: off, every download stays with the browser.
//! - **Show "Download video" on this site**: the panel's per-site list,
//!   the same one its × menu adds to, so a site turned off there is turned
//!   back on here.
//! - **Take every download from this site**: whatever the app's rules say
//!   about a download's type or size, this site's go to dm (which still
//!   refuses what only the browser can fetch).
//! - **Hand over this site's sign-in**: the browser's cookies for this site
//!   go to dm with its downloads, so a file behind a sign-in is taken and
//!   not left. Asks the browser for the `cookies` permission the first time
//!   it is switched on, for that is when it is first needed.
//! - **Open dm**: the app's window to the front, or the app started.
//!
//! The page is `extension/popup.html`; this wires it. What a site is, and
//! whether the panel is kept off one, is decided in [`crate::decide`].

use js_sys::{Array, Function, Object, Promise, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Document, Element, HtmlInputElement};

use crate::decide;

fn document() -> Option<Document> {
    web_sys::window()?.document()
}

fn by_id(id: &str) -> Option<Element> {
    document()?.get_element_by_id(id)
}

fn input(id: &str) -> Option<HtmlInputElement> {
    by_id(id)?.dyn_into().ok()
}

/// `ext.<path>.<method>(args…)`, awaited.
async fn call(ext: &JsValue, path: &str, method: &str, args: &[JsValue]) -> Option<JsValue> {
    let mut target = ext.clone();
    for part in path.split('.') {
        target = Reflect::get(&target, &part.into()).ok()?;
    }
    let function: Function = Reflect::get(&target, &method.into())
        .ok()?
        .dyn_into()
        .ok()?;
    let list: Array = args.iter().cloned().collect();
    let promise: Promise = function.apply(&target, &list).ok()?.dyn_into().ok()?;
    JsFuture::from(promise).await.ok()
}

async fn stored(ext: &JsValue, key: &str) -> JsValue {
    call(ext, "storage.local", "get", &[key.into()])
        .await
        .and_then(|got| Reflect::get(&got, &key.into()).ok())
        .unwrap_or(JsValue::UNDEFINED)
}

async fn store(ext: &JsValue, key: &str, value: JsValue) {
    let item = Object::new();
    let _ = Reflect::set(&item, &key.into(), &value);
    let _ = call(ext, "storage.local", "set", &[item.into()]).await;
}

fn sites(value: &JsValue) -> Vec<String> {
    value
        .dyn_ref::<Array>()
        .map(|a| a.iter().filter_map(|v| v.as_string()).collect())
        .unwrap_or_default()
}

async fn ask(ext: &JsValue, kind: &str) -> JsValue {
    let message = Object::new();
    let _ = Reflect::set(&message, &"type".into(), &kind.into());
    call(ext, "runtime", "sendMessage", &[message.into()])
        .await
        .unwrap_or(JsValue::UNDEFINED)
}

fn text(reply: &JsValue, key: &str) -> String {
    Reflect::get(reply, &key.into())
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default()
}

fn say(text: &str, bad: bool) {
    if let Some(status) = by_id("status") {
        status.set_text_content(Some(text));
        status.set_class_name(if bad { "status bad" } else { "status" });
    }
}

fn on_change(element: &Element, handler: impl Fn() + 'static) {
    let closure = Closure::<dyn Fn()>::new(handler);
    let _ = element.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref());
    closure.forget();
}

/// Wires the popup, once its WebAssembly has loaded.
#[wasm_bindgen]
pub fn popup_start(ext: JsValue) {
    // The app, asked through the background, which owns the connection.
    let asked = ext.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let reply = ask(&asked, decide::STATUS_MESSAGE).await;
        let ok = Reflect::get(&reply, &"ok".into())
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if ok {
            let ytdlp = Reflect::get(&reply, &"ytdlp".into())
                .ok()
                .and_then(|v| v.as_bool());
            let extra = if ytdlp == Some(false) {
                " · install yt-dlp for video sites"
            } else {
                ""
            };
            say(
                &format!("Connected to dm {}{extra}", text(&reply, "version")),
                false,
            );
        } else {
            let said = text(&reply, "said");
            say(
                if said.is_empty() {
                    "The dm app did not answer"
                } else {
                    &said
                },
                true,
            );
        }
    });

    // Catch downloads: on unless switched off.
    if let Some(catching) = input("catching") {
        let (read, shown) = (ext.clone(), catching.clone());
        wasm_bindgen_futures::spawn_local(async move {
            shown.set_checked(stored(&read, decide::CATCHING_KEY).await.as_bool() != Some(false));
        });
        let (ext, box_) = (ext.clone(), catching.clone());
        on_change(&catching, move || {
            let (ext, on) = (ext.clone(), box_.checked());
            wasm_bindgen_futures::spawn_local(async move {
                store(&ext, decide::CATCHING_KEY, JsValue::from_bool(on)).await;
            });
        });
    }

    // The panel on this site: the tab in front, by the same rule the page
    // script keeps it off by.
    let ext_tab = ext.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let (Some(panel), Some(row), Some(label)) =
            (input("panel"), by_id("panel-row"), by_id("panel-label"))
        else {
            return;
        };
        let query = Object::new();
        let _ = Reflect::set(&query, &"active".into(), &JsValue::TRUE);
        let _ = Reflect::set(&query, &"currentWindow".into(), &JsValue::TRUE);
        let url = call(&ext_tab, "tabs", "query", &[query.into()])
            .await
            .and_then(|tabs| tabs.dyn_into::<Array>().ok())
            .map(|tabs| text(&tabs.get(0), "url"))
            .unwrap_or_default();
        let Some(site) = decide::site_of(&url).filter(|_| url.starts_with("http")) else {
            panel.set_disabled(true);
            panel.set_checked(false);
            row.set_class_name("row off");
            label.set_text_content(Some("No video panel on this page"));
            return;
        };
        label.set_text_content(Some(&format!("Show “Download video” on {site}")));
        let off = sites(&stored(&ext_tab, decide::PANEL_OFF_KEY).await);
        panel.set_checked(!decide::panel_off(&off, &url));
        let (ext, box_) = (ext_tab.clone(), panel.clone());
        on_change(&panel, move || {
            let (ext, show, site) = (ext.clone(), box_.checked(), site.clone());
            wasm_bindgen_futures::spawn_local(async move {
                let off = sites(&stored(&ext, decide::PANEL_OFF_KEY).await);
                let list: Array = decide::with_site(&off, &site, !show)
                    .iter()
                    .map(|s| JsValue::from_str(s))
                    .collect();
                store(&ext, decide::PANEL_OFF_KEY, list.into()).await;
            });
        });
    });

    // Every download from this site, whatever its type: the worker reads
    // the same list before it asks the app's rules.
    let ext_all = ext.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let (Some(all), Some(row), Some(label)) = (
            input("catchall"),
            by_id("catchall-row"),
            by_id("catchall-label"),
        ) else {
            return;
        };
        let query = Object::new();
        let _ = Reflect::set(&query, &"active".into(), &JsValue::TRUE);
        let _ = Reflect::set(&query, &"currentWindow".into(), &JsValue::TRUE);
        let url = call(&ext_all, "tabs", "query", &[query.into()])
            .await
            .and_then(|tabs| tabs.dyn_into::<Array>().ok())
            .map(|tabs| text(&tabs.get(0), "url"))
            .unwrap_or_default();
        let Some(site) = decide::site_of(&url).filter(|_| url.starts_with("http")) else {
            all.set_disabled(true);
            all.set_checked(false);
            row.set_class_name("row off");
            label.set_text_content(Some("No downloads to take from this page"));
            return;
        };
        label.set_text_content(Some(&format!("Take every download from {site}")));
        let listed = sites(&stored(&ext_all, decide::CATCH_ALL_KEY).await);
        all.set_checked(decide::forced(&listed, &url));
        let (ext, box_) = (ext_all.clone(), all.clone());
        on_change(&all, move || {
            let (ext, on, site) = (ext.clone(), box_.checked(), site.clone());
            wasm_bindgen_futures::spawn_local(async move {
                let listed = sites(&stored(&ext, decide::CATCH_ALL_KEY).await);
                let list: Array = decide::with_site(&listed, &site, on)
                    .iter()
                    .map(|s| JsValue::from_str(s))
                    .collect();
                store(&ext, decide::CATCH_ALL_KEY, list.into()).await;
            });
        });
    });

    // This site's sign-in with its downloads: the cookies permission is
    // asked for on the switch, which is a person's gesture, and only then.
    let ext_cookies = ext.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let (Some(switch), Some(row), Some(label)) = (
            input("cookies"),
            by_id("cookies-row"),
            by_id("cookies-label"),
        ) else {
            return;
        };
        let query = Object::new();
        let _ = Reflect::set(&query, &"active".into(), &JsValue::TRUE);
        let _ = Reflect::set(&query, &"currentWindow".into(), &JsValue::TRUE);
        let url = call(&ext_cookies, "tabs", "query", &[query.into()])
            .await
            .and_then(|tabs| tabs.dyn_into::<Array>().ok())
            .map(|tabs| text(&tabs.get(0), "url"))
            .unwrap_or_default();
        let Some(site) = decide::site_of(&url).filter(|_| url.starts_with("http")) else {
            switch.set_disabled(true);
            switch.set_checked(false);
            row.set_class_name("row off");
            label.set_text_content(Some("No sign-in to hand over from this page"));
            return;
        };
        label.set_text_content(Some(&format!(
            "Hand over {site}'s sign-in with its downloads"
        )));
        let listed = sites(&stored(&ext_cookies, decide::COOKIES_SITES_KEY).await);
        switch.set_checked(decide::cookies_wanted(&listed, &url));
        let (ext, box_) = (ext_cookies.clone(), switch.clone());
        on_change(&switch, move || {
            let (ext, on, site, box_) = (ext.clone(), box_.checked(), site.clone(), box_.clone());
            wasm_bindgen_futures::spawn_local(async move {
                if on {
                    let wanted = Object::new();
                    let names: Array = [JsValue::from_str("cookies")].into_iter().collect();
                    let _ = Reflect::set(&wanted, &"permissions".into(), &names);
                    let granted = call(&ext, "permissions", "request", &[wanted.into()])
                        .await
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if !granted {
                        box_.set_checked(false);
                        say(
                            "Without the cookies permission, sign-ins stay with the browser",
                            true,
                        );
                        return;
                    }
                }
                let listed = sites(&stored(&ext, decide::COOKIES_SITES_KEY).await);
                let list: Array = decide::with_site(&listed, &site, on)
                    .iter()
                    .map(|s| JsValue::from_str(s))
                    .collect();
                store(&ext, decide::COOKIES_SITES_KEY, list.into()).await;
            });
        });
    });

    // Open dm: its window to the front, then the popup gets out of the way.
    if let Some(open) = by_id("open") {
        let ext = ext.clone();
        let closure = Closure::<dyn Fn()>::new(move || {
            let ext = ext.clone();
            wasm_bindgen_futures::spawn_local(async move {
                say("Opening dm…", false);
                let reply = ask(&ext, decide::OPEN_MESSAGE).await;
                let ok = Reflect::get(&reply, &"ok".into())
                    .ok()
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if ok {
                    say("dm is open", false);
                    if let Some(window) = web_sys::window() {
                        let _ = window.close();
                    }
                } else {
                    say(&text(&reply, "said"), true);
                }
            });
        });
        let _ = open.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
}
