// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The native host's answer to each frame, and what a hand-off is checked for before it is filed.

use super::*;

/// The native host's reply to one frame from the browser's extension, as
/// JSON: `None` for a frame it does not take (another version's, or one
/// for the window, such as a pause). What the reply needs of the app (its
/// settings, its inbox, a page's qualities) is asked of `world`.
pub fn answer(
    home: &Path,
    frame: &serde_json::Value,
    world: &mut dyn World,
) -> Option<serde_json::Value> {
    let reply = match Asked::from_frame(frame) {
        // What this app can do, said up front.
        Ok(Asked::Hello(greeting)) => replies::hello_reply(home, greeting),

        // Which downloads to catch: the app's choice, read on every ask so a
        // change in the window is in force by the next download.
        Ok(Asked::Settings) => Reply::Settings(SettingsReply {
            rules: catch_rules(home),
        }),

        Ok(Asked::Batch(batch)) => replies::links_reply(home, batch, world),

        // The toolbar button: the window to the front, or dm started, which
        // opens it. Nothing is downloaded.
        Ok(Asked::Show) => {
            let shown = if world.app_running() {
                let shown = std::fs::create_dir_all(inbox(home)).and_then(|()| {
                    std::fs::write(inbox(home).join(dm_core::paths::INBOX_SHOW), b"")
                });
                world.ring();
                shown
            } else {
                world.launch()
            };
            Reply::Shown {
                reason: shown
                    .err()
                    .map(|e| format!("dm could not be brought up: {e}")),
            }
        }

        Ok(Asked::Formats(FormatsAsked {
            url,
            referrer,
            cookies,
        })) => match world.formats(url.trim(), referrer.trim(), &cookies) {
            Ok(choices) => Reply::Formats {
                choices,
                reason: None,
            },
            Err(why) => Reply::Formats {
                choices: Vec::new(),
                reason: Some(why),
            },
        },

        Ok(Asked::Start(start)) => take(home, start, world),

        // Would the app take this download the browser started, by its rules as they are
        // now: an extension that asks judges nothing of type, size or site itself.
        Ok(Asked::Judge(judge)) => {
            let verdict = judge.judged_by(&catch_rules(home));
            Reply::Judged {
                id: judge.id,
                take: verdict.is_ok(),
                reason: verdict.err(),
            }
        }

        // An item of the menu the app served, clicked: the app chooses what it takes.
        Ok(Asked::MenuClicked(click)) => match menu_start(&click) {
            Ok(start) => take(home, start, world),
            Err(why) => Reply::failed(&click.id, why),
        },

        // The panel's list, made by this app's own reading of what the player loaded.
        Ok(Asked::Streams(asked)) => Reply::Streams {
            streams: dm_protocol::media::panel_list(asked.streams),
        },

        // What the window is fetching, as it last wrote it (`app::going`), for the toolbar popup.
        Ok(Asked::Downloads) => {
            let running = world.app_running();
            let published = crate::app::going::read(home, world.now_ms(), running);
            Reply::Downloads {
                downloads: published.downloads,
                running: published.running,
                failed: published.failed,
            }
        }

        // Another version, or something this host does not do: pausing and
        // cancelling belong to the window, which is where the download is.
        Err(_) => return None,
    };
    Some(reply.to_frame())
}

