// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What dm asks a server before a download starts, for a presentation to
//! show while a person decides: a file's name and size (Download details), a
//! page's links each with its name, kind and size ("Download all links"), a
//! video page's qualities or the videos it lists, and a playlist. Each is
//! asked in the background with the application's client and settings, and
//! told to a callback; what is shown, and in which words, is the
//! presentation's.

use std::sync::Arc;
use std::time::Duration;

use crate::app::backend::Backend;

/// How long a server may take to say what a file is.
pub const PROBE_WITHIN: Duration = Duration::from_secs(20);

/// How long a video page's qualities, or the videos it lists, may take.
pub const CHOICES_WITHIN: Duration = Duration::from_secs(40);

/// How many of a page's links are asked about at once.
pub const LINKS_AT_ONCE: usize = 6;

/// What Download details learned by asking the server: the name the file
/// will have, where it gives one, and its size, where it says.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Probed {
    /// The name the file will have.
    pub name: Option<String>,
    /// The file's size in bytes.
    pub size: Option<u64>,
    /// Why the server would not give the file, where it said: a 404 was
    /// found only once the download had failed.
    pub problem: Option<String>,
    /// A web page rather than a file (a video site's watch page): dm finds
    /// the video in it when it starts, so nothing is wrong to say.
    pub page: bool,
    /// The `https://` address taken in place of the `http://` one typed, where it answered
    /// (gaps row H6).
    pub upgraded: Option<String>,
}

/// The name an HLS stream at `url` is saved under: the engine's own naming
/// for streams (a manifest called `index.m3u8` takes its folder's name),
/// `.mp4` where transport streams are remuxed (`remux`). `None` for what is
/// not an HLS stream.
pub fn stream_name(url: &str, content_type: Option<&str>, remux: bool) -> Option<String> {
    if dm_core::kind_of(url, content_type) != Some(dm_core::StreamKind::Hls) {
        return None;
    }
    // The engine names a stream from its address and what its segments
    // are; asked with one transport-stream segment, it names it as one.
    let one = "#EXTM3U\n#EXTINF:1,\nsegment.ts\n#EXT-X-ENDLIST\n";
    let named = dm_core::media_playlist(one, url, url, Default::default())
        .ok()?
        .filename;
    match named.strip_suffix(".ts") {
        Some(stem) if remux => Some(format!("{stem}.mp4")),
        _ => Some(named),
    }
}

fn referrer_of(referrer: &str) -> Option<&str> {
    Some(referrer).filter(|r| !r.is_empty())
}

/// Asks the server of `url` what the file is, in the background, and gives
/// `done` what Download details shows of it. A web page's name ("watch") and
/// size are no file's, so are left unsaid; a stream's are its file's, not its
/// playlist's. A watch page that turned the quick look away (a 403 or a 429
/// bot check) is the video reader's when the download starts: said as a page,
/// with no problem. Nothing is given where the server did not answer in
/// [`PROBE_WITHIN`].
pub fn probe_later(
    backend: &Backend,
    url: String,
    referrer: String,
    sign_in: Option<dm_core::cookies::SignIn>,
    remux: bool,
    done: impl FnOnce(Probed) + Send + 'static,
) {
    let client = backend.client();
    backend.runtime.spawn(async move {
        let asked = tokio::time::timeout(
            PROBE_WITHIN,
            dm_core::probe(&client, &url, referrer_of(&referrer), sign_in.as_ref()),
        )
        .await;
        if let Some(found) = probed_of(&url, asked, remux) {
            done(found);
        }
    });
}

