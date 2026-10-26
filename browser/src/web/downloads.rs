//! A download the browser started: deciding whether the app takes it.

use super::*;

#[wasm_bindgen(start)]
pub fn started() {
    std::panic::set_hook(Box::new(|panic| log("error", &panic.to_string())));
}

/// A download the browser has begun. Taken when the app wants it and takes
/// it; otherwise left exactly as it was.
#[wasm_bindgen]
pub async fn on_download_created(item: JsValue) {
    let item: DownloadItem = serde_json::from_value(from_js(&item)).unwrap_or_default();
    // However this ends, Chrome's question about the file's name is
    // answered, or dropped with the download dm took.
    let mut decided = Decided {
        id: item.id.map(|n| n as i64),
        taken: false,
    };
    // The toolbar popup's switch: off, the browser keeps every download.
    if !catching().await {
        return log("debug", "left with the browser: catching is switched off");
    }
    let found = match first_look(&item, &own_id()) {
        Ok(found) => found,
        Err(why) => return log("debug", &format!("left with the browser: {why}")),
    };
    let asked_for = item.url.clone().unwrap_or_default();
    if RELEASED.with(|r| {
        let mut released = r.borrow_mut();
        released.take(&found.url, now()) || released.take(&asked_for, now())
    }) {
        return log(
            "debug",
            "left with the browser: given back to it a moment ago",
        );
    }

    // Held while the app is asked, so the browser does not finish it in the
    // meantime and leave the file in two places. When the rules the app last
    // sent would catch it, it is held before the app is asked at all:
    // starting the host takes long enough for a small download to finish.
    let id = JsValue::from_f64(found.id as f64);
    let pause = || async {
        call("downloads", "pause", std::slice::from_ref(&id))
            .await
            .is_ok()
    };
    // With nothing remembered yet (before the app has ever answered), every
    // candidate is held; one the app turns out not to want is given back.
    // A site the popup takes everything from is held whatever the rules say
    // about type and size; the app's own switch and skip list still hold.
    let clicked = TAKEN.with(|t| {
        let mut taken = t.borrow_mut();
        taken.take(&found.url, now()) || taken.take(&asked_for, now())
    });
    let forced = clicked || forced_for(&found).await;
    let early = remembered_rules()
        .await
        .is_none_or(|rules| rules.takes(&found.candidate(), forced).is_ok());
    let mut paused = early && pause().await;

    let (app, port) = app().await;
    let verdict = match &app {
        App::Present { rules, hello } => rules
            .takes(&found.candidate(), forced)
            .map_err(|left| left.reason().to_string())
            .and_then(|()| {
                // An app too old to tell a sign-in page from the file would
                // take the page, and the browser's copy would be erased.
                decide::takes_caught(hello).map_err(|old| {
                    if !TOLD_OLD.with(|t| t.replace(true)) {
                        notify("Update the dm app", old);
                    }
                    old.to_string()
                })
            }),
        App::Absent { .. } => Err("no app to hand it to".to_string()),
    };
    if let Err(why) = verdict {
        if paused {
            give_back(&id, &found.url).await;
        }
        // Said when the browser keeps a download that dm would have taken
        // had it been reachable; a type dm does not catch is not news.
        if matches!(app, App::Absent { .. }) {
            notify("The browser is downloading it", "dm could not be reached");
        }
        return log("debug", &format!("left with the browser: {why}"));
    }

    if !paused {
        paused = pause().await;
    }
    // Not held: it may have finished already (a small file served fast, a
    // PDF in Chromium above all); that still goes, by its address.
    if !paused {
        if let Err(why) = decide::unheld(state_of(found.id).await.as_deref()) {
            return log("debug", &format!("left with the browser: {why}"));
        }
    }

    // The site's sign-in, where the popup has that on for the site, or
    // the app's Options for every site.
    let (cookies, cookies_once) = sign_in_for(&found.url, found.store.as_deref(), false).await;
    let start = Start {
        id: hand_id("b", found.id, now()),
        url: found.url.clone(),
        referrer: found.referrer.clone(),
        // The browser holds it: dm checks it can fetch it before taking it,
        // and only an app that judges what it is served may take it.
        caught: true,
        held: true,
        cookies,
        cookies_once,
        // What the browser is being sent, so the app can tell a sign-in
        // page served to it in the file's place, and refuse.
        mime: found.mime.clone(),
        ..Start::default()
    };
    // dm refused for want of the browser's session (a link only a signed-in
    // browser can fetch): handed over again with that session, for this one
    // download, rather than left to download in the browser.
    let mut outcome = hand(port, start.clone()).await;
    if outcome
        .as_ref()
        .is_err_and(|why| why.contains("dm cannot fetch this without the browser"))
        && start.cookies.is_empty()
    {
        let (cookies, _) = listed_sign_in(&found.url, found.store.as_deref(), true, true).await;
        if !cookies.is_empty() {
            outcome = hand(
                None,
                Start {
                    id: hand_id("b", found.id, now()),
                    cookies,
                    cookies_once: true,
                    ..start
                },
            )
            .await;
        }
    }
    match outcome {
        Ok(()) => {
            log("info", &format!("handed {} to the dm app", found.url));
            // Said in the browser, where the download just vanished; dm's
            // own window may be behind it.
            notify(
                "dm is downloading it",
                &decide::taken_name(&found.filename, &found.url),
            );
            // Held or not, it can have finished in the browser meanwhile:
            // a pause does not stop what Chromium has already received.
            let (verb, what) = match decide::after_taken(state_of(found.id).await.as_deref()) {
                decide::BrowserCopy::Cancel => ("cancelled", "cancel"),
                decide::BrowserCopy::Remove => ("removed", "removeFile"),
            };
            let mut done = call("downloads", what, std::slice::from_ref(&id)).await;
            // Finished between the look and the cancel: a fast file (a
            // 300 KB PDF) stayed whole in the browser's folder beside dm's,
            // its list entry erased below. Chromium's
            // cancel of a finished download succeeds and does nothing, so
            // the state is looked at after every cancel, and a finished copy
            // removed.
            if what == "cancel" {
                for _ in 0..20 {
                    match state_of(found.id).await.as_deref() {
                        Some("complete") => {
                            done = call("downloads", "removeFile", std::slice::from_ref(&id)).await;
                            break;
                        }
                        Some("in_progress") => sleep(100.0).await,
                        // Interrupted, as a cancel that took leaves it, or gone.
                        _ => break,
                    }
                }
            }
            match done {
                // Gone from the browser before it named the file: its Save
                // dialog, where it would have asked, never opens.
                Ok(_) => decided.taken = true,
                Err(why) => log(
                    "warn",
                    &format!("the browser's copy could not be {verb}: {why}"),
                ),
            }
            let query = to_js(&serde_json::json!({ "id": found.id }));
            let _ = call("downloads", "erase", &[query]).await;
        }
        Err(why) => {
            log(
                "warn",
                &format!(
                    "the dm app did not take {}, so the browser keeps it: {why}",
                    found.url
                ),
            );
            notify("The browser is downloading it", &why);
            if paused {
                give_back(&id, &found.url).await;
            }
        }
    }
}

