// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The request headers the browser sent, kept for the download they began.
//!
//! A file or stream a site guards with a header (a bearer token on an API, a
//! playback session) is refused to anything that does not send it. The worker
//! hears each request's headers as they go out (`onSendHeaders`, read only:
//! a Manifest V3 worker cannot change them) and keeps the last ones per
//! address, in memory and bounded, so the hand-off for that address can carry
//! them. They are never written to a log or to storage.

use std::collections::VecDeque;

use serde::Deserialize;

use super::Header;

/// A request as `webRequest.onSendHeaders` reports it.
#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Sent {
    /// The request's address.
    pub url: String,
    /// What kind of request it is, as the browser names it.
    #[serde(rename = "type")]
    pub kind: String,
    /// The headers as sent.
    pub request_headers: Vec<Header>,
    /// The tab that sent it; the browser says -1 for a request of no tab's (a service worker's).
    pub tab_id: Option<i64>,
}

impl Sent {
    /// The tab that sent it, if a tab did.
    pub fn tab(&self) -> Option<i64> {
        self.tab_id.filter(|t| *t >= 0)
    }
}

/// One address's last request: the address, the headers it carried, and the tab that sent it.
pub type Kept = (String, Vec<(String, String)>, Option<i64>);

/// The most addresses remembered, and headers and value length kept for one.
const MOST_ADDRESSES: usize = 200;
/// The most (tab, host) pairs remembered: which hosts each tab's requests went to.
const MOST_REACHED: usize = 2000;
/// The most cookie names kept for one tab and host, and the longest name.
const MOST_COOKIES: usize = 64;
const LONGEST_COOKIE_NAME: usize = 256;
const MOST_HEADERS: usize = 40;
const LONGEST_VALUE: usize = 4096;

/// Headers that belong to one connection, or that dm sets itself.
const NOT_SENT_ON: [&str; 17] = [
    "user-agent",
    "referer",
    "host",
    "range",
    "if-range",
    "content-length",
    "content-type",
    "connection",
    "accept-encoding",
    "cookie",
    "transfer-encoding",
    "upgrade",
    "te",
    "expect",
    "if-none-match",
    "if-modified-since",
    "proxy-authorization",
];

/// Whether a header of a request goes on to the app.
pub fn forwarded(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && !name.starts_with(':')
        && !NOT_SENT_ON.iter().any(|n| n.eq_ignore_ascii_case(name))
}

/// The names of the cookies a request's `Cookie` header carried, each once.
fn cookie_names(headers: &[Header]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for value in headers
        .iter()
        .filter(|h| h.name.trim().eq_ignore_ascii_case("cookie"))
        .filter_map(|h| h.value.as_deref())
    {
        for pair in value.split(';') {
            let name = pair.split('=').next().unwrap_or_default().trim();
            if !name.is_empty()
                && name.len() <= LONGEST_COOKIE_NAME
                && !names.iter().any(|n| n == name)
            {
                names.push(name.to_string());
            }
        }
    }
    names
}

/// The address with its fragment removed, as the browser requests it.
fn key(url: &str) -> &str {
    url.split('#').next().unwrap_or(url)
}

/// The last request headers per address, oldest forgotten first.
#[derive(Debug, Default)]
pub struct RequestHeaders {
    kept: VecDeque<Kept>,
    /// The hosts each tab's requests went to, newest last, each with the names of the cookies the
    /// browser itself sent it from that tab: kept apart from the addresses, which a page's many
    /// requests for pieces turn over within seconds.
    reached: VecDeque<(i64, String, Vec<String>)>,
}

impl RequestHeaders {
    /// Remembers what `sent` carried, replacing an earlier request for the
    /// same address. A request that carries nothing worth sending is not kept.
    pub fn keep(&mut self, sent: &Sent) {
        if !matches!(
            sent.kind.as_str(),
            "main_frame" | "sub_frame" | "media" | "xmlhttprequest" | "other" | "object"
        ) {
            return;
        }
        let headers: Vec<(String, String)> = sent
            .request_headers
            .iter()
            .filter(|h| forwarded(&h.name))
            .filter_map(|h| Some((h.name.clone(), h.value.clone()?)))
            .filter(|(_, v)| v.len() <= LONGEST_VALUE)
            .take(MOST_HEADERS)
            .collect();
        let url = key(&sent.url);
        if let (Some(tab), Some(host)) = (sent.tab(), super::site_of(url)) {
            if url.starts_with("http") {
                // What the browser chose to send this host from this tab (`SameSite`, third-party
                // cookies blocked or not), all its requests' together: the cookies' names alone,
                // their values read from the browser's store when a download needs them.
                let mut names = self
                    .reached
                    .iter()
                    .position(|(t, h, _)| *t == tab && *h == host)
                    .and_then(|at| self.reached.remove(at))
                    .map(|(_, _, names)| names)
                    .unwrap_or_default();
                for name in cookie_names(&sent.request_headers) {
                    if names.len() < MOST_COOKIES && !names.contains(&name) {
                        names.push(name);
                    }
                }
                self.reached.push_back((tab, host, names));
                while self.reached.len() > MOST_REACHED {
                    self.reached.pop_front();
                }
            }
        }
        self.kept.retain(|(u, _, _)| u != url);
        if headers.is_empty() || !url.starts_with("http") {
            return;
        }
        self.kept.push_back((url.to_string(), headers, sent.tab()));
        while self.kept.len() > MOST_ADDRESSES {
            self.kept.pop_front();
        }
    }