/// [`probe_later`] for an address a person typed: where it is a plain `http://` one to a server
/// on the internet, its `https://` form asked first, within [`SECURE_WITHIN`], and taken where it
/// answers (`Probed::upgraded`); the address as typed otherwise (gaps row H6).
pub fn probe_upgrading(
    backend: &Backend,
    url: String,
    referrer: String,
    sign_in: Option<dm_core::cookies::SignIn>,
    remux: bool,
    done: impl FnOnce(Probed) + Send + 'static,
) {
    let Some(secure) = secure_form(&url) else {
        return probe_later(backend, url, referrer, sign_in, remux, done);
    };
    let client = backend.client();
    backend.runtime.spawn(async move {
        let first = tokio::time::timeout(
            SECURE_WITHIN,
            dm_core::probe(&client, &secure, referrer_of(&referrer), sign_in.as_ref()),
        )
        .await;
        if let Ok(Ok(_)) = &first {
            if let Some(mut found) = probed_of(&secure, first, remux) {
                found.upgraded = Some(secure);
                done(found);
            }
            return;
        }
        let asked = tokio::time::timeout(
            PROBE_WITHIN,
            dm_core::probe(&client, &url, referrer_of(&referrer), sign_in.as_ref()),
        )
        .await;
        if let Some(found) = probed_of(&url, asked, remux) {
            done(found);
        }
    });
}

/// How long an address's `https://` form is given to answer before the `http://` one typed is
/// asked instead.
pub const SECURE_WITHIN: Duration = Duration::from_secs(5);

/// The `https://` form of a plain `http://` address to a server on the internet; `None` for any
/// other address, and for one to this computer or the local network (an address written as
/// numbers, `localhost`, a `.local` name), whose servers seldom speak HTTPS.
pub fn secure_form(url: &str) -> Option<String> {
    let rest = url.strip_prefix("http://")?;
    let host = url::Url::parse(url).ok()?.host()?.to_owned();
    match host {
        url::Host::Domain(name) => {
            let name = name.to_ascii_lowercase();
            let local = name == "localhost" || name.ends_with(".local") || !name.contains('.');
            (!local).then(|| format!("https://{rest}"))
        }
        url::Host::Ipv4(_) | url::Host::Ipv6(_) => None,
    }
}

/// What Download details shows of a probe's `asked` answer for `url`; `None` where the server
/// did not answer in time.
fn probed_of(
    url: &str,
    asked: Result<Result<dm_core::Probe, dm_core::Error>, tokio::time::error::Elapsed>,
    remux: bool,
) -> Option<Probed> {
    let mut answer = None;
    {
        let mut done = |found: Probed| answer = Some(found);
        let url = url.to_string();
        match asked {
            Ok(Ok(probe)) => {
                let kind = probe.content_type.as_deref();
                done(if let Some(name) = stream_name(&url, kind, remux) {
                    Probed {
                        name: Some(name),
                        ..Probed::default()
                    }
                } else if dm_core::videopage::is_page(kind) {
                    Probed {
                        page: !dm_core::names_a_document(&url),
                        ..Probed::default()
                    }
                } else {
                    Probed {
                        name: Some(probe.filename).filter(|n| !n.is_empty()),
                        size: probe.size,
                        ..Probed::default()
                    }
                });
            }
            Ok(Err(e)) => {
                let refused_page = matches!(
                    e,
                    dm_core::Error::Refused {
                        status: 403 | 429,
                        ..
                    }
                ) && !dm_core::names_a_document(&url);
                done(if refused_page {
                    Probed {
                        page: true,
                        ..Probed::default()
                    }
                } else {
                    Probed {
                        problem: Some(e.to_string()),
                        ..Probed::default()
                    }
                });
            }
            Err(_) => {}
        }
    }
    answer
}

/// What one of a page's links turned out to be, as its server said.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkFound {
    /// The server answered.
    Known {
        /// The name the server gave it.
        name: String,
        /// Its kind, by its name and the type the server gave.
        kind: dm_core::Category,
        /// Its size in bytes, where the server said.
        size: Option<u64>,
        /// Whether it is a web page rather than a file.
        page: bool,
    },
    /// The server could not be asked, with why: said, rather than left
    /// waiting for ever as though still asking.
    Failed(String),
}

