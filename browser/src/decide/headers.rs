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
mod tests {
    use super::*;

    /// The tab the tests' requests come from.
    const TAB: Option<i64> = Some(7);

    fn sent(url: &str, kind: &str, headers: &[(&str, &str)]) -> Sent {
        from_tab(url, kind, headers, TAB)
    }

    fn from_tab(url: &str, kind: &str, headers: &[(&str, &str)], tab: Option<i64>) -> Sent {
        Sent {
            tab_id: tab,
            url: url.into(),
            kind: kind.into(),
            request_headers: headers
                .iter()
                .map(|(n, v)| Header {
                    name: (*n).into(),
                    value: Some((*v).into()),
                })
                .collect(),
        }
    }

    #[test]
    fn a_token_is_kept_and_the_connections_own_headers_are_not() {
        let mut kept = RequestHeaders::default();
        kept.keep(&sent(
            "https://api.example/v/1.mp4#t=3",
            "xmlhttprequest",
            &[
                ("Authorization", "Bearer t"),
                ("Range", "bytes=0-"),
                ("Cookie", "a=1"),
                ("User-Agent", "x"),
                ("X-Token", "abc"),
            ],
        ));
        let got = kept.of("https://api.example/v/1.mp4");
        assert_eq!(
            got,
            vec![
                ("Authorization".to_string(), "Bearer t".to_string()),
                ("X-Token".to_string(), "abc".to_string())
            ]
        );
    }

    #[test]
    fn the_last_request_for_an_address_wins_and_the_memory_is_bounded() {
        let mut kept = RequestHeaders::default();
        kept.keep(&sent("https://a.example/f", "media", &[("X-A", "1")]));
        kept.keep(&sent("https://a.example/f", "media", &[("X-A", "2")]));
        assert_eq!(
            kept.of("https://a.example/f"),
            vec![("X-A".to_string(), "2".to_string())]
        );
        for i in 0..(MOST_ADDRESSES + 50) {
            kept.keep(&sent(
                &format!("https://a.example/{i}"),
                "media",
                &[("X-A", "1")],
            ));
        }
        assert!(kept.kept.len() <= MOST_ADDRESSES);
        assert!(kept.of("https://a.example/f").is_empty());
    }

    #[test]
    fn a_stream_asked_for_again_with_a_new_signature_is_renewed() {
        let mut kept = RequestHeaders::default();
        kept.keep(&sent(
            "https://c.example/v.mp4?itag=22&sig=old",
            "media",
            &[("X-A", "1")],
        ));
        kept.keep(&sent(
            "https://c.example/v.mp4?itag=18&sig=other",
            "media",
            &[("X-A", "1")],
        ));
        kept.keep(&sent(
            "https://c.example/v.mp4?itag=22&sig=new",
            "media",
            &[("X-A", "1")],
        ));
        assert_eq!(
            kept.renewed("https://c.example/v.mp4?itag=22&sig=old", TAB)
                .as_deref(),
            Some("https://c.example/v.mp4?itag=22&sig=new")
        );
        assert_eq!(
            kept.renewed("https://c.example/v.mp4?itag=22&sig=new", TAB),
            None
        );
        // A piece is not the file, and another path is another stream.
        assert_eq!(
            kept.renewed("https://c.example/v.mp4?itag=22&sig=old&range=0-9", TAB),
            None
        );
        assert_eq!(
            kept.renewed("https://c.example/w.mp4?itag=22&sig=old", TAB),
            None
        );
    }

    /// Another video on the same edge host, with the same itag (another tab's, or an embed's), is
    /// not a renewal of this one: YouTube's `id` tells them apart.
    #[test]
    fn another_videos_address_with_the_same_itag_is_not_a_renewal() {
        let mut kept = RequestHeaders::default();
        let at = |id: &str, sig: &str| {
            format!("https://r1.googlevideo.com/videoplayback?id={id}&itag=22&mime=video%2Fmp4&sig={sig}")
        };
        kept.keep(&sent(&at("aaa", "old"), "media", &[("X-A", "1")]));
        kept.keep(&sent(&at("bbb", "new"), "media", &[("X-A", "1")]));
        assert_eq!(kept.renewed(&at("aaa", "old"), TAB), None);
        kept.keep(&sent(&at("aaa", "newer"), "media", &[("X-A", "1")]));
        assert_eq!(
            kept.renewed(&at("aaa", "old"), TAB),
            Some(at("aaa", "newer"))
        );
    }

