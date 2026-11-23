// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Each kind of message `on_page_message` answers: the panel's code, what a page's player
//! asked for, the app's rules for a site, the toolbar popup, and the clicks on a page.

use super::*;

/// The answer to a page's message: whether it was done, and a word on it.
pub(super) fn answer(ok: bool, said: &str) -> JsValue {
    to_js(&serde_json::json!({ "ok": ok, "said": said }))
}

/// The panel's code, put into the asking frame by the browser itself: only into a frame of a
/// tab, the one that asked.
pub(super) async fn load_panel(sender: &JsValue) -> JsValue {
    let (Some(tab), Some(frame)) = (
        get(sender, "tab.id").as_f64(),
        get(sender, "frameId").as_f64(),
    ) else {
        return answer(false, "the panel goes only into a page's frame");
    };
    let target = to_js(&serde_json::json!({
        "target": { "tabId": tab as i64, "frameIds": [frame as i64] },
        "files": [decide::PANEL_FILE],
    }));
    match call("scripting", "executeScript", &[target]).await {
        Ok(_) => to_js(&serde_json::json!({ "ok": true })),
        Err(why) => answer(false, &why),
    }
}

/// What the page's player asked for, as the page script saw it: read as
/// a response of the tab it came from, which is all the list needs.
pub(super) async fn media_seen(message: &JsValue, sender: &JsValue) -> JsValue {
    let tab = get(sender, "tab.id").as_f64().unwrap_or(-1.0);
    let url = get(message, "url").as_string().unwrap_or_default();
    if tab >= 0.0 && decide::track_of(&url).is_some() {
        let details = to_js(&serde_json::json!({
            "url": url,
            "tabId": tab,
            "type": "xmlhttprequest",
            "statusCode": 200,
            "responseHeaders": [],
        }));
        on_response_started(details).await;
    }
    answer(true, "noted")
}

/// The page script asking which requests of its site have answers to read
/// (the app's rules for the site; none for most sites).
pub(super) async fn page_rules(sender: &JsValue) -> JsValue {
    ensure_served().await;
    let page = get(sender, "url").as_string().unwrap_or_default();
    to_js(&serde_json::json!({
        "ok": true,
        "urls": decide::rules::watched_for(&page),
        "finders": decide::rules::finders_for(&page),
        "privateFetches": decide::rules::private_fetches_for(&page),
        "title": decide::rules::title_for(&page),
    }))
}

/// The answer to one of those requests: read for the streams a page builds
/// itself, only where the app's rules for the page's site name the request,
/// and only as JSON, so a page that sends anything else gets nothing taken.
pub(super) async fn page_answer(message: &JsValue, sender: &JsValue) -> JsValue {
    ensure_served().await;
    let tab = get(sender, "tab.id").as_f64().unwrap_or(-1.0);
    let page = get(sender, "url").as_string().unwrap_or_default();
    let asked = get(message, "url").as_string().unwrap_or_default();
    let body = get(message, "body").as_string().unwrap_or_default();
    if tab >= 0.0 {
        if let Some(rule) = decide::rules::answer_rule_for(&page, &asked) {
            for address in decide::addresses_by(&body, &rule, &page) {
                let details = to_js(&serde_json::json!({
                    "url": address,
                    "tabId": tab,
                    "type": "xmlhttprequest",
                    "statusCode": 200,
                    "responseHeaders": [],
                }));
                on_response_started(details).await;
            }
        }
    }
    answer(true, "noted")
}

