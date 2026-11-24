// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the page script and the panel ask: hand-offs, quality lists and links.

use super::*;

mod clicks;

use clicks::*;

/// A file the person asked dm for that dm could not take, downloaded by the
/// browser instead: the
/// person asked for the file, and gets it either way. A page (a player for
/// the video reader) has nothing the browser could save, so its reason stands.
pub(super) async fn in_browser_instead(target: &Target, why: &str) -> Result<String, String> {
    // Only what the browser downloads: a magnet given to its downloader was
    // accepted and failed, said as taken, and nothing was opened.
    // The caller gives a magnet back to the page.
    let web = ["http://", "https://", "ftp://"]
        .iter()
        .any(|s| target.url.to_ascii_lowercase().starts_with(s));
    if target.page || !web {
        return Err(why.to_string());
    }
    RELEASED.with(|r| r.borrow_mut().remember(&target.url, now()));
    let options = to_js(&serde_json::json!({ "url": target.url }));
    match call("downloads", "download", &[options]).await {
        Ok(_) => {
            log(
                "info",
                &format!("{} left to the browser: {why}", target.url),
            );
            Ok(format!("the browser is downloading it (dm: {why})"))
        }
        Err(browser) => Err(format!(
            "{why}; and the browser could not either: {browser}"
        )),
    }
}

/// Hands something a person asked for. `Ok` carries a word for whoever
/// asked; `Err` why it could not be done.
pub(super) async fn hand_target(
    target: Target,
    store: Option<String>,
    tab: Option<i64>,
) -> Result<String, String> {
    hand_chosen(
        target,
        store,
        tab,
        None,
        None,
        None,
        false,
        (None, None),
        None,
    )
    .await
}

/// Hands one download over, with the quality picked from the panel's list
/// and, as for a video from the panel, asking the app to show
/// "Download details" before it starts.
#[allow(
    clippy::too_many_arguments,
    reason = "each is one field of the panel's click, passed straight into Start"
)]
pub(super) async fn hand_chosen(
    target: Target,
    store: Option<String>,
    tab: Option<i64>,
    quality: Option<String>,
    label: Option<String>,
    sound: Option<String>,
    ask: bool,
    part: (Option<String>, Option<String>),
    title: Option<String>,
) -> Result<String, String> {
    let (app, port) = app().await;
    if let App::Absent { reason } = app {
        let why = absent_text(&reason, &own_id(), is_firefox());
        return in_browser_instead(&target, &why).await;
    }
    // A stream chosen from the panel (`ask`) is a video the page is playing:
    // its session goes with it, for this download alone, as a video page's
    // does. TikTok's and others' CDN refuse a signed address without it.
    // All of it on the page's own site; on another (TikTok's video servers
    // are on a domain of their own), only the cookies the browser itself sent
    // that host from this tab: the page names the file, and could name
    // another site's whose sign-in the browser keeps from it (gaps row 159 c).
    let video =
        (target.page || ask) && decide::session_goes(&target.url, &target.referrer, target.page);
    let (mut cookies, mut cookies_once) = sign_in_for(&target.url, store.as_deref(), video).await;
    if ask && !video && cookies.is_empty() {
        cookies = sent_sign_in(tab, &target.url, store.as_deref()).await;
        cookies_once = true;
    }
    // A signed address runs out: the newest one the player has asked for goes, not the first seen.
    let address = if target.page {
        target.url.clone()
    } else {
        renewed(&target.url, tab).await
    };
    let start = Start {
        id: hand_id("m", 0, now()),
        url: address.clone(),
        referrer: target.referrer.clone(),
        cookies,
        cookies_once,
        // A file or stream the page's own request fetched, with the headers
        // that request carried; a page has none that matter.
        headers: if target.page {
            Vec::new()
        } else {
            sent_to(&address).await
        },
        // Not checked first, as a caught download is: this one was asked of
        // dm, by name, so dm takes it and says if it fails, rather than
        // handing it back to start in the browser when dm's own request is
        // refused (a site that answers dm differently from the browser).
        caught: false,
        // A page is handed over for its media, picked or not: with a
        // quality the app never saves the page itself in the video's place
        // when the app cannot read it.
        quality: quality.or_else(|| target.page.then(|| "best".to_string())),
        label,
        sound: sound.filter(|s| s.starts_with("https://") || s.starts_with("http://")),
        ask,
        // The panel's "Part" line; the app checks what was typed.
        from: part.0.map(|s| decide::part_text(&s)).unwrap_or_default(),
        to: part.1.map(|s| decide::part_text(&s)).unwrap_or_default(),
        title: title
            .map(|t| decide::clean_title(&t, &target.referrer))
            .unwrap_or_default(),
        ..Start::default()
    };
    if let Err(why) = hand(port, start, tab).await {
        return in_browser_instead(&target, &why).await;
    }
    log("info", &format!("handed {} to the dm app", target.url));
    // A page goes to the app to be read for its video, which it may not be able to do (it
    // reads YouTube's pages; gaps row 155): said as that, never as the app having the video.
    // In English here: the panel says each in the browser's language (`words::answered`).
    Ok(if target.page {
        crate::words::PANEL_APP_READING.en.into()
    } else if ask {
        crate::words::PANEL_APP_HAS.en.into()
    } else {
        crate::words::PANEL_APP_DOWNLOADING.en.into()
    })
}

