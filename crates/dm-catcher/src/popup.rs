// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
//!   not left, and the row keeps them. A video page's session goes with
//!   its video either way, for that download alone (`decide::cookies_sent`).
//!
//! A switch turned off on part of a listed site turns it off for the listed
//! site too, and says so.
//! - **Open dm**: the app's window to the front, or the app started; not
//!   shown when the browser has no host for dm, which it would need.
//! - What the app is fetching, each with Stop, and under "Recent" what it stopped or finished
//!   last, each with Open and Open folder, Resume or Retry, and Clear finished (gaps row C12):
//!   the buttons only where the app says it acts on downloads.
//!
//! The page is `extension/popup.html`; this wires it. What a site is, and
//! whether the panel is kept off one, is decided in [`crate::decide`].

use js_sys::{Array, Function, Object, Promise, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Document, Element, HtmlInputElement};

use crate::decide;
use crate::words::{self, Word};

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

/// The page's own words (`popup.html`, each marked `data-i18n` with its key) in the browser's
/// language, and the page's `lang` with them, so a screen reader reads them in it. A key the
/// list does not know leaves its English.
fn in_browser_language() {
    let Some(document) = document() else {
        return;
    };
    if let (Some(root), Some(language)) = (document.document_element(), words::ui_language()) {
        let _ = root.set_attribute("lang", &language);
    }
    let Ok(marked) = document.query_selector_all("[data-i18n]") else {
        return;
    };
    for at in 0..marked.length() {
        let Some(element) = marked.item(at).and_then(|n| n.dyn_into::<Element>().ok()) else {
            continue;
        };
        let key = element.get_attribute("data-i18n").unwrap_or_default();
        if let Some(word) = words::ALL.iter().find(|w| w.key == key) {
            element.set_text_content(Some(&word.say()));
        }
    }
}

fn on_change(element: &Element, handler: impl Fn() + 'static) {
    let closure = Closure::<dyn Fn()>::new(handler);
    let _ = element.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref());
    closure.forget();
}

fn as_array(list: &[String]) -> JsValue {
    let array: Array = list.iter().map(|s| JsValue::from_str(s)).collect();
    array.into()
}

/// The address of the tab in front.
async fn active_url(ext: &JsValue) -> String {
    let query = Object::new();
    let _ = Reflect::set(&query, &"active".into(), &JsValue::TRUE);
    let _ = Reflect::set(&query, &"currentWindow".into(), &JsValue::TRUE);
    call(ext, "tabs", "query", &[query.into()])
        .await
        .and_then(|tabs| tabs.dyn_into::<Array>().ok())
        .map(|tabs| text(&tabs.get(0), "url"))
        .unwrap_or_default()
}

/// One of the three per-site switches.
struct SiteSwitch {
    input: &'static str,
    row: &'static str,
    label: &'static str,
    /// The storage key of its list of sites.
    key: &'static str,
    /// Whether a listed site reads as on (the panel's list is of sites it
    /// is kept off).
    listed_is_on: bool,
    /// The label, its gap filled with the site.
    says: Word,
    /// The label on a page that is not a site.
    none: Word,
}

/// Draws a per-site switch for the tab in front and wires it. Turned off
/// on part of a listed site (music.youtube.com under youtube.com), it takes
/// out the entry that had it on, and says so: taking out only the exact
/// site left the switch on.
async fn site_switch(ext: JsValue, switch: SiteSwitch) {
    let (Some(input), Some(row), Some(label)) =
        (input(switch.input), by_id(switch.row), by_id(switch.label))
    else {
        return;
    };
    let url = active_url(&ext).await;
    let Some(site) = decide::site_of(&url).filter(|_| url.starts_with("http")) else {
        input.set_disabled(true);
        input.set_checked(false);
        row.set_class_name("row off");
        label.set_text_content(Some(&switch.none.say()));
        return;
    };
    label.set_text_content(Some(&switch.says.with(&[&site])));
    let listed = sites(&stored(&ext, switch.key).await);
    input.set_checked(decide::site_listed(&listed, &url) == switch.listed_is_on);
    let box_ = input.clone();
    on_change(&input, move || {
        let (ext, on, site) = (ext.clone(), box_.checked(), site.clone());
        let (key, listed_is_on) = (switch.key, switch.listed_is_on);
        wasm_bindgen_futures::spawn_local(async move {
            let listed = sites(&stored(&ext, key).await);
            let add = on == listed_is_on;
            let note = if add {
                None
            } else {
                decide::wider_note(&listed, &site)
            };
            let list = decide::with_site(&listed, &site, add);
            store(&ext, key, as_array(&list)).await;
            if let Some(note) = note {
                say(&note, false);
            }
        });
    });
}