    /// Everything kept, oldest first, for the session's storage (memory the browser clears when
    /// it closes), which outlives the worker that heard the requests.
    pub fn export(&self) -> Vec<Kept> {
        self.kept.iter().cloned().collect()
    }

    /// Takes in what an earlier worker kept: older than anything heard since, so an address
    /// heard again keeps its newer headers.
    pub fn merge(&mut self, older: Vec<Kept>) {
        let mut merged: VecDeque<Kept> = older
            .into_iter()
            .filter(|(url, headers, _)| {
                url.starts_with("http")
                    && headers.len() <= MOST_HEADERS
                    && !self.kept.iter().any(|(u, _, _)| u == url)
            })
            .collect();
        merged.extend(std::mem::take(&mut self.kept));
        while merged.len() > MOST_ADDRESSES {
            merged.pop_front();
        }
        self.kept = merged;
    }

    /// Whether the browser itself sent a request of `tab`'s to `url`'s host: the page's player,
    /// a frame or the page fetched from there, with the browser's own cookies for it.
    pub fn reached(&self, tab: Option<i64>, url: &str) -> bool {
        let (Some(tab), Some(host)) = (tab, super::site_of(url)) else {
            return false;
        };
        self.reached.iter().any(|(t, h, _)| *t == tab && *h == host)
    }

    /// The names of the cookies the browser itself sent `url`'s host from `tab`: none where it
    /// sent none (a `SameSite` sign-in to another site's page, third-party cookies blocked) or
    /// the tab's requests never went there. A page can make its tab reach any host, so reaching
    /// it says nothing of its sign-in; what the browser sent there does (gaps row 159 c).
    pub fn sent_cookies(&self, tab: Option<i64>, url: &str) -> Vec<String> {
        let (Some(tab), Some(host)) = (tab, super::site_of(url)) else {
            return Vec::new();
        };
        self.reached
            .iter()
            .find(|(t, h, _)| *t == tab && *h == host)
            .map(|(_, _, names)| names.clone())
            .unwrap_or_default()
    }

    /// The (tab, host, cookie names) of each, oldest first, for the session's storage, as
    /// `export`.
    pub fn export_reached(&self) -> Vec<(i64, String, Vec<String>)> {
        self.reached.iter().cloned().collect()
    }

    /// Takes in what an earlier worker kept, older than anything heard since.
    pub fn merge_reached(&mut self, older: Vec<(i64, String, Vec<String>)>) {
        let mut merged: VecDeque<(i64, String, Vec<String>)> = older
            .into_iter()
            .filter(|(t, h, names)| {
                !h.is_empty()
                    && names.len() <= MOST_COOKIES
                    && !self.reached.iter().any(|(t2, h2, _)| t2 == t && h2 == h)
            })
            .collect();
        merged.extend(std::mem::take(&mut self.reached));
        while merged.len() > MOST_REACHED {
            merged.pop_front();
        }
        self.reached = merged;
    }

    /// The address the page's player asked for most recently that is the same stream as `url`
    /// with a newer signature: the same path, the same query names and the same `itag`, and no
    /// byte-range parameter (a piece is not the file). A signed address runs out; the newest one
    /// is the one that will still be good when the app asks for it. Only `tab`'s own requests
    /// count: another tab's video on the same host is another person's choice, not this one's
    /// renewal (security review, 2026-11-07). `None` when `url` is already the newest, nothing
    /// newer was seen in that tab, or the hand-off came from no tab.
    pub fn renewed(&self, url: &str, tab: Option<i64>) -> Option<String> {
        let tab = tab?;
        const PIECE: [&str; 5] = ["range", "bytestart", "byteend", "rn", "rbuf"];
        let split = |u: &str| {
            let u = key(u).to_string();
            let (path, query) = match u.split_once('?') {
                Some((p, q)) => (p.to_string(), q.to_string()),
                None => (u, String::new()),
            };
            let pairs: Vec<(String, String)> = query
                .split('&')
                .filter(|p| !p.is_empty())
                .map(|p| match p.split_once('=') {
                    Some((n, v)) => (n.to_string(), v.to_string()),
                    None => (p.to_string(), String::new()),
                })
                .collect();
            (path, pairs)
        };
        let (path, pairs) = split(url);
        if !path.starts_with("http") || pairs.iter().any(|(n, _)| PIECE.contains(&n.as_str())) {
            return None;
        }
        let names = |p: &[(String, String)]| {
            let mut n: Vec<String> = p.iter().map(|(n, _)| n.clone()).collect();
            n.sort();
            n
        };
        // What says which stream it is, as against how it is signed: YouTube's `itag`, and its
        // `id` and `mime`, which another video's address on the same edge host does not share
        // (security review, 2026-11-07: another tab's video with the same itag was a "renewal").
        let same = |p: &[(String, String)], q: &[(String, String)]| {
            ["itag", "id", "mime"].iter().all(|name| {
                let value = |x: &[(String, String)]| {
                    x.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone())
                };
                value(p) == value(q)
            })
        };
        let newest = self
            .kept
            .iter()
            .rev()
            .filter(|(_, _, t)| *t == Some(tab))
            .map(|(u, _, _)| u.as_str())
            .find(|u| {
                let (other_path, other) = split(u);
                other_path == path && names(&other) == names(&pairs) && same(&other, &pairs)
            })?;
        (newest != key(url)).then(|| newest.to_string())
    }

    /// What was sent to `url` last, or nothing.
    pub fn of(&self, url: &str) -> Vec<(String, String)> {
        let url = key(url);
        self.kept
            .iter()
            .rev()
            .find(|(u, _, _)| u == url)
            .map(|(_, h, _)| h.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