/// The qualities a player page's video comes in, from the app. A
/// file is one quality, so it has no list.
pub(super) async fn formats_of(
    target: &Target,
    store: Option<&str>,
) -> Result<Vec<dm_protocol::Choice>, String> {
    if !target.page {
        return Ok(Vec::new());
    }
    let (app, port) = app().await;
    if let App::Absent { reason } = app {
        return Err(absent_text(&reason, &own_id(), is_firefox()));
    }
    let mut port = match port {
        Some(port) => port,
        None => Port::open()?,
    };
    // The page's session, for this one reading: a video site asks the app
    // to "confirm you're not a bot" without it.
    let (cookies, _) = sign_in_for(&target.url, store, true).await;
    port.send(&Request::Formats(dm_protocol::FormatsAsked {
        url: target.url.clone(),
        referrer: target.referrer.clone(),
        cookies,
    }))?;
    // The app reads a page in a few seconds and gives it thirty.
    let deadline = now() + 40_000.0;
    loop {
        match port.next(deadline).await {
            Heard::Frame(frame) => {
                if let Ok(dm_protocol::Reply::Formats { choices, reason }) =
                    dm_protocol::Reply::from_frame(&frame)
                {
                    return match reason {
                        Some(why) if choices.is_empty() => Err(why),
                        _ => Ok(choices),
                    };
                }
            }
            Heard::Gone(why) => return Err(why),
        }
    }
}

/// The panel's list of what a tab's player loaded: made by the app from what this extension
/// kept, where the app says it makes it (gaps row A22 d), so a fix to how the list is joined,
/// ordered or named reaches this extension with the app; otherwise, or where the app does not
/// answer in time, made here.
pub(super) async fn panel_streams(tab: i64, page: &str) -> Vec<decide::Stream> {
    let raw = raw_streams_of(tab).await;
    if raw.is_empty() {
        return Vec::new();
    }
    let (app, port) = app().await;
    let lists = matches!(&app, App::Present { hello, .. } if hello.has(dm_protocol::feature::LISTS_STREAMS));
    if lists {
        if let Some(listed) = listed_by_app(port, page, raw.clone()).await {
            return listed;
        }
    }
    decide::panel_list(raw)
}

/// Asks the app for the panel's list of `streams`, or `None` where it did not answer in time.
async fn listed_by_app(
    port: Option<Port>,
    page: &str,
    streams: Vec<decide::Stream>,
) -> Option<Vec<decide::Stream>> {
    let mut port = match port {
        Some(port) => port,
        None => Port::open().ok()?,
    };
    port.send(&Request::Streams(dm_protocol::StreamsAsked {
        page: page.to_string(),
        streams,
    }))
    .ok()?;
    let deadline = now() + decide::STREAMS_LISTED_WITHIN_MS;
    loop {
        match port.next(deadline).await {
            Heard::Frame(frame) => {
                if let Some(listed) = decide::streams_listed(&frame) {
                    return Some(listed);
                }
            }
            Heard::Gone(why) => {
                log(
                    "debug",
                    &format!("the app did not list a tab's streams: {why}"),
                );
                return None;
            }
        }
    }
}

/// Hands something a person asked for, and says so when it cannot.
pub(super) async fn hand_asked(target: Target, store: Option<String>, tab: Option<i64>) {
    let name = if target.url.to_ascii_lowercase().starts_with("magnet:") {
        decide::magnet_name(&target.url)
    } else {
        decide::taken_name("", &target.url)
    };
    match hand_target(target, store, tab).await {
        // Left to the browser, which is downloading it: said so, not as
        // dm's.
        Ok(said) if said.starts_with("the browser is downloading it") => notify("dm", &said),
        // Said, as a caught download is: nothing in the browser showed it
        // had been taken.
        Ok(_) => toast_sent(name),
        Err(why) => notify("dm could not take that", &why),
    }
}