/// The sign-in switch: on where the site is listed. The browser grants the
/// `cookies` permission with the extension, so switching it on only lists
/// the site.
async fn cookies_switch(ext: JsValue) {
    let (Some(input), Some(row), Some(label)) = (
        input("cookies"),
        by_id("cookies-row"),
        by_id("cookies-label"),
    ) else {
        return;
    };
    let url = active_url(&ext).await;
    let Some(site) = decide::site_of(&url).filter(|_| url.starts_with("http")) else {
        input.set_disabled(true);
        input.set_checked(false);
        row.set_class_name("row off");
        label.set_text_content(Some(&words::POPUP_COOKIES_NONE.say()));
        return;
    };
    label.set_text_content(Some(&words::POPUP_COOKIES_ON.with(&[&site])));
    let listed = sites(&stored(&ext, decide::COOKIES_SITES_KEY).await);
    input.set_checked(decide::cookies_wanted(&listed, &url));
    // Kept here, so a switch-on can store the site without waiting.
    let known = std::rc::Rc::new(std::cell::RefCell::new(listed));
    let box_ = input.clone();
    on_change(&input, move || {
        let on = box_.checked();
        let before = known.borrow().clone();
        let note = if on {
            None
        } else {
            decide::wider_note(&before, &site)
        };
        let after = decide::with_site(&before, &site, on);
        (*known.borrow_mut()).clone_from(&after);
        let ext = ext.clone();
        wasm_bindgen_futures::spawn_local(async move {
            store(&ext, decide::COOKIES_SITES_KEY, as_array(&after)).await;
            if let Some(note) = note {
                say(&note, false);
            }
        });
    });
}

/// The extension's API: `browser` where there is one (Firefox, promise-based), else `chrome`.
fn extension() -> JsValue {
    let global: JsValue = js_sys::global().into();
    let browser = Reflect::get(&global, &"browser".into()).unwrap_or(JsValue::UNDEFINED);
    if browser.is_object() {
        browser
    } else {
        Reflect::get(&global, &"chrome".into()).unwrap_or(JsValue::UNDEFINED)
    }
}

/// Shows what the background said of the app; true when the app answered.
fn show_status(reply: &JsValue) -> bool {
    let flag = |key: &str, otherwise: bool| {
        Reflect::get(reply, &key.into())
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(otherwise)
    };
    let shown = |id: &str, on: bool| {
        if let Some(element) = by_id(id) {
            let _ = if on {
                element.remove_attribute("hidden")
            } else {
                element.set_attribute("hidden", "")
            };
        }
    };
    if flag("ok", false) {
        say(&text(reply, "line"), flag("bad", false));
        shown("steps", false);
        shown("advanced", false);
        shown("get-actions", false);
        shown("open-actions", true);
        return true;
    }
    // No host: Open dm could not reach dm, so it is not offered; the steps say how to connect,
    // with where to get the app, and the command folded under them.
    let host = flag("host", true);
    shown("steps", !host);
    shown("open-actions", host);
    shown("get-actions", !host);
    let advanced = text(reply, "advanced");
    if let Some(line) = by_id("advanced") {
        line.set_text_content(Some(&advanced));
    }
    shown("advanced", !host && !advanced.is_empty());
    let said = text(reply, "said");
    let said = if said.is_empty() {
        words::POPUP_NO_ANSWER.say()
    } else {
        said
    };
    say(&said, true);
    false
}

