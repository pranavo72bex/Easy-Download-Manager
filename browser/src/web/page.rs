//! What the page script and the panel ask: hand-offs, quality lists and links.

use super::*;

/// A file the person asked dm for that dm could not take, downloaded by the
/// browser instead: the
/// person asked for the file, and gets it either way. A page (a player for
/// yt-dlp) has nothing the browser could save, so its reason stands.
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
pub(super) async fn hand_target(target: Target, store: Option<String>) -> Result<String, String> {
    hand_chosen(target, store, None, None, None, false).await
}

/// Hands one download over, with the quality picked from the panel's list
/// and, as for a video from the panel, asking the app to show
/// "Download details" before it starts.
pub(super) async fn hand_chosen(
    target: Target,
    store: Option<String>,
    quality: Option<String>,
    label: Option<String>,
    sound: Option<String>,
    ask: bool,
) -> Result<String, String> {
    let (app, port) = app().await;
    let hello = match app {
        App::Absent { reason } => {
            let why = absent_text(&reason, &own_id(), is_firefox());
            return in_browser_instead(&target, &why).await;
        }
        App::Present { hello, .. } => hello,
    };
    let (cookies, cookies_once) = sign_in_for(&target.url, store.as_deref(), target.page).await;
    let start = Start {
        id: hand_id("m", 0, now()),
        url: target.url.clone(),
        referrer: target.referrer.clone(),
        cookies,
        cookies_once,
        // Not checked first, as a caught download is: this one was asked of
        // dm, by name, so dm takes it and says if it fails, rather than
        // handing it back to start in the browser when dm's own request is
        // refused (a site that answers dm differently from the browser).
        caught: false,
        // A page is handed over for its media, picked or not: with a
        // quality the app never saves the page itself in the video's place
        // when yt-dlp cannot read it.
        quality: quality.or_else(|| target.page.then(|| "best".to_string())),
        label,
        sound: sound.filter(|s| s.starts_with("https://") || s.starts_with("http://")),
        ask,
        ..Start::default()
    };
    if let Err(why) = hand(port, start).await {
        return in_browser_instead(&target, &why).await;
    }
    log("info", &format!("handed {} to the dm app", target.url));
    if target.page && hello.ytdlp == Some(false) {
        return Ok(decide::NO_YTDLP.to_string());
    }
    Ok(if ask {
        "the dm app has it".into()
    } else if target.page {
        "the dm app is fetching it with yt-dlp".into()
    } else {
        "the dm app is downloading it".into()
    })
}

