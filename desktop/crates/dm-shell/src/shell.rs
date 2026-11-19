// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the app does around its window: how it starts, how it takes a
//! browser's hand-off, how a settings change is put into force.
//!
//! None of it depends on how the window is drawn, so it lives here rather
//! than in the window's crate, and a test can drive it with no window.

use std::path::{Path, PathBuf};

use dm_core::{Checksum, DownloadList, Id};

use crate::host;
use crate::Error;

// Opening a file and showing it in its folder, one file for each system.
mod opener;
use crate::settings::{self, Settings};
#[cfg(all(test, unix, not(target_os = "macos")))]
use opener::linux_open_in;

mod after;
mod args;
mod inbox;
mod open;
mod start;
pub use after::{run_after, run_program, words};
pub use args::{
    args, browser_launch, launched_at_login, launcher_known, launcher_known_in, one_link_launch,
    orders, unrecognised, Orders, HOST_MANIFEST, WHEN_DONE,
};
pub use inbox::{
    ask_running_to_quit, ask_running_to_retry, hand_off, hand_to_running, leave,
    leave_new_download, leave_when_done, opened_by_system, retry_asked, route_to_window,
    take_from_browser, watch_inbox, Taken, NEW_DOWNLOAD_FILE, QUIT, RETRY_PREFIX, WHEN_DONE_FILE,
};
#[cfg(test)]
use inbox::{open_bell, wait_for_inbox};
pub use open::{
    for_shell, open_file, open_with, open_with_command, plain, reveal_in_folder, verbatim,
    SHELL_PATH_MOST,
};
pub use start::{apply, check, start, use_cookies, wake, watch_rules, Started};

/// The form a hand-over's download is the answer to, as the engine sends it:
/// `None` for none, or a body that is not base64 or is too big to be a form.
pub fn posted(url: &str, post: Option<&dm_protocol::Posted>) -> Option<dm_core::cookies::Posted> {
    use base64::Engine;
    let post = post?;
    let (body, content_type) = if post.raw.is_empty() {
        (
            post.form.clone().into_bytes(),
            if post.content_type.is_empty() {
                "application/x-www-form-urlencoded".to_string()
            } else {
                post.content_type.clone()
            },
        )
    } else {
        (
            base64::engine::general_purpose::STANDARD
                .decode(post.raw.trim())
                .ok()?,
            post.content_type.clone(),
        )
    };
    (body.len() <= dm_core::cookies::POST_MOST).then(|| dm_core::cookies::Posted {
        url: url.trim().to_string(),
        body,
        content_type,
    })
}

/// What a request from the command line or a browser asks the queue for.
pub fn add_options(request: &host::Request) -> dm_core::AddOptions {
    dm_core::AddOptions {
        // The panel's "Part" line, where no Download details asks (contract row B59): kept, not
        // dropped for the whole video.
        span: part_span(&request.from, &request.to),
        referrer: request.referrer.clone(),
        save_dir: request.save_dir.clone(),
        save_name: request.save_name.clone(),
        later: request.later,
        quality: request.quality.clone(),
        sound: request.sound.clone(),
        cookies: request.cookies.clone(),
        cookies_once: request.cookies_once,
        headers: request.headers.clone(),
        post: posted(&request.url, request.post.as_ref()),
        title_hint: Some(request.title.clone()).filter(|t| !t.is_empty()),
        queue: if request.later {
            dm_core::MAIN_QUEUE.to_string()
        } else {
            String::new()
        },
        ..dm_core::AddOptions::default()
    }
}

/// The part of a video a hand-off asks for (the browser panel's "Part" line), as Download
/// details takes it: an end alone starts at 0:00. A start alone, or ends that make no span (an end
/// before its start), ask for none: the whole video, as a click with nothing typed.
pub fn part_span(from: &str, to: &str) -> Option<dm_core::Span> {
    if to.trim().is_empty() {
        return None;
    }
    let from = if from.trim().is_empty() { "0:00" } else { from };
    dm_core::Span::parse(from, to)
}

