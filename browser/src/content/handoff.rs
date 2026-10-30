//! A click on the panel: asking the background and handing the media over.

use super::*;

/// The player's media and the frame it is in, for a message to the
/// background, or `None` when the panel is already busy.
pub(super) fn asking() -> Option<(String, String)> {
    let (src, settled) = with_panel(|p| {
        let src = p
            .current
            .as_ref()
            .and_then(media_of)
            .map(|m| m.src)
            .unwrap_or_default();
        (src, p.settled)
    })?;
    if settled {
        return None;
    }
    let frame = window()
        .and_then(|w| w.location().href().ok())
        .unwrap_or_default();
    Some((src, frame))
}

/// How long an answer asked for ahead of a click is still the one to give.
const READ_AHEAD_MS: f64 = 120_000.0;

/// The app's answer about the page's qualities: the one asked for when the
/// panel was raised if it is still fresh, else a new ask. An answer that was
/// a refusal or never came is dropped, so the next click asks again.
fn reading(src: &str, frame: &str) -> Promise {
    let now = js_sys::Date::now();
    let kept = READING.with(|r| {
        r.borrow()
            .as_ref()
            .filter(|(page, at, _)| page == frame && now - at < READ_AHEAD_MS)
            .map(|(_, _, promise)| promise.clone())
    });
    if let Some(promise) = kept {
        return promise;
    }
    let asked = message(decide::FORMATS_MESSAGE, src, frame);
    let page = frame.to_string();
    let promise = wasm_bindgen_futures::future_to_promise(async move {
        let reply = send(asked.into()).await;
        let ok = reply
            .as_ref()
            .ok()
            .and_then(|r| Reflect::get(r, &"ok".into()).ok())
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !ok {
            READING.with(|r| {
                let mut r = r.borrow_mut();
                if r.as_ref().is_some_and(|(p, _, _)| *p == page) {
                    *r = None;
                }
            });
        }
        reply
    });
    READING.with(|r| *r.borrow_mut() = Some((frame.to_string(), now, promise.clone())));
    promise
}

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
        PLAYER_ASKED.with(|a| *a.borrow_mut() = Some((token.clone(), resolve.clone())));
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

/// The panel's lines for heights the player named, before sizes are
/// known: "1080p" each, fetched at that height.
fn lines_of_heights(heights: &[u32]) -> Array {
    let out = Array::new();
    for height in heights {
        let choice = js_sys::Object::new();
        let _ = Reflect::set(&choice, &"line".into(), &format!("{height}p").into());
        let _ = Reflect::set(&choice, &"quality".into(), &height.to_string().into());
        out.push(&choice);
    }
    out
}

/// Asks for the page's qualities now, so they are there at the click.
pub(super) fn read_ahead(src: &str) {
    let frame = window()
        .and_then(|w| w.location().href().ok())
        .unwrap_or_default();
    let _ = reading(src, &frame);
}

pub(super) fn message(kind: &str, src: &str, frame: &str) -> js_sys::Object {
    let message = js_sys::Object::new();
    let _ = Reflect::set(&message, &"type".into(), &kind.into());
    let _ = Reflect::set(&message, &"src".into(), &src.into());
    let _ = Reflect::set(&message, &"frame".into(), &frame.into());
    message
}

pub(super) fn text_of(reply: &JsValue, key: &str) -> String {
    Reflect::get(reply, &key.into())
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default()
}