/// A click on the floating panel the page script draws over a player.
/// Answered, so the panel can say what happened; only this extension's own
/// page script, in a tab, is listened to.
#[wasm_bindgen]
pub async fn on_page_message(message: JsValue, sender: JsValue) -> JsValue {
    if get(&sender, "id").as_string().unwrap_or_default() != own_id() {
        return answer(false, "that message did not come from dm");
    }
    if get(&message, "type").as_string().as_deref() == Some(decide::PANEL_LOAD_MESSAGE) {
        return load_panel(&sender).await;
    }
    if get(&message, "type").as_string().as_deref() == Some(decide::MEDIA_SEEN_MESSAGE) {
        return media_seen(&message, &sender).await;
    }
    if get(&message, "type").as_string().as_deref() == Some(decide::PAGE_RULES_MESSAGE) {
        return page_rules(&sender).await;
    }
    if get(&message, "type").as_string().as_deref() == Some(decide::PAGE_ANSWER_MESSAGE) {
        return page_answer(&message, &sender).await;
    }
    if let Some(said) = popup_asked(&message).await {
        return said;
    }
    let tab_url = get(&sender, "tab.url").as_string().unwrap_or_default();
    let private = get(&sender, "tab.incognito").as_bool() == Some(true);
    let click: decide::PanelClick = serde_json::from_value(from_js(&message)).unwrap_or_default();
    if click.kind == decide::MAGNET_MESSAGE {
        return magnet_click(&click, &sender, &tab_url, private).await;
    }
    if click.kind == decide::INLINE_MESSAGE {
        return inline_click(&click, &sender, &tab_url, private).await;
    }
    if click.kind == decide::TAKE_MESSAGE {
        return take_click(&click, &sender, &tab_url, private).await;
    }
    // Asked again only once the last answer is a minute old; a fresh one
    // is kept where every page's script watches it.
    if click.kind == decide::RULES_MESSAGE {
        let _ = app().await;
        return answer(true, "the rules are current");
    }
    if click.kind == decide::SKIP_MESSAGE {
        return skip_click(&click, &sender, private).await;
    }
    let target = match decide::panel_target(&click, &tab_url, private) {
        Ok(target) => target,
        Err(why) => return answer(false, &why),
    };
    if click.kind == decide::FORMATS_MESSAGE {
        return formats_click(&target, &sender, &tab_url).await;
    }
    // A click on the bar or on a line of its list: handed over to be asked
    // about in the app, which opens "Download details".
    match hand_chosen(
        target,
        tab_store(&sender, "tab"),
        tab_of(&sender, "tab"),
        click.quality,
        click.label,
        click.sound,
        true,
        (click.from, click.to),
        click.title,
    )
    .await
    {
        Ok(said) => answer(true, &said),
        Err(why) => answer(false, &why),
    }
}