/// Why `url` cannot be added, where that can be told before fetching: a
/// web or FTP address with no site in it (`http://`) was accepted and only
/// failed later with "builder error: empty host".
///
/// Read from the text as typed, not only as parsed: the parser turns
/// `http:///x` into a site called `x`, and that row failed sixteen seconds
/// later with a name lookup error. A magnet with no torrent hash in it is
/// refused here too, rather than failing once the engine has it.
pub fn unusable(url: &str) -> Option<String> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("magnet:") {
        return (!magnet_names_a_torrent(url))
            .then(|| format!("{url} names no torrent to download (no valid xt=urn:btih: hash)"));
    }
    let rest = ["http://", "https://", "ftp://", "ftps://"]
        .iter()
        .find_map(|scheme| lower.strip_prefix(scheme))?;
    // The authority as typed: up to the first `/`, `?` or `#`, less any
    // `user:password@`.
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let site = authority.rsplit('@').next().unwrap_or("");
    if site.is_empty() || site.starts_with(':') {
        return Some(format!("{url} names no site to download from"));
    }
    match url::Url::parse(url) {
        // Port 0 is not one a server listens on: the row could only fail.
        Ok(parsed) if parsed.port() == Some(0) => {
            Some(format!("{url} names port 0, which no server listens on"))
        }
        Ok(parsed) if parsed.host_str().is_some_and(|h| !h.is_empty()) => None,
        Ok(_) | Err(url::ParseError::EmptyHost) => {
            Some(format!("{url} names no site to download from"))
        }
        Err(e) => Some(format!("{url} is not a link dm can read ({e})")),
    }
}

/// Whether a magnet carries a torrent's hash: an `xt` of `urn:btih:` with
/// 40 hex digits or 32 base32 letters, or a v2 `urn:btmh:` multihash.
fn magnet_names_a_torrent(url: &str) -> bool {
    let query = url.split_once('?').map_or("", |(_, q)| q);
    url::form_urlencoded::parse(query.as_bytes())
        .filter(|(key, _)| key == "xt" || key.starts_with("xt."))
        .any(|(_, topic)| {
            let topic = topic.to_ascii_lowercase();
            if let Some(hash) = topic.strip_prefix("urn:btih:") {
                (hash.len() == 40 && hash.chars().all(|c| c.is_ascii_hexdigit()))
                    || (hash.len() == 32
                        && hash.chars().all(|c| matches!(c, 'a'..='z' | '2'..='7')))
            } else if let Some(hash) = topic.strip_prefix("urn:btmh:") {
                // sha2-256 multihash: `1220` and 64 hex digits.
                hash.len() == 68
                    && hash.starts_with("1220")
                    && hash.chars().all(|c| c.is_ascii_hexdigit())
            } else {
                false
            }
        })
}

/// Queues a URL, or several pasted together.
///
/// Whitespace-separated rather than one at a time: pasting a list is the
/// normal way people add more than one. What counts as a URL is the queue's
/// to decide, so the window and the browser hand-off cannot disagree.
///
/// A link in the list that cannot be used is skipped, not the whole paste:
/// the rest are added, and what was skipped comes back with the reason.
pub async fn add(queue: &DownloadList, text: &str) -> Result<Added, Error> {
    let mut skipped = Vec::new();
    let urls: Vec<&str> = text
        .split_whitespace()
        .filter(|u| match unusable(u) {
            Some(why) => {
                skipped.push(why);
                false
            }
            None => true,
        })
        .collect();
    let ids = if urls.is_empty() {
        Vec::new()
    } else {
        queue.add_all(&urls).await
    };
    if ids.len() < urls.len() {
        skipped.push(match urls.len() - ids.len() {
            1 => "one of the links is not one dm can download".to_string(),
            n => format!("{n} of the links are not ones dm can download"),
        });
    }
    if ids.is_empty() {
        return Err(skipped
            .into_iter()
            .next()
            .unwrap_or_else(|| "that is not a URL dm can download".into())
            .into());
    }
    wake(queue);
    Ok(Added { ids, skipped })
}

/// What `add` added, and why any link was skipped.
#[derive(Debug, Default)]
pub struct Added {
    /// The rows added, in the order asked.
    pub ids: Vec<Id>,
    /// Each link not added, with why.
    pub skipped: Vec<String>,
}

/// A digest typed against a row: empty clears it, which is how one pasted by
/// mistake is undone.
pub fn checksum(text: &str) -> Result<Option<Checksum>, Error> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    Checksum::parse(text)
        .map(Some)
        .ok_or_else(|| "that is not a checksum dm recognises".into())
}

#[cfg(test)]
mod tests;