    /// The same video asked for anew in another tab, or by no tab (a service worker), is not
    /// this tab's renewal; a hand-off from no tab is renewed from nowhere.
    #[test]
    fn only_the_handed_tabs_own_requests_renew_it() {
        let mut kept = RequestHeaders::default();
        let at = |sig: &str| format!("https://cdn.example/v.mp4?itag=22&sig={sig}");
        kept.keep(&sent(&at("old"), "media", &[("X-A", "1")]));
        kept.keep(&from_tab(&at("other"), "media", &[("X-A", "1")], Some(8)));
        kept.keep(&from_tab(&at("worker"), "media", &[("X-A", "1")], Some(-1)));
        assert_eq!(kept.renewed(&at("old"), TAB), None);
        assert_eq!(kept.renewed(&at("old"), Some(8)), Some(at("other")));
        assert_eq!(kept.renewed(&at("old"), None), None);
        kept.keep(&sent(&at("new"), "media", &[("X-A", "1")]));
        assert_eq!(kept.renewed(&at("old"), TAB), Some(at("new")));
    }

    /// The hosts a tab's requests went to are that tab's alone, outlive the addresses' turnover,
    /// and survive the worker's nap.
    #[test]
    fn the_hosts_a_tab_reached_are_kept_per_tab() {
        let mut kept = RequestHeaders::default();
        kept.keep(&sent(
            "https://v16.tiktokcdn.com/v.mp4?sig=a",
            "media",
            &[("X-A", "1")],
        ));
        kept.keep(&from_tab(
            "https://bank.example/",
            "main_frame",
            &[("X-A", "1")],
            Some(9),
        ));
        kept.keep(&from_tab(
            "https://worker.example/x",
            "other",
            &[("X-A", "1")],
            Some(-1),
        ));
        for i in 0..(MOST_ADDRESSES + 10) {
            kept.keep(&sent(
                &format!("https://www.site.example/{i}"),
                "xmlhttprequest",
                &[("X-A", "1")],
            ));
        }
        assert!(kept.reached(TAB, "https://v16.tiktokcdn.com/other.mp4"));
        assert!(
            kept.reached(TAB, "https://site.example/f"),
            "www. is the same site"
        );
        assert!(
            !kept.reached(TAB, "https://bank.example/statement.pdf"),
            "another tab's"
        );
        assert!(kept.reached(Some(9), "https://bank.example/statement.pdf"));
        assert!(!kept.reached(Some(-1), "https://worker.example/x"));
        assert!(!kept.reached(None, "https://v16.tiktokcdn.com/v.mp4"));
        let mut next = RequestHeaders::default();
        next.merge_reached(kept.export_reached());
        assert!(next.reached(TAB, "https://v16.tiktokcdn.com/v.mp4"));
    }

    /// The cookies a tab's requests carried to a host are kept by name, every request's
    /// together, for that tab alone: what the browser itself chose to send there. A request
    /// that carried none, as a page's own `fetch` of another site carries none of a `SameSite`
    /// sign-in, adds nothing (gaps row 159 c).
    #[test]
    fn the_cookies_the_browser_sent_a_host_from_a_tab_are_kept_by_name() {
        let mut kept = RequestHeaders::default();
        kept.keep(&sent(
            "https://v16.tiktokcdn.com/v.mp4?sig=a",
            "media",
            &[("Cookie", "tt_chain_token=abc; msToken=x=y"), ("X-A", "1")],
        ));
        kept.keep(&sent(
            "https://v16.tiktokcdn.com/v.mp4?sig=b",
            "media",
            &[("Cookie", "tt_chain_token=abc; ttwid=2"), ("X-A", "1")],
        ));
        // A page's own request to another site, which the browser sent with none of its cookies.
        kept.keep(&sent(
            "https://bank.example/statement.pdf",
            "xmlhttprequest",
            &[("X-A", "1")],
        ));
        assert_eq!(
            kept.sent_cookies(TAB, "https://v16.tiktokcdn.com/w.mp4"),
            ["tt_chain_token", "msToken", "ttwid"],
        );
        assert!(kept.reached(TAB, "https://bank.example/x"), "reached");
        assert!(
            kept.sent_cookies(TAB, "https://bank.example/statement.pdf")
                .is_empty(),
            "but sent no cookie"
        );
        assert!(
            kept.sent_cookies(Some(9), "https://v16.tiktokcdn.com/v.mp4")
                .is_empty(),
            "another tab's"
        );
        assert!(kept
            .sent_cookies(None, "https://v16.tiktokcdn.com/v.mp4")
            .is_empty());
        let mut next = RequestHeaders::default();
        next.merge_reached(kept.export_reached());
        assert_eq!(
            next.sent_cookies(TAB, "https://v16.tiktokcdn.com/v.mp4")
                .len(),
            3
        );
    }

    #[test]
    fn an_image_or_a_script_is_not_kept() {
        let mut kept = RequestHeaders::default();
        kept.keep(&sent("https://a.example/i.png", "image", &[("X-A", "1")]));
        assert!(kept.of("https://a.example/i.png").is_empty());
    }
}