/// Chrome asking what to name a download (Chrome only; Firefox has no such
/// event). Held until `on_download_created` has decided, so a download dm
/// takes is gone before Chrome would open its Save dialog for it: it asks
/// when the name is too long for the disk, or when "Ask where to save each
/// file" is on, and a dialog opened before the cancel stayed open.
/// The loader returns `true` for it, which tells Chrome the answer comes later.
#[wasm_bindgen]
pub fn on_determining_filename(item: JsValue, suggest: JsValue) {
    let Ok(suggest) = suggest.dyn_into::<Function>() else {
        return;
    };
    let Some(id) = get(&item, "id").as_f64().map(|n| n as i64) else {
        return name_as_the_browser_would(&suggest);
    };
    if let Some(now) = HOLDS.with(|h| h.borrow_mut().ask(id, suggest)) {
        return name_as_the_browser_would(&now);
    }
    // Whatever happens to the decision, the browser is not held for ever.
    wasm_bindgen_futures::spawn_local(async move {
        sleep(decide::DETERMINE_WITHIN_MS).await;
        if let Some(late) = HOLDS.with(|h| h.borrow_mut().expire(id)) {
            log(
                "warn",
                "a download's name was held too long; the browser names it",
            );
            name_as_the_browser_would(&late);
        }
    });
}

/// Answers Chrome's question with no suggestion: the name it chose stands.
pub(super) fn name_as_the_browser_would(suggest: &Function) {
    if let Err(why) = suggest.call0(&JsValue::UNDEFINED) {
        log(
            "debug",
            &format!("the browser's name question: {}", say(&why)),
        );
    }
}

/// Settles Chrome's file name question for a download once its `onCreated`
/// has run its course, on every way out of it.
pub(super) struct Decided {
    id: Option<i64>,
    /// Taken by dm and cancelled or removed in the browser.
    taken: bool,
}

impl Drop for Decided {
    fn drop(&mut self) {
        let Some(id) = self.id else { return };
        if self.taken {
            HOLDS.with(|h| h.borrow_mut().drop_taken(id));
        } else if let Some(suggest) = HOLDS.with(|h| h.borrow_mut().decide(id)) {
            name_as_the_browser_would(&suggest);
        }
    }
}

/// The state the browser reports for a download now: `in_progress`,
/// `interrupted` or `complete`; `None` once it is gone from the list.
pub(super) async fn state_of(id: i64) -> Option<String> {
    let query = to_js(&serde_json::json!({ "id": id }));
    let found = call("downloads", "search", &[query])
        .await
        .unwrap_or_default();
    get(&found, "0.state").as_string()
}

/// Lets the browser carry on with a download it was holding. Where it cannot
/// be resumed — Firefox cannot resume what the server will not range — it is
/// started again, marked so it is not caught a second time.
pub(super) async fn give_back(id: &JsValue, url: &str) {
    if call("downloads", "resume", std::slice::from_ref(id))
        .await
        .is_ok()
    {
        return;
    }
    // Finished while it was held (Chromium completes a fast PDF despite the
    // pause): the browser has the file, and starting it again would make two.
    if let Some(n) = id.as_f64() {
        if state_of(n as i64).await.as_deref() == Some("complete") {
            return;
        }
    }
    RELEASED.with(|r| r.borrow_mut().remember(url, now()));
    let options = to_js(&serde_json::json!({ "url": url }));
    if let Err(why) = call("downloads", "download", &[options]).await {
        log(
            "error",
            &format!("{url} could not be given back to the browser: {why}"),
        );
        notify(
            "A download could not be kept",
            &format!("Neither dm nor the browser could carry on with {url}: {why}"),
        );
    }
}