/// The answer to a hand-off ([`Asked::Start`]), or to a click on an item of the menu the app
/// served, made into one ([`menu_start`]).
fn take(home: &Path, start: Start, world: &mut dyn World) -> Reply {
    let Start {
        id,
        url,
        referrer,
        caught,
        quality,
        sound,
        label,
        ask,
        from,
        to,
        cookies,
        cookies_once,
        headers,
        title,
        mime,
        deadline,
        post,
        renews,
        private_window,
        ..
    } = start;
    let url = url.trim().to_string();
    if url.is_empty() {
        Reply::failed(&id, "no URL was given")
    } else if renews && !world.app_running() {
        // Nothing waits for it with no app running, and dm is not
        // started for a renewal the person did not ask for.
        Reply::failed(&id, "no dm is running to take a renewed address")
    } else if !dm_core::accepted(&url)
        // A file on this computer is dm's to read only when the
        // person opens it with dm, never at a web page's word.
        || url.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("file:"))
    {
        // Refused here by the queue's own rule, rather than accepted
        // and then dropped by the queue unseen.
        Reply::failed(
            &id,
            "dm downloads http(s) and ftp links, magnets and .torrent files only",
        )
    } else if let Some(why) = caught
        .then(|| unwritable(&saved(home).folders.download_dir))
        .flatten()
    {
        // A download the browser holds is left with it when dm could
        // not save it anyway: taken, it would sit paused in dm.
        Reply::failed(&id, why)
    } else if let Some(why) = checked(
        world,
        // Not asked about first where the person said the site's
        // links work once (Options › Browser): the asking spent the
        // link before dm's own download could use it.
        // Nor a form's answer: asking would send the form again.
        caught && post.is_none() && !saved(home).browser.catch.works_once(&url, &referrer),
        Asking {
            url: &url,
            referrer: &referrer,
            cookies: &cookies,
            headers: &headers,
        },
        &mime,
        deadline,
    ) {
        // Before anything is filed: the browser fetches this with its
        // cookies and its session, and dm has neither. A link only
        // the signed-in browser can fetch is refused here, while the
        // browser still holds it, rather than taken from the browser
        // and failed in dm — a download caught and then lost.
        Reply::failed(&id, why)
    } else {
        // A caught download asks too, where Options says so; a renewal never does.
        let asks = !renews && (ask || (caught && saved(home).general.ask_caught_downloads));
        match deposit(
            home,
            &id,
            &Request {
                url,
                referrer,
                quality: quality.filter(|q| !q.trim().is_empty()),
                sound: sound
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
                label,
                ask: asks,
                // Only a video asked for from the panel has a part.
                from: if caught {
                    String::new()
                } else {
                    from.trim().to_string()
                },
                to: if caught {
                    String::new()
                } else {
                    to.trim().to_string()
                },
                caught,
                cookies: cookies.trim().to_string(),
                cookies_once: cookies_once && (!cookies.trim().is_empty() || !headers.is_empty()),
                headers: dm_core::cookies::forwardable(&headers),
                title: title.trim().chars().take(300).collect(),
                browser: parent_browser(),
                post,
                from_browser: true,
                renews,
                keep_no_record: private_window,
                ..Request::default()
            },
        ) {
            Ok(_) if world.app_running() => {
                world.ring();
                Reply::started(&id, asks)
            }
            Ok(filed) => match world.launch() {
                Ok(()) => Reply::started(&id, asks),
                // Filed, but nothing will read it: said now, not
                // discovered when the file never appears — and taken
                // back out, since the browser keeps the download and
                // a dm started later must not fetch it as well.
                Err(e) => {
                    // Only one this hand-off filed: the same request
                    // already waiting was answered Started before.
                    if filed.new {
                        let _ = std::fs::remove_file(&filed.path);
                    }
                    Reply::failed(&id, format!("dm could not be started to take it: {e}"))
                }
            },
            Err(e) => Reply::failed(&id, format!("dm could not take it: {e}")),
        }
    }
}