/// The toolbar popup: how the app is, and bringing it up. `None` for a message that is not
/// the popup's.
pub(super) async fn popup_asked(message: &JsValue) -> Option<JsValue> {
    match get(message, "type").as_string().as_deref() {
        Some(decide::STATUS_MESSAGE) => {
            let (app, _) = app().await;
            Some(to_js(&match app {
                App::Present { hello, .. } => {
                    let (line, bad) = decide::status_line(&hello);
                    serde_json::json!({
                        "ok": true,
                        "version": hello.engine_version,
                        "line": line,
                        "bad": bad,
                    })
                }
                // With no host, the popup's Open dm has nothing to ask.
                App::Absent { reason } => serde_json::json!({
                    "ok": false,
                    "said": decide::absent_popup_text(&reason, &own_id(), is_firefox()),
                    "advanced": decide::popup_register(&own_id(), is_firefox()),
                    "host": !decide::not_connected(&reason),
                }),
            }))
        }
        Some(decide::DOWNLOADS_MESSAGE) => Some(to_js(
            &serde_json::to_value(popup_downloads().await).unwrap_or_default(),
        )),
        // A button beside a download in the popup (gaps row C12).
        Some(decide::ACT_MESSAGE) => {
            let act = dm_protocol::Act {
                does: get(message, "does").as_string().unwrap_or_default(),
                id: get(message, "id").as_f64().map(|id| id as u64),
            };
            Some(match popup_act(act).await {
                Ok(()) => answer(true, "done"),
                Err(why) => answer(false, &why),
            })
        }
        Some(decide::OPEN_MESSAGE) => Some(match show_app().await {
            Ok(()) => answer(true, "dm is open"),
            Err(why) => answer(false, &why),
        }),
        _ => None,
    }
}

/// A magnet link clicked on a page: the app's, as a clicked .torrent is;
/// and where the app cannot take it, the browser does what it would
/// have done, opening it with whatever the system has for magnets.
pub(super) async fn magnet_click(
    click: &decide::PanelClick,
    sender: &JsValue,
    tab_url: &str,
    private: bool,
) -> JsValue {
    let url = match decide::magnet_url(click) {
        Ok(url) => url,
        Err(why) => return answer(false, &why),
    };
    let target = Target {
        url: url.clone(),
        referrer: tab_url.to_string(),
        page: false,
    };
    // The popup's "Catch downloads" switch off: the browser's, as a
    // clicked file would be.
    let refused = if private {
        Some("dm takes nothing from a private window".to_string())
    } else if !catching().await {
        Some("catching is switched off".to_string())
    } else {
        match hand_target(target, tab_store(sender, "tab"), tab_of(sender, "tab")).await {
            Ok(_) => {
                toast_sent(decide::magnet_name(&url));
                return answer(true, "the app takes it");
            }
            Err(why) => Some(why),
        }
    };
    let why = refused.unwrap_or_default();
    log(
        "info",
        &format!("a clicked magnet is left to the browser: {why}"),
    );
    // The page opens it on this answer, as it would have without dm,
    // so the browser's prompt names the site, not the extension.
    answer(false, &why)
}

/// A plain click on a link to a PDF, a video or a song: the browser shows
/// it in the tab and starts no download, so the app takes it here, by the
/// same rules a download is caught by. Where it does not (no app, a site
/// to leave alone, a type it does not list), the answer is no and the page
/// opens the link as it would have.
pub(super) async fn inline_click(
    click: &decide::PanelClick,
    sender: &JsValue,
    tab_url: &str,
    private: bool,
) -> JsValue {
    let url = match decide::inline_url(click) {
        Ok(url) => url,
        Err(why) => return answer(false, &why),
    };
    if private {
        return answer(false, "dm takes nothing from a private window");
    }
    if !catching().await {
        return answer(false, "catching is switched off");
    }
    let paused = paused_sites().await;
    if decide::paused(&paused, &url) || decide::paused(&paused, tab_url) {
        return answer(false, "capturing is paused for this site");
    }
    match app().await.0 {
        App::Absent { reason } => return answer(false, &reason),
        App::Present { rules, .. } => {
            let candidate = dm_protocol::Candidate {
                url: &url,
                asked_url: &url,
                referrer: tab_url,
                ..dm_protocol::Candidate::default()
            };
            if let Err(left) = rules.takes(&candidate, false) {
                return answer(false, left.reason());
            }
        }
    }
    let target = Target {
        url: url.clone(),
        referrer: tab_url.to_string(),
        page: false,
    };
    match hand_target(target, tab_store(sender, "tab"), tab_of(sender, "tab")).await {
        Ok(_) => {
            toast_sent(decide::taken_name("", &url));
            answer(true, "the app takes it")
        }
        Err(why) => answer(false, &why),
    }
}