/// The qualities a player page's video comes in, from the app's yt-dlp. A
/// file is one quality, so it has no list.
pub(super) async fn formats_of(
    target: &Target,
    store: Option<&str>,
) -> Result<Vec<dm_protocol::Choice>, String> {
    if !target.page {
        return Ok(Vec::new());
    }
    let (app, port) = app().await;
    let hello = match app {
        App::Absent { reason } => return Err(absent_text(&reason, &own_id(), is_firefox())),
        App::Present { hello, .. } => hello,
    };
    if hello.ytdlp == Some(false) {
        return Err(decide::NO_YTDLP.to_string());
    }
    let mut port = match port {
        Some(port) => port,
        None => Port::open()?,
    };
    // The page's session, for this one reading: a video site asks yt-dlp
    // to "confirm you're not a bot" without it.
    let (cookies, _) = sign_in_for(&target.url, store, true).await;
    port.send(&Request::Formats(dm_protocol::FormatsAsked {
        url: target.url.clone(),
        referrer: target.referrer.clone(),
        cookies,
    }))?;
    // yt-dlp reads a page in a few seconds; the app gives it thirty.
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

/// Hands something a person asked for, and says so when it cannot.
pub(super) async fn hand_asked(target: Target, store: Option<String>) {
    let name = if target.url.to_ascii_lowercase().starts_with("magnet:") {
        decide::magnet_name(&target.url)
    } else {
        decide::taken_name("", &target.url)
    };
    match hand_target(target, store).await {
        Ok(said) if said == decide::NO_YTDLP => notify("dm", &said),
        // Left to the browser, which is downloading it: said so, not as
        // dm's.
        Ok(said) if said.starts_with("the browser is downloading it") => notify("dm", &said),
        // Said, as a caught download is: nothing in the browser showed it
        // had been taken.
        Ok(_) => notify("dm is downloading it", &name),
        Err(why) => notify("dm could not take that", &why),
    }
}

/// A click on the floating panel the page script draws over a player.
/// Answered, so the panel can say what happened; only this extension's own
/// page script, in a tab, is listened to.
#[wasm_bindgen]
pub async fn on_page_message(message: JsValue, sender: JsValue) -> JsValue {
    let answer = |ok: bool, said: &str| to_js(&serde_json::json!({ "ok": ok, "said": said }));
    if get(&sender, "id").as_string().unwrap_or_default() != own_id() {
        return answer(false, "that message did not come from dm");
    }
    // What the page's player asked for, as the page script saw it: read as
    // a response of the tab it came from, which is all the list needs.
    if get(&message, "type").as_string().as_deref() == Some(decide::MEDIA_SEEN_MESSAGE) {
        let tab = get(&sender, "tab.id").as_f64().unwrap_or(-1.0);
        let url = get(&message, "url").as_string().unwrap_or_default();
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
        return answer(true, "noted");
    }
    // The toolbar popup: how the app is, and bringing it up.
    match get(&message, "type").as_string().as_deref() {
        Some(decide::STATUS_MESSAGE) => {
            let (app, _) = app().await;
            return to_js(&match app {
                App::Present { hello, .. } => {
                    let (line, bad) = decide::status_line(&hello);
                    serde_json::json!({
                        "ok": true,
                        "version": hello.engine_version,
                        "ytdlp": hello.ytdlp,
                        "line": line,
                        "bad": bad,
                    })
                }
                // With no host, the popup's Open dm has nothing to ask.
                App::Absent { reason } => serde_json::json!({
                    "ok": false,
                    "said": decide::absent_popup_text(&reason, &own_id(), is_firefox()),
                    "host": !decide::not_connected(&reason),
                }),
            });
        }
        Some(decide::OPEN_MESSAGE) => {
            return match show_app().await {
                Ok(()) => answer(true, "dm is open"),
                Err(why) => answer(false, &why),
            };
        }
        _ => {}
    }
    let tab_url = get(&sender, "tab.url").as_string().unwrap_or_default();
    let private = get(&sender, "tab.incognito").as_bool() == Some(true);
    let click: decide::PanelClick = serde_json::from_value(from_js(&message)).unwrap_or_default();
    // A magnet link clicked on a page: the app's, as a clicked .torrent is;
    // and where the app cannot take it, the browser does what it would
    // have done, opening it with whatever the system has for magnets.
    if click.kind == decide::MAGNET_MESSAGE {
        let url = match decide::magnet_url(&click) {
            Ok(url) => url,
            Err(why) => return answer(false, &why),
        };
        let target = Target {
            url: url.clone(),
            referrer: tab_url.clone(),
            page: false,
        };
        // The popup's "Catch downloads" switch off: the browser's, as a
        // clicked file would be.
        let refused = if private {
            Some("dm takes nothing from a private window".to_string())
        } else if !catching().await {
            Some("catching is switched off".to_string())
        } else {
            match hand_target(target, tab_store(&sender, "tab")).await {
                Ok(_) => {
                    notify("dm is downloading it", &decide::magnet_name(&url));
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
        return answer(false, &why);
    }
    // A plain click on a link to a PDF, a video or a song: the browser shows
    // it in the tab and starts no download, so the app takes it here, by the
    // same rules a download is caught by. Where it does not (no app, a site
    // to leave alone, a type it does not list), the answer is no and the page
    // opens the link as it would have.
    if click.kind == decide::INLINE_MESSAGE {
        let url = match decide::inline_url(&click) {
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
        if decide::paused(&paused, &url) || decide::paused(&paused, &tab_url) {
            return answer(false, "capturing is paused for this site");
        }
        match app().await.0 {
            App::Absent { reason } => return answer(false, &reason),
            App::Present { rules, .. } => {
                let candidate = dm_protocol::Candidate {
                    url: &url,
                    asked_url: &url,
                    referrer: &tab_url,
                    ..dm_protocol::Candidate::default()
                };
                if let Err(left) = rules.takes(&candidate, false) {
                    return answer(false, left.reason());
                }
            }
        }
        let target = Target {
            url: url.clone(),
            referrer: tab_url.clone(),
            page: false,
        };
        return match hand_target(target, tab_store(&sender, "tab")).await {
            Ok(_) => {
                notify("dm is downloading it", &decide::taken_name("", &url));
                answer(true, "the app takes it")
            }
            Err(why) => answer(false, &why),
        };
    }
    // A click with the take key held: the download about to start is the
    // app's, whatever the types and size floor say.
    // Handed over as "Download link with dm" is: a link that is not itself
    // a download (Shift opened it in a new window) reaches the
    // app all the same. The page script kept the browser from acting on it.
    if click.kind == decide::TAKE_MESSAGE {
        return match decide::take_url(&click) {
            Ok(url) => {
                TAKEN.with(|t| t.borrow_mut().remember(&url, now()));
                let menu = decide::MenuClick {
                    menu_item_id: serde_json::Value::String(decide::MENU_LINK.to_string()),
                    link_url: Some(url),
                    page_url: Some(tab_url.clone()),
                    ..decide::MenuClick::default()
                };
                match decide::menu_target(&menu, private) {
                    Ok(target) => {
                        hand_asked(target, tab_store(&sender, "tab")).await;
                        answer(true, "the app takes it")
                    }
                    Err(why) => answer(false, &why),
                }
            }
            Err(why) => answer(false, &why),
        };
    }
    // Asked again only once the last answer is a minute old; a fresh one
    // is kept where every page's script watches it.
    if click.kind == decide::RULES_MESSAGE {
        let _ = app().await;
        return answer(true, "the rules are current");
    }
    // Alt-click: the download about to start is the browser's.
    if click.kind == decide::SKIP_MESSAGE {
        return match decide::skip_url(&click) {
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
        };
    }
    let target = match decide::panel_target(&click, &tab_url, private) {
        Ok(target) => target,
        Err(why) => return answer(false, &why),
    };
    // What the video comes in, for the panel's list: the qualities the
    // app's yt-dlp reads from the page, then the streams the page loaded.
    // A site yt-dlp does not know still offers what its player fetched.
    if click.kind == decide::FORMATS_MESSAGE {
        let tab = get(&sender, "tab.id").as_f64().map_or(-1, |id| id as i64);
        let streams = streams_of(tab).await;
        // The video helper can take half a minute over a page. Where the page
        // has already shown streams of its own, they are not held back for
        // it past a few seconds.
        let store = tab_store(&sender, "tab");
        let ask = Box::pin(formats_of(&target, store.as_deref()));
        let read = if streams.is_empty() {
            ask.await
        } else {
            let patience = Box::pin(sleep(8_000.0));
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
        return match read {
            Err(why) if lines.is_empty() => answer(false, &why),
            _ => to_js(&serde_json::json!({ "ok": true, "choices": lines })),
        };
    }
    // A click on the bar or on a line of its list: handed over to be asked
    // about in the app, which opens "Download details".
    match hand_chosen(
        target,
        tab_store(&sender, "tab"),
        click.quality,
        click.label,
        click.sound,
        true,
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
    }))?;
    let deadline = now() + decide::TAKEN_WITHIN_MS;
    loop {
        match port.next(deadline).await {
            Heard::Frame(frame) => {
                if let Some(outcome) = decide::taken(&frame, &id) {
                    return outcome.map(|()| count);
                }
            }
            Heard::Gone(why) => return Err(why),
        }
    }
}
