// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
        format!(
            "https://r1.googlevideo.com/videoplayback?id={id}&itag=22&mime=video%2Fmp4&sig={sig}"
        )
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