/// What a click on an item of the menu the app served hands over: the link clicked, else the
/// video's or sound's own address, else the page (or frame) it was on, read for its video.
/// The items an app of today serves are carried out by the extension itself
/// (`dm_protocol::menu_does`); this is where an item added later is decided, by the app.
pub(super) fn menu_start(click: &dm_protocol::MenuClicked) -> Result<Start, crate::Error> {
    let web = |url: &str| {
        let lower = url.trim().to_ascii_lowercase();
        lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("ftp://")
    };
    let handable = |url: &str| web(url) || url.trim().to_ascii_lowercase().starts_with("magnet:");
    let page = if click.frame_url.trim().is_empty() {
        click.page_url.trim()
    } else {
        click.frame_url.trim()
    };
    let (url, referrer, read_page) = if handable(&click.link_url) {
        (click.link_url.trim(), click.page_url.trim(), false)
    } else if handable(&click.src_url) {
        (click.src_url.trim(), click.page_url.trim(), false)
    } else if web(page) {
        (page, "", true)
    } else {
        return Err("there is no address here for dm to take".into());
    };
    Ok(Start {
        id: click.id.clone(),
        url: url.to_string(),
        referrer: referrer.to_string(),
        cookies: click.cookies.clone(),
        cookies_once: click.cookies_once,
        title: click.title.clone(),
        // A page is handed over for its video, which the app reads, never saved itself.
        quality: read_page.then(|| "best".to_string()),
        ..Start::default()
    })
}

/// Why dm may not save into `folder`, or `None` where it may: made where
/// it is missing, and a file written in it and taken away again.
pub(crate) fn unwritable(folder: &Path) -> Option<String> {
    // A new file of an unguessable name, never one already there: a link
    // planted at the name is not followed (`create_new`). The count keeps
    // two checks at once (two hand-offs, threads of one process) apart; a
    // name already taken is tried again under the next, and only a file
    // this check made is removed.
    static CHECKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tried = std::fs::create_dir_all(folder).and_then(|()| {
        let mut last = None;
        for _ in 0..3 {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos());
            let count = CHECKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let probe = folder.join(format!(
                ".dm-write-check-{}-{count}-{nanos}",
                std::process::id()
            ));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&probe)
            {
                Ok(file) => {
                    drop(file);
                    let _ = std::fs::remove_file(&probe);
                    return Ok(());
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| std::io::Error::other("no name to check with")))
    });
    tried.err().map(|e| {
        format!(
            "dm may not save into its download folder {} ({e}), so the browser keeps the \
             download; choose another folder in dm's Options › Folders",
            folder.display()
        )
    })
}

/// The request a hand-off would make: its address and what the browser sent with it.
#[derive(Clone, Copy)]
struct Asking<'a> {
    url: &'a str,
    referrer: &'a str,
    cookies: &'a str,
    headers: &'a [(String, String)],
}

/// Why a hand-off must not be taken, or `None` when it can be filed.
///
/// Too late first: past the extension's deadline it has given the download
/// back to the browser, and a download filed now is fetched twice. Then, for
/// a download the browser holds, whether dm can fetch it at all — and
/// whether what dm is served is the file and not a sign-in page the site
/// sends a request without the browser's cookies to, which a check for
/// errors alone passed, losing the browser's copy for a page of HTML. The
/// time is checked again after asking, since asking takes most of it.
fn checked(
    world: &mut dyn World,
    caught: bool,
    asking: Asking<'_>,
    mime: &str,
    deadline: Option<u64>,
) -> Option<String> {
    let Asking {
        url,
        referrer,
        cookies,
        headers,
    } = asking;
    let late = "the browser stopped waiting for dm, so it keeps the download";
    let Some(within) = dm_protocol::time_left(world.now_ms(), deadline) else {
        return Some(late.into());
    };
    if !caught {
        return None;
    }
    match world.reachable_with(url, referrer, cookies, headers, within) {
        Err(why) => Some(format!("dm cannot fetch this without the browser: {why}")),
        Ok(served) if dm_protocol::page_instead_of_file(mime, url, &served) => Some(format!(
            "dm cannot fetch this without the browser: the site answers dm with a page \
             ({}) rather than the file, usually a sign-in",
            served.trim()
        )),
        Ok(_) if dm_protocol::time_left(world.now_ms(), deadline).is_none() => Some(late.into()),
        Ok(_) => None,
    }
}