/// A click on the bar. A file is one quality and goes at once; a player
/// page's qualities are asked of the app first and, when there is more than
/// one, listed with their sizes.
pub(super) fn clicked() {
    // A copy-protected player: said, and nothing handed over.
    if with_panel(|p| {
        let protected = p.go.has_attribute("data-protected");
        if protected {
            say(p, decide::PROTECTED_SAID);
        }
        protected
    })
    .unwrap_or(false)
    {
        return;
    }
    let Some((src, frame)) = asking() else {
        return;
    };
    let page = with_panel(|p| !p.caret.has_attribute("hidden")).unwrap_or(false);
    if !page {
        hand(&src, &frame, None, None);
        return;
    }
    with_panel(|p| {
        p.settled = true;
        // Short enough for the bar's title, which cut the longer one to
        // "Reading the vi…".
        set_title(p, "Reading qualities…");
        close_list(p);
        // Said, so a wait is not taken for a hang: reading a video page can
        // take up to half a minute.
        say(
            p,
            "dm is reading this page's video qualities; this can take up to half a minute.",
        );
    });
    let pending = reading(&src, &frame);
    // YouTube's own player lists its qualities in a moment: shown at once,
    // and the app's reading, with each one's size, takes the list's place
    // when it comes. A list already clicked is left alone.
    // Whatever goes wrong behind the click (the background asleep, the app
    // not answering, a helper that never returns), the panel does not stay
    // on "Reading qualities…": past the longest the app may take, it says so
    // and gives the click back. A list the player gave stays.
    let answered = std::rc::Rc::new(std::cell::Cell::new(false));
    let fast = std::rc::Rc::new(std::cell::Cell::new(false));
    if let Some(id) = decide::youtube_video_id(&frame) {
        let (src, frame, fast) = (src.clone(), frame.clone(), std::rc::Rc::clone(&fast));
        let answered = std::rc::Rc::clone(&answered);
        wasm_bindgen_futures::spawn_local(async move {
            let Ok(data) = JsFuture::from(player_answer()).await else {
                return;
            };
            let heights = heights_answered(&data, &id);
            // Still waiting on the app (its answer not come, nothing handed
            // over), and a choice worth making.
            let waiting = !answered.get() && with_panel(|p| p.settled).unwrap_or(false);
            if heights.len() < 2 || !waiting {
                return;
            }
            fast.set(true);
            with_panel(|p| {
                p.settled = false;
                say(p, "");
            });
            list(&src, &frame, &lines_of_heights(&heights));
        });
    }
    {
        let answered = std::rc::Rc::clone(&answered);
        let fast = std::rc::Rc::clone(&fast);
        set_timeout(
            move || {
                if !answered.replace(true) && !fast.get() {
                    failed("dm did not answer in time. Try again, or right-click the video and choose Download media with dm.");
                }
            },
            decide::FORMATS_PATIENCE_MS,
        );
    }
    wasm_bindgen_futures::spawn_local(async move {
        let reply = JsFuture::from(pending).await;
        // A reply after the watchdog spoke is the old click's: dropped.
        if answered.replace(true) {
            return;
        }
        // The player's list was clicked already: the app has its hand-off.
        if fast.get() && with_panel(|p| p.settled).unwrap_or(false) {
            return;
        }
        let reply = match reply {
            Ok(reply) => reply,
            Err(e) => {
                web_sys::console::debug_2(&"dm: the panel's message was not delivered:".into(), &e);
                failed("dm was updated: reload this page to use it");
                return;
            }
        };
        let ok = Reflect::get(&reply, &"ok".into())
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !ok {
            failed(&text_of(&reply, "said"));
            return;
        }
        let choices: Array = Reflect::get(&reply, &"choices".into())
            .ok()
            .and_then(|v| v.dyn_into().ok())
            .unwrap_or_default();
        with_panel(|p| {
            p.settled = false;
            say(p, "");
        });
        match choices.length() {
            0 => hand(&src, &frame, None, None),
            1 => hand_choice(&src, &frame, &choices.get(0)),
            _ => list(&src, &frame, &choices),
        }
    });
}

/// The list: one line per quality, with its size.
pub(super) fn list(src: &str, frame: &str, choices: &Array) {
    let Some(document) = document() else {
        return;
    };
    let first = with_panel(|p| {
        set_title(p, "Download video");
        close_list(p);
        for choice in choices.iter() {
            let Ok(item) = document.create_element("button") else {
                continue;
            };
            item.set_class_name("item");
            let _ = item.set_attribute("role", "menuitem");
            item.set_text_content(Some(&text_of(&choice, "line")));
            let (src, frame) = (src.to_string(), frame.to_string());
            listen(&item, "click", move |event| {
                event.prevent_default();
                event.stop_propagation();
                hand_choice(&src, &frame, &choice);
            });
            let _ = p.list.append_child(&item);
        }
        open_list(p)
    })
    .flatten();
    focus(first);
}

/// One line of the list, handed over: a quality of the page the app's
/// yt-dlp reads, or a stream the page loaded, which goes by its own address.
pub(super) fn hand_choice(src: &str, frame: &str, choice: &JsValue) {
    let label = Some(text_of(choice, "label")).filter(|l| !l.is_empty());
    let url = text_of(choice, "url");
    if url.is_empty() {
        hand(src, frame, Some(text_of(choice, "quality")), label);
    } else {
        let sound = Some(text_of(choice, "sound")).filter(|s| !s.is_empty());
        hand_with(&url, frame, None, label, sound);
    }
}

pub(super) fn failed(said: &str) {
    with_panel(|p| {
        p.settled = true;
        p.wrap.set_class_name("wrap dead");
        set_title(p, "dm could not take it");
        say(p, said);
    });
    hide_soon(6000);
}

/// Hands the player's media to the app through the background, at the
/// quality picked; the app asks where to save it.
pub(super) fn hand(src: &str, frame: &str, quality: Option<String>, label: Option<String>) {
    hand_with(src, frame, quality, label, None);
}

