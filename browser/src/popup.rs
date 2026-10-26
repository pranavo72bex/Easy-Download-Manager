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
    /// The label, given the site.
    says: fn(&str) -> String,
    /// The label on a page that is not a site.
    none: &'static str,
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
        label.set_text_content(Some(switch.none));
        return;
    };
    label.set_text_content(Some(&(switch.says)(&site)));
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
        label.set_text_content(Some("No sign-in to hand over from this page"));
        return;
    };
    label.set_text_content(Some(&format!(
        "Hand over {site}'s sign-in with its downloads"
    )));
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
        *known.borrow_mut() = after.clone();
        let ext = ext.clone();
        wasm_bindgen_futures::spawn_local(async move {
            store(&ext, decide::COOKIES_SITES_KEY, as_array(&after)).await;
            if let Some(note) = note {
                say(&note, false);
            }
        });
    });
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
            let bad = Reflect::get(&reply, &"bad".into())
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            say(&text(&reply, "line"), bad);
        } else {
            // No host: Open dm could not reach dm, so it is not offered;
            // the status says how to connect instead.
            let host = Reflect::get(&reply, &"host".into())
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            // with no app to open, where to get one.
            if !host {
                if let Some(open) = by_id("open-actions") {
                    let _ = open.set_attribute("hidden", "");
                }
                if let Some(get) = by_id("get-actions") {
                    let _ = get.remove_attribute("hidden");
                }
            }
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
            says: |site| format!("Show “Download video” on {site}"),
            none: "No video panel on this page",
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
            says: |site| format!("Take every download from {site}"),
            none: "No downloads to take from this page",
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
            says: |site| format!("Pause capturing on {site}"),
            none: "No downloads to pause on this page",
        },
    ));

    // This site's sign-in with its downloads.
    wasm_bindgen_futures::spawn_local(cookies_switch(ext.clone()));

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