/// Shows `element` where `on`, hides it where not.
fn shown_if(element: &Element, on: bool) {
    let _ = if on {
        element.remove_attribute("hidden")
    } else {
        element.set_attribute("hidden", "")
    };
}

/// Asks the background to have the app do `does` to download `id` (none: the whole list's), then
/// lists again what the app has; says why where it was not done.
fn act(ext: &JsValue, does: &str, id: Option<u64>) {
    let (ext, does) = (ext.clone(), does.to_string());
    wasm_bindgen_futures::spawn_local(async move {
        let message = Object::new();
        let _ = Reflect::set(&message, &"type".into(), &decide::ACT_MESSAGE.into());
        let _ = Reflect::set(&message, &"does".into(), &does.into());
        if let Some(id) = id {
            let _ = Reflect::set(&message, &"id".into(), &JsValue::from_f64(id as f64));
        }
        let reply = call(&ext, "runtime", "sendMessage", &[message.into()])
            .await
            .unwrap_or(JsValue::UNDEFINED);
        let ok = Reflect::get(&reply, &"ok".into())
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !ok {
            let said = text(&reply, "said");
            let said = if said.is_empty() {
                words::POPUP_NO_ANSWER.say()
            } else {
                said
            };
            say(&said, true);
        }
        show_downloads(&ask(&ext, decide::DOWNLOADS_MESSAGE).await);
    });
}

/// A button that asks the app to do `does` to download `id`: what it does and to which row are
/// kept on it, for its list's one listener ([`on_list_click`]), so a list drawn again every two
/// seconds adds no listener.
fn act_button(document: &Document, label: &str, does: &str, id: Option<u64>) -> Option<Element> {
    let button = document.create_element("button").ok()?;
    button.set_class_name("small");
    let _ = button.set_attribute("type", "button");
    let _ = button.set_attribute("data-does", does);
    if let Some(id) = id {
        let _ = button.set_attribute("data-id", &id.to_string());
    }
    button.set_text_content(Some(label));
    Some(button)
}

/// The one listener of the list `id`: a click on one of its buttons ([`act_button`]) asks the app.
fn on_list_click(ext: &JsValue, id: &str) {
    let Some(list) = by_id(id) else {
        return;
    };
    let ext = ext.clone();
    let closure = Closure::<dyn Fn(web_sys::Event)>::new(move |event: web_sys::Event| {
        let Some(button) = event.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
            return;
        };
        let Some(does) = button.get_attribute("data-does") else {
            return;
        };
        let row = button
            .get_attribute("data-id")
            .and_then(|id| id.parse().ok());
        act(&ext, &does, row);
    });
    let _ = list.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
    closure.forget();
}

/// Fills the list `id` with `rows`, a line each with its buttons; hidden where there are none.
fn fill(document: &Document, id: &str, rows: &[decide::PopupRow]) {
    let Some(list) = by_id(id) else {
        return;
    };
    list.set_text_content(None);
    for row in rows {
        let Ok(item) = document.create_element("li") else {
            continue;
        };
        if let Ok(name) = document.create_element("span") {
            name.set_class_name("name");
            name.set_text_content(Some(&row.line));
            let _ = name.set_attribute("title", &row.line);
            let _ = item.append_child(&name);
        }
        for button in &row.buttons {
            if let Some(button) = act_button(document, &button.label, &button.does, row.id) {
                let _ = item.append_child(&button);
            }
        }
        let _ = list.append_child(&item);
    }
    shown_if(&list, !rows.is_empty());
}

/// Lists what the background said the app is fetching and has fetched (`decide::popup_list`): a
/// line a download, then the last failure, then the recent downloads under their heading with
/// Clear finished; each hidden where there is nothing.
fn show_downloads(reply: &JsValue) {
    let Some(document) = document() else {
        return;
    };
    let listed: decide::PopupList = js_sys::JSON::stringify(reply)
        .ok()
        .and_then(|json| json.as_string())
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    fill(&document, "going", &listed.going);
    if let Some(line) = by_id("failed") {
        let failed = listed.failed.unwrap_or_default();
        line.set_text_content(Some(&failed));
        shown_if(&line, !failed.is_empty());
    }
    fill(&document, "recent", &listed.recent);
    if let Some(head) = by_id("recent-head") {
        shown_if(&head, !listed.recent.is_empty());
    }
    if let Some(clear) = by_id("clear") {
        if let Some(label) = &listed.clear {
            clear.set_text_content(Some(label));
        }
        shown_if(&clear, listed.clear.is_some());
    }
}