/// Asks the page (the frame that was right-clicked) for its links, all or
/// the selected ones, and hands them to the app together, for its "Download
/// all links" dialog. Returns how many.
pub(super) async fn page_links(
    tab: &JsValue,
    info: &JsValue,
    selected: bool,
    page: &str,
) -> Result<usize, String> {
    let tab_id = get(tab, "id");
    if tab_id.as_f64().is_none() {
        return Err("that is not a page dm can read links from".into());
    }
    let frame_id = get(info, "frameId").as_f64().unwrap_or(0.0);
    let asked = to_js(&serde_json::json!({ "type": decide::LINKS_MESSAGE, "selected": selected }));
    let frame = to_js(&serde_json::json!({ "frameId": frame_id }));
    let args = [tab_id.clone(), asked, frame];
    let ask = || call("tabs", "sendMessage", &args);
    let answer = match ask().await {
        Ok(answer) => answer,
        // No page script answered: a tab open before dm was installed or
        // updated (Chromium puts content scripts only into pages loaded
        // since). It is given one now, where the browser allows, and asked
        // again.
        Err(first) => {
            if !decide::injectable(page) {
                return Err(decide::NO_LINKS_HERE.into());
            }
            let target = to_js(&serde_json::json!({
                "target": { "tabId": tab_id.as_f64(), "frameIds": [frame_id] },
                "files": ["content.js"],
                // Now, not once the page has finished arriving.
                "injectImmediately": true,
            }));
            if let Err(why) = call("scripting", "executeScript", &[target]).await {
                log("warn", &format!("the links reader could not be put into {page}: {why} (asked first: {first})"));
                return Err(decide::NO_LINKS_HERE.into());
            }
            // Asked a few times: a reader just put in can take a moment to
            // be listening on a page still arriving (0 of 3 links on one machine, not seen elsewhere).
            let mut answer = ask().await;
            for _ in 0..3 {
                if answer.is_ok() {
                    break;
                }
                sleep(150.0).await;
                answer = ask().await;
            }
            answer.map_err(|why| {
                log(
                    "warn",
                    &format!("{page} did not answer for its links: {why} (asked first: {first})"),
                );
                decide::NO_LINKS_HERE.to_string()
            })?
        }
    };
    let hrefs: Vec<String> =
        serde_json::from_value(from_js(&get(&answer, "links"))).unwrap_or_default();
    let urls = decide::links_from(&hrefs, page);
    let words: std::collections::BTreeMap<String, String> =
        serde_json::from_value(from_js(&get(&answer, "texts"))).unwrap_or_default();
    let texts = decide::link_texts(&words, &urls);
    let given: std::collections::BTreeMap<String, String> =
        serde_json::from_value(from_js(&get(&answer, "names"))).unwrap_or_default();
    let names = decide::link_names(&given, &urls);
    if urls.is_empty() {
        return Err(if selected {
            "the selection has no links".into()
        } else {
            "this page has no links".into()
        });
    }
    let (app, port) = app().await;
    if let App::Absent { reason } = app {
        return Err(absent_text(&reason, &own_id(), is_firefox()));
    }
    let mut port = match port {
        Some(port) => port,
        None => Port::open()?,
    };
    let id = hand_id("l", 0, now());
    let count = urls.len();
    // The site's sign-in where the popup hands it over, as for one link:
    // "Download selected links" fetched a signed-in file without it and got
    // a 403.
    let cookies = cookies_for(page, tab_store(tab, "").as_deref()).await;
    port.send(&Request::Batch(dm_protocol::Batch {
        id: id.clone(),
        page: page.to_string(),
        urls,
        cookies,
        texts,
        names,
    }))?;
    let deadline = now() + decide::TAKEN_WITHIN_MS;
    loop {
        match port.next(deadline).await {
            Heard::Frame(frame) => {
                if let Some(outcome) = decide::taken(&frame, &id) {
                    return outcome.map(|_| count);
                }
            }
            Heard::Gone(why) => return Err(why),
        }
    }
}

/// What the app is fetching and has fetched, as the toolbar popup lists it (gaps rows U18 and
/// C12): asked of an app that says it lists them, nothing of one that does not, or that did not
/// answer in time. Its buttons only where the app says it acts on downloads.
async fn popup_downloads() -> decide::PopupList {
    let (app, port) = app().await;
    let (lists, acts) = match &app {
        App::Present { hello, .. } => (
            hello.has(dm_protocol::feature::LISTS_DOWNLOADS),
            hello.has(dm_protocol::feature::ACTS_ON_DOWNLOADS),
        ),
        App::Absent { .. } => (false, false),
    };
    if !lists {
        return decide::PopupList::default();
    }
    let Some(mut port) = port.or_else(|| Port::open().ok()) else {
        return decide::PopupList::default();
    };
    if port.send(&Request::Downloads).is_err() {
        return decide::PopupList::default();
    }
    let deadline = now() + decide::DOWNLOADS_LISTED_WITHIN_MS;
    loop {
        match port.next(deadline).await {
            Heard::Frame(frame) => {
                if let Some(listed) = decide::downloads_listed(&frame) {
                    return decide::popup_list(&listed, acts);
                }
            }
            Heard::Gone(_) => return decide::PopupList::default(),
        }
    }
}

/// Asks the app to do what the popup's button says to a download (gaps row C12): `Err` with why
/// where it was not done, or the app does not act on downloads or did not answer in time.
async fn popup_act(act: dm_protocol::Act) -> Result<(), String> {
    let (app, port) = app().await;
    let acts = matches!(&app, App::Present { hello, .. } if hello.has(dm_protocol::feature::ACTS_ON_DOWNLOADS));
    if !acts {
        return Err(crate::words::POPUP_NO_ANSWER.say());
    }
    let mut port = match port {
        Some(port) => port,
        None => Port::open()?,
    };
    port.send(&Request::Act(act))?;
    let deadline = now() + decide::DOWNLOADS_LISTED_WITHIN_MS;
    loop {
        match port.next(deadline).await {
            Heard::Frame(frame) => {
                if let Some(done) = decide::acted(&frame) {
                    return done;
                }
            }
            Heard::Gone(why) => return Err(why),
        }
    }
}
