// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The page's own player asked: which video it is, and the heights it lists, for the panel's list.

use super::*;

thread_local! {
    /// Who is waiting for the page's own player to list its qualities:
    /// the ask's token and the promise's resolve.
    static PLAYER_ASKED: RefCell<Option<(String, Function)>> = const { RefCell::new(None) };
    /// Whether the page's answers are listened for yet.
    static PLAYER_LISTENING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// How long the page's player is given to answer: it answers in a frame.
const PLAYER_PATIENCE_MS: i32 = 400;

/// What YouTube's own player says of the video `id`, asked in the page's
/// world (`document.js`): the heights it lists, in a moment, so the panel
/// has a list before the app's reading with sizes arrives. Empty where the
/// player is another video's, not YouTube's, or does not answer.
pub(super) fn player_answer() -> Promise {
    let Some(window) = window() else {
        return Promise::resolve(&JsValue::NULL);
    };
    if !PLAYER_LISTENING.with(|l| l.replace(true)) {
        listen(&window, "message", |event| {
            let Ok(event) = event.dyn_into::<web_sys::MessageEvent>() else {
                return;
            };
            let data = event.data();
            if text_of(&data, "source") != "dm-player-qualities" {
                return;
            }
            let token = text_of(&data, "token");
            let waiting = PLAYER_ASKED.with(|a| {
                let mut a = a.borrow_mut();
                match a.as_ref() {
                    Some((asked, _)) if *asked == token => a.take(),
                    _ => None,
                }
            });
            if let Some((_, resolve)) = waiting {
                let _ = resolve.call1(&JsValue::NULL, &data);
            }
        });
    }
    let token = format!("q{}", js_sys::Date::now());
    let origin = window.location().origin().unwrap_or_else(|_| "*".into());
    Promise::new(&mut |resolve, _reject| {
        // An ask still waiting is answered with nothing rather than left
        // unanswered for good, whoever awaits it.
        let older = PLAYER_ASKED.with(|a| a.borrow_mut().replace((token.clone(), resolve.clone())));
        if let Some((_, older)) = older {
            let _ = older.call1(&JsValue::NULL, &JsValue::NULL);
        }
        let ask = js_sys::Object::new();
        let _ = Reflect::set(&ask, &"source".into(), &"dm-ask-qualities".into());
        let _ = Reflect::set(&ask, &"token".into(), &token.clone().into());
        let _ = window.post_message(&ask, &origin);
        let token = token.clone();
        set_timeout(
            move || {
                // Unanswered: nothing, and the next click asks afresh.
                let late = PLAYER_ASKED.with(|a| {
                    let mut a = a.borrow_mut();
                    match a.as_ref() {
                        Some((asked, _)) if *asked == token => a.take(),
                        _ => None,
                    }
                });
                if let Some((_, resolve)) = late {
                    let _ = resolve.call1(&JsValue::NULL, &JsValue::NULL);
                }
            },
            PLAYER_PATIENCE_MS,
        );
    })
}

thread_local! {
    /// Who is waiting for the page to say which video the player is: the
    /// ask's token and the promise's resolve.
    static ID_ASKED: RefCell<Option<(String, Function)>> = const { RefCell::new(None) };
    /// Whether the page's id answers are listened for yet.
    static ID_LISTENING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Which video the player under the panel is, by the finders the app's
/// rules give this site (`document.js` reads them): its id, or empty where
/// the site has no finder, the player matches none, or the page does not
/// answer in a moment.
pub(super) async fn player_id() -> String {
    let Some(window) = window() else {
        return String::new();
    };
    let Some(Some(element)) = with_panel(|p| p.current.clone()) else {
        return String::new();
    };
    if !ID_LISTENING.with(|l| l.replace(true)) {
        listen(&window, "message", |event| {
            let Ok(event) = event.dyn_into::<web_sys::MessageEvent>() else {
                return;
            };
            let data = event.data();
            if text_of(&data, "source") != "dm-player-id" {
                return;
            }
            let token = text_of(&data, "token");
            let waiting = ID_ASKED.with(|a| {
                let mut a = a.borrow_mut();
                match a.as_ref() {
                    Some((asked, _)) if *asked == token => a.take(),
                    _ => None,
                }
            });
            if let Some((_, resolve)) = waiting {
                let _ = resolve.call1(&JsValue::NULL, &data);
            }
        });
    }
    let token = format!("f{}", js_sys::Date::now() as u64);
    let origin = window.location().origin().unwrap_or_else(|_| "*".into());
    // The element is marked for the page script to find, for this one ask.
    let _ = element.set_attribute("data-dm-find", &token);
    let asked = Promise::new(&mut |resolve, _reject| {
        ID_ASKED.with(|a| *a.borrow_mut() = Some((token.clone(), resolve.clone())));
        let ask = js_sys::Object::new();
        let _ = Reflect::set(&ask, &"source".into(), &"dm-find-id".into());
        let _ = Reflect::set(&ask, &"token".into(), &token.clone().into());
        let _ = window.post_message(&ask, &origin);
        let token = token.clone();
        set_timeout(
            move || {
                let late = ID_ASKED.with(|a| {
                    let mut a = a.borrow_mut();
                    match a.as_ref() {
                        Some((asked, _)) if *asked == token => a.take(),
                        _ => None,
                    }
                });
                if let Some((_, resolve)) = late {
                    let _ = resolve.call1(&JsValue::NULL, &JsValue::NULL);
                }
            },
            PLAYER_PATIENCE_MS,
        );
    });
    let answer = JsFuture::from(asked).await.unwrap_or(JsValue::NULL);
    element.remove_attribute("data-dm-find").ok();
    if answer.is_null() {
        return String::new();
    }
    text_of(&answer, "id")
}

/// The heights the player's answer names for the video `id`, tallest
/// first; none where the answer is another video's (the player is reused
/// for the next video, and for an advertisement) or never came.
fn heights_answered(data: &JsValue, id: &str) -> Vec<u32> {
    if text_of(data, "id") != id {
        return Vec::new();
    }
    let levels: Vec<String> = Reflect::get(data, &"levels".into())
        .ok()
        .and_then(|v| v.dyn_into::<Array>().ok())
        .map(|a| a.iter().filter_map(|l| l.as_string()).collect())
        .unwrap_or_default();
    decide::player_heights(&levels)
}

/// A promise of the heights YouTube's player lists for the video `id`, as
/// numbers, tallest first: asked again every [`decide::PLAYER_ASK_EVERY_MS`]
/// until it lists two or more, `stop` is set, or
/// [`decide::PLAYER_LIST_PATIENCE_MS`] has passed. A player still loading,
/// or playing an advertisement first, has no list of the video's own yet,
/// and a click asked it only once (gaps row 178).
pub(super) fn player_list(id: String, stop: std::rc::Rc<std::cell::Cell<bool>>) -> Promise {
    wasm_bindgen_futures::future_to_promise(async move {
        let until = js_sys::Date::now() + decide::PLAYER_LIST_PATIENCE_MS;
        loop {
            let data = JsFuture::from(player_answer())
                .await
                .unwrap_or(JsValue::NULL);
            let heights = heights_answered(&data, &id);
            if heights.len() >= 2 || stop.get() || js_sys::Date::now() >= until {
                return Ok(heights
                    .iter()
                    .map(|h| JsValue::from(*h))
                    .collect::<Array>()
                    .into());
            }
            let pause = Promise::new(&mut |resolve, _reject| {
                set_timeout(
                    move || {
                        let _ = resolve.call0(&JsValue::NULL);
                    },
                    decide::PLAYER_ASK_EVERY_MS,
                );
            });
            let _ = JsFuture::from(pause).await;
        }
    })
}

/// The heights a [`player_list`] promise came to; none where it failed.
pub(super) async fn heights_listed(list: Promise) -> Vec<u32> {
    JsFuture::from(list)
        .await
        .ok()
        .and_then(|v| v.dyn_into::<Array>().ok())
        .map(|a| {
            a.iter()
                .filter_map(|h| h.as_f64())
                .map(|h| h as u32)
                .collect()
        })
        .unwrap_or_default()
}

/// The panel's lines for heights the player named, before sizes are
/// known: "1080p" each, fetched at that height.
pub(super) fn lines_of_heights(heights: &[u32]) -> Array {
    let out = Array::new();
    for height in heights {
        let choice = js_sys::Object::new();
        let _ = Reflect::set(&choice, &"line".into(), &format!("{height}p").into());
        let _ = Reflect::set(&choice, &"quality".into(), &height.to_string().into());
        out.push(&choice);
    }
    out
}