/// Asks the server of each of `links` (found on `page`, with the page's
/// sign-in) what it is, [`LINKS_AT_ONCE`] at a time, in the background, and
/// tells `each` as each answers.
pub fn probe_links(
    backend: &Backend,
    links: Vec<String>,
    page: String,
    sign_in: Option<dm_core::cookies::SignIn>,
    each: impl Fn(String, LinkFound) + Send + Sync + 'static,
) {
    let client = backend.client();
    let gate = Arc::new(tokio::sync::Semaphore::new(LINKS_AT_ONCE));
    let each = Arc::new(each);
    for url in links {
        let (client, gate, each) = (client.clone(), Arc::clone(&gate), Arc::clone(&each));
        let (sign_in, page) = (sign_in.clone(), page.clone());
        backend.runtime.spawn(async move {
            let Ok(_turn) = gate.acquire().await else {
                return;
            };
            let asked = tokio::time::timeout(
                PROBE_WITHIN,
                dm_core::probe(&client, &url, referrer_of(&page), sign_in.as_ref()),
            )
            .await;
            let found = match asked {
                Ok(Ok(probe)) => LinkFound::Known {
                    kind: dm_core::classify(&probe.filename, probe.content_type.as_deref()),
                    page: dm_core::videopage::is_page(probe.content_type.as_deref()),
                    name: probe.filename,
                    size: probe.size,
                },
                Ok(Err(e)) => LinkFound::Failed(e.to_string()),
                Err(_) => LinkFound::Failed("the server did not answer in time".to_string()),
            };
            each(url, found);
        });
    }
}

/// Reads the playlist at `url` in the background (YouTube's by dm itself;
/// a feed's items, RSS or Atom, as its videos (gaps row W10); another
/// site's from its page) and gives `done` its videos, or why not.
pub fn read_playlist_later(
    backend: &Backend,
    url: String,
    done: impl FnOnce(Result<dm_core::youtube::Playlist, String>) + Send + 'static,
) {
    let client = backend.client();
    let proxy = backend.settings().advanced.proxy.url_for_tools();
    backend.runtime.spawn(async move {
        let got = if dm_core::youtube::is_list(&url) {
            dm_core::youtube::read_playlist(&client, &url, None)
                .await
                .map_err(|e| e.to_string())
        } else if dm_core::feed::looks_like_feed(&url) {
            dm_core::feed::read_feed(&client, &url)
                .await
                .map_err(|e| e.to_string())
        } else {
            match dm_core::videopage::read_playlist(&url, None, proxy.as_deref(), None).await {
                Ok(Some(list)) => Ok(list),
                Ok(None) => Err("That page is one video, not a playlist.".to_string()),
                Err(e) => Err(e.to_string()),
            }
        };
        done(got);
    });
}

/// Reads the qualities a video page at `url` offers, in the background, and
/// gives `done` them and, where another site's page has none (`youtube`
/// false), the videos it lists when there are several (a channel, a
/// showcase, an album). Each reading has [`CHOICES_WITHIN`].
pub fn list_choices_later(
    backend: &Backend,
    url: String,
    referrer: Option<String>,
    sign_in: Option<dm_core::cookies::SignIn>,
    youtube: bool,
    done: impl FnOnce(Vec<dm_core::videopage::Choice>, Option<dm_core::youtube::Playlist>)
        + Send
        + 'static,
) {
    let proxy = backend.settings().advanced.proxy.url_for_tools();
    backend.runtime.spawn(async move {
        let listed = tokio::time::timeout(
            CHOICES_WITHIN,
            dm_core::videopage::list_choices(
                &url,
                referrer.as_deref(),
                proxy.as_deref(),
                sign_in.as_ref(),
            ),
        )
        .await;
        let listed = match listed {
            Ok(Ok(list)) => list,
            _ => Vec::new(),
        };
        let mut several = None;
        if listed.is_empty() && !youtube {
            let read = dm_core::videopage::read_playlist(
                &url,
                referrer.as_deref(),
                proxy.as_deref(),
                sign_in.as_ref(),
            );
            if let Ok(Ok(Some(playlist))) = tokio::time::timeout(CHOICES_WITHIN, read).await {
                if playlist.entries.len() > 1 {
                    several = Some(playlist);
                }
            }
        }
        done(listed, several);
    });
}

#[cfg(test)]
mod tests;