/// A click with the take key held: the download about to start is the
/// app's, whatever the types and size floor say.
/// Handed over as "Download link with dm" is: a link that is not itself
/// a download (Shift opened it in a new window) reaches the
/// app all the same. The page script kept the browser from acting on it.
pub(super) async fn take_click(
    click: &decide::PanelClick,
    sender: &JsValue,
    tab_url: &str,
    private: bool,
) -> JsValue {
    match decide::take_url(click) {
        Ok(url) => {
            TAKEN.with(|t| t.borrow_mut().remember(&url, now()));
            let menu = decide::MenuClick {
                menu_item_id: serde_json::Value::String(decide::MENU_LINK.to_string()),
                link_url: Some(url),
                page_url: Some(tab_url.to_string()),
                ..decide::MenuClick::default()
            };
            match decide::menu_target(&menu, private) {
                Ok(target) => {
                    hand_asked(target, tab_store(sender, "tab"), tab_of(sender, "tab")).await;
                    answer(true, "the app takes it")
                }
                Err(why) => answer(false, &why),
            }
        }
        Err(why) => answer(false, &why),
    }
}

/// Alt-click: the download about to start is the browser's.
pub(super) async fn skip_click(click: &decide::PanelClick) -> JsValue {
    match decide::skip_url(click) {
        Ok(url) => {
            RELEASED.with(|r| r.borrow_mut().remember(&url, now()));
            // Firefox does not download on Alt-click as Chromium does;
            // asked to, it downloads it itself, and dm leaves it be
            // (it did nothing at all).
            if is_firefox() {
                let wanted = to_js(&serde_json::json!({ "url": url }));
                if let Err(why) = call("downloads", "download", &[wanted]).await {
                    return answer(false, &why);
                }
            }
            answer(true, "the browser keeps it")
        }
        Err(why) => answer(false, &why),
    }
}

/// What the video comes in, for the panel's list: the qualities the
/// app reads from the page, then the streams the page loaded.
/// A site the app cannot read still offers what its player fetched.
pub(super) async fn formats_click(target: &Target, sender: &JsValue, tab_url: &str) -> JsValue {
    let tab = get(sender, "tab.id").as_f64().map_or(-1, |id| id as i64);
    let streams = panel_streams(tab, tab_url).await;
    // The video helper can take half a minute over a page. Where the page
    // has already shown streams of its own that make a choice, they are
    // not held back for it past a few seconds; a single stream is no
    // choice, and the reading is waited for (gaps row 178).
    let store = tab_store(sender, "tab");
    let ask = Box::pin(formats_of(target, store.as_deref()));
    let read = if !decide::cut_reading_short(streams.len()) {
        ask.await
    } else {
        let patience = Box::pin(sleep(decide::STREAMS_PATIENCE_MS));
        match futures_util::future::select(ask, patience).await {
            futures_util::future::Either::Left((done, _)) => done,
            futures_util::future::Either::Right(_) => {
                Err("the video helper is still reading this page".to_string())
            }
        }
    };
    let mut lines: Vec<serde_json::Value> = match &read {
        Ok(choices) => choices
            .iter()
            .map(|c| {
                serde_json::json!({
                    "quality": c.quality,
                    "label": c.label,
                    "line": decide::choice_line(&c.label, c.size),
                })
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    lines.extend(streams.iter().map(|s| {
        serde_json::json!({
            "url": s.url,
            "sound": s.sound,
            "label": s.label,
            "line": decide::choice_line(&s.label, s.size),
        })
    }));
    match read {
        Err(why) if lines.is_empty() => answer(false, &why),
        _ => to_js(&serde_json::json!({ "ok": true, "choices": lines })),
    }
}