/// As [`hand`], with the address of a sound the player fetched apart from
/// the picture at `src`, which the app fetches with it and joins.
pub(super) fn hand_with(
    src: &str,
    frame: &str,
    quality: Option<String>,
    label: Option<String>,
    sound: Option<String>,
) {
    let message = message(PANEL_MESSAGE, src, frame);
    if let Some(sound) = &sound {
        let _ = Reflect::set(&message, &"sound".into(), &sound.into());
    }
    if let Some(quality) = &quality {
        let _ = Reflect::set(&message, &"quality".into(), &quality.into());
    }
    if let Some(label) = &label {
        let _ = Reflect::set(&message, &"label".into(), &label.into());
    }
    with_panel(|p| {
        p.settled = true;
        close_list(p);
        set_title(p, "Sending to dm…");
    });
    wasm_bindgen_futures::spawn_local(async move {
        let answer = send(message.into()).await;
        let (ok, said) = match answer {
            Ok(reply) => (
                Reflect::get(&reply, &"ok".into())
                    .ok()
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                text_of(&reply, "said"),
            ),
            // The extension was updated or removed under this page.
            Err(e) => {
                web_sys::console::debug_2(&"dm: the panel's message was not delivered:".into(), &e);
                (false, "dm was updated: reload this page to use it".into())
            }
        };
        if !ok {
            failed(&said);
            return;
        }
        // dm could not take it and the browser is downloading it instead:
        // said as that, not as sent.
        let title = if said.starts_with("the browser is downloading it") {
            "Downloading in the browser"
        } else {
            "Sent to dm"
        };
        with_panel(|p| {
            p.wrap.set_class_name("wrap done");
            set_title(p, title);
            say(p, &said);
        });
        hide_soon(2500);
    });
}

pub(super) async fn send(message: JsValue) -> Result<JsValue, JsValue> {
    let runtime = Reflect::get(&ext(), &"runtime".into())?;
    let send: Function = Reflect::get(&runtime, &"sendMessage".into())?.dyn_into()?;
    let returned = send.call1(&runtime, &message)?;
    let promise: Promise = returned.dyn_into()?;
    JsFuture::from(promise).await
}

/// Starts the page script; called by `extension/content.js` once the
/// WebAssembly has loaded. Runs once per page and frame.
#[wasm_bindgen]
pub fn content_start(ext: JsValue) {
    EXT.with(|cell| *cell.borrow_mut() = ext);
    let Some(document) = document() else {
        return;
    };
    let global: JsValue = js_sys::global().into();
    if Reflect::has(&global, &"__dmPanel".into()).unwrap_or(false) {
        return;
    }
    let _ = Reflect::set(&global, &"__dmPanel".into(), &JsValue::TRUE);

    let Some(panel) = build(&document) else {
        return;
    };
    PANEL.with(|cell| *cell.borrow_mut() = Some(panel));
    // Kept off this site? Read now, and followed as the list changes, so
    // the toolbar popup turning it back on is in force without a reload.
    wasm_bindgen_futures::spawn_local(async {
        apply_off(&sites_off().await);
        apply_rules(&stored_rules().await);
    });
    let changed = Closure::<dyn Fn(JsValue, JsValue)>::new(|changes: JsValue, area: JsValue| {
        if area.as_string().as_deref() != Some("local") {
            return;
        }
        // The app's Options › Browser, in force without a reload.
        if let Ok(rules) = Reflect::get(&changes, &RULES_KEY.into()) {
            if rules.is_object() {
                apply_rules(
                    &Reflect::get(&rules, &"newValue".into()).unwrap_or(JsValue::UNDEFINED),
                );
            }
        }
        let Ok(change) = Reflect::get(&changes, &decide::PANEL_OFF_KEY.into()) else {
            return;
        };
        if change.is_object() {
            apply_off(&list_of(
                &Reflect::get(&change, &"newValue".into()).unwrap_or(JsValue::UNDEFINED),
            ));
        }
    });
    let on_changed = Reflect::get(&self::ext(), &"storage".into())
        .and_then(|storage| Reflect::get(&storage, &"onChanged".into()))
        .and_then(|event| Reflect::get(&event, &"addListener".into()).map(|add| (event, add)));
    if let Ok((event, add)) = on_changed {
        if let Ok(add) = add.dyn_into::<Function>() {
            let _ = add.call1(&event, changed.as_ref());
        }
    }
    changed.forget();

    listen(&document, "mousemove", on_pointer);
    // Alt-click and "Download all/selected links" are answered by
    // `extension/content.js` itself, before this loads: a page that forbids
    // WebAssembly still has them.
    // The keyboard's way in: the focus on a player raises its panel, and
    // Tab from it goes into the panel.
    listen(&document, "focusin", on_focus);
    listen(&document, "focusout", on_blur);
    let tab = Closure::<dyn Fn(Event)>::new(on_page_key);
    let _ = document.add_event_listener_with_callback_and_bool(
        "keydown",
        tab.as_ref().unchecked_ref(),
        true,
    );
    tab.forget();
    // Fullscreen: only the fullscreen element is drawn, so the panel goes
    // inside it, and back to the body after.
    listen(&document, "fullscreenchange", on_fullscreen);
    // Capturing, because `play` does not bubble.
    let play = Closure::<dyn Fn(Event)>::new(on_play);
    let _ = document.add_event_listener_with_callback_and_bool(
        "play",
        play.as_ref().unchecked_ref(),
        true,
    );
    play.forget();
    // Capturing, so a scroll inside any container moves the panel too.
    let moved = Closure::<dyn Fn(Event)>::new(on_move);
    let _ = document.add_event_listener_with_callback_and_bool(
        "scroll",
        moved.as_ref().unchecked_ref(),
        true,
    );
    moved.forget();
    if let Some(window) = window() {
        listen(&window, "resize", on_move);
    }
}