/// Waits `ms` milliseconds.
async fn pause(ms: i32) {
    let promise = Promise::new(&mut |done, _| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&done, ms);
        }
    });
    let _ = JsFuture::from(promise).await;
}

/// Wires the popup, once its WebAssembly has loaded (`pkg/popup.js`, which `scripts/package.sh`
/// writes, starts it).
#[wasm_bindgen]
pub fn popup_start() {
    in_browser_language();
    let ext = extension();
    // The app, asked through the background, which owns the connection; asked again every two
    // seconds until it answers, so opening dm connects the popup without reopening it (gaps row
    // U18).
    // Once it answers, what it is fetching is listed, and kept up to date while the popup is open.
    let asked = ext.clone();
    wasm_bindgen_futures::spawn_local(async move {
        loop {
            if show_status(&ask(&asked, decide::STATUS_MESSAGE).await) {
                show_downloads(&ask(&asked, decide::DOWNLOADS_MESSAGE).await);
            } else {
                show_downloads(&JsValue::UNDEFINED);
            }
            pause(decide::POPUP_ASK_AGAIN_MS).await;
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

    // The panel on this site, by the same rule the page script keeps it
    // off by.
    wasm_bindgen_futures::spawn_local(site_switch(
        ext.clone(),
        SiteSwitch {
            input: "panel",
            row: "panel-row",
            label: "panel-label",
            key: decide::PANEL_OFF_KEY,
            listed_is_on: false,
            says: words::POPUP_PANEL_ON,
            none: words::POPUP_PANEL_NONE,
        },
    ));

    // Every download from this site, whatever its type: the worker reads
    // the same list before it asks the app's rules.
    wasm_bindgen_futures::spawn_local(site_switch(
        ext.clone(),
        SiteSwitch {
            input: "catchall",
            row: "catchall-row",
            label: "catchall-label",
            key: decide::CATCH_ALL_KEY,
            listed_is_on: true,
            says: words::POPUP_CATCH_ALL_ON,
            none: words::POPUP_CATCH_ALL_NONE,
        },
    ));

    // Capturing paused for this site: its downloads stay with the browser.
    wasm_bindgen_futures::spawn_local(site_switch(
        ext.clone(),
        SiteSwitch {
            input: "paused",
            row: "paused-row",
            label: "paused-label",
            key: decide::PAUSED_KEY,
            listed_is_on: true,
            says: words::POPUP_PAUSED_ON,
            none: words::POPUP_PAUSED_NONE,
        },
    ));

    // This site's sign-in with its downloads.
    wasm_bindgen_futures::spawn_local(cookies_switch(ext.clone()));

    // The buttons beside the downloads listed.
    on_list_click(&ext, "going");
    on_list_click(&ext, "recent");

    // Clear finished: the finished downloads off dm's list, as its Delete Completed does.
    if let Some(clear) = by_id("clear") {
        let ext = ext.clone();
        let closure = Closure::<dyn Fn()>::new(move || {
            act(&ext, dm_protocol::act_does::CLEAR_FINISHED, None)
        });
        let _ = clear.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }

    // Open dm: its window to the front, then the popup gets out of the way.
    if let Some(open) = by_id("open") {
        let ext = ext;
        let closure = Closure::<dyn Fn()>::new(move || {
            let ext = ext.clone();
            wasm_bindgen_futures::spawn_local(async move {
                say(&words::POPUP_OPENING.say(), false);
                let reply = ask(&ext, decide::OPEN_MESSAGE).await;
                let ok = Reflect::get(&reply, &"ok".into())
                    .ok()
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if ok {
                    say(&words::POPUP_IS_OPEN.say(), false);
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
