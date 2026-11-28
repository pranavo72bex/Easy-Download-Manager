// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

fn document(extra: &str) -> String {
    format!(r#"{{ "format": 1, "revision": 3, {extra} }}"#)
}

#[test]
fn a_document_is_read_and_its_lists_tidied() {
    let text = document(
        r#""exclude": { "advert_hosts": [" Ads.Example.COM ", "", "a b", "bad/host", "ads.example.com", ".lead.dot"],
                         "windows_media_floor": 999999999999 },
           "sites": [ { "host": " Video.Example ", "answers": [
                { "url_contains": " /API/Play ", "keys": ["hlsUrl", ""] },
                { "url_contains": "   " } ] },
              { "host": "", "answers": [ { "url_contains": "x" } ] },
              { "host": "no.answers.example" } ]"#,
    );
    let rules = Rules::parse(&text).expect("a document");
    assert_eq!(rules.revision, 3);
    assert_eq!(rules.exclude.advert_hosts, vec!["ads.example.com"]);
    assert_eq!(rules.exclude.windows_media_floor, Some(MAX_MEDIA_FLOOR));
    assert_eq!(rules.sites.len(), 1);
    assert_eq!(rules.sites[0].host, "video.example");
    assert_eq!(rules.sites[0].answers.len(), 1);
    assert_eq!(rules.sites[0].answers[0].url_contains, "/api/play");
    assert_eq!(rules.sites[0].answers[0].keys, vec!["hlsUrl"]);
}

/// A site's rule can name the element its pages keep a video's title in (gaps row 124): a
/// site with that alone is kept, found by its sub-domains, and an over-long one is dropped.
#[test]
fn a_title_selector_is_read_and_found_by_site() {
    let long = "x".repeat(MAX_FINDER_TEXT + 1);
    let rules = Rules::parse(&document(&format!(
        r#""sites": [ {{ "host": "videos.example", "title": " h1.video-title " }},
                      {{ "host": "long.example", "title": "{long}" }} ]"#
    )))
    .expect("a document");
    assert_eq!(
        rules.title_for("www.videos.example"),
        Some("h1.video-title")
    );
    assert_eq!(rules.title_for("other.example"), None);
    assert_eq!(rules.title_for("long.example"), None);
    assert_eq!(rules.sites.len(), 1, "a site with nothing left is dropped");
}

#[test]
fn finders_and_private_fetches_are_read_tidied_and_found_by_site() {
    let rules = Rules::parse(&document(
        r#""sites": [ { "host": "feed.example", "finders": [
                { "player": " div.vp ", "id_path": "*.return.props.videoID" },
                { "player": "", "id_path": "x" },
                { "player": "video" } ],
              "private_fetches": ["w|a.b|fetch", "bad", "a||c"] } ]"#,
    ))
    .expect("a document");
    assert_eq!(rules.sites.len(), 1, "a site with finders only is kept");
    let finders = rules.finders_for("www.feed.example");
    assert_eq!(finders.len(), 1);
    assert_eq!(finders[0].player, "div.vp");
    assert_eq!(rules.private_fetches_for("feed.example"), ["w|a.b|fetch"]);
    assert!(rules.finders_for("other.example").is_empty());
}

/// A finder's field past the sane length is cleared, and a finder left
/// with no way to find an id is dropped with it.
#[test]
fn an_over_long_finder_field_is_cleared() {
    let long = "x".repeat(MAX_FINDER_TEXT + 1);
    let rules = Rules::parse(&document(&format!(
        r#""sites": [ {{ "host": "feed.example", "finders": [
                {{ "player": "div.vp", "id_path": "a.b", "container": "{long}" }},
                {{ "player": "div.vp", "id_attr": "{long}" }} ] }} ]"#
    )))
    .expect("a document");
    let finders = rules.finders_for("feed.example");
    assert_eq!(finders.len(), 1, "the one with only a long id is gone");
    assert_eq!(finders[0].id_path, "a.b");
    assert!(finders[0].container.is_empty());
}

#[test]
fn a_document_that_is_not_one_is_refused() {
    assert!(Rules::parse("not json").is_err());
    assert!(Rules::parse("[1, 2]").is_err());
    assert!(Rules::parse("{}").is_err(), "no format");
    assert!(
        Rules::parse(r#"{ "format": 2, "revision": 9 }"#).is_err(),
        "a format this side does not read"
    );
    let huge = format!(
        r#"{{ "format": 1, "revision": 1, "exclude": {{ "advert_hosts": ["{}"] }} }}"#,
        "a".repeat(MAX_RULES_BYTES)
    );
    assert!(Rules::parse(&huge).is_err(), "over the size cap");
}

#[test]
fn a_field_this_side_does_not_know_is_ignored() {
    let rules = Rules::parse(&document(r#""future": { "x": 1 }"#)).expect("a document");
    assert_eq!(rules.revision, 3);
    assert!(rules.sites.is_empty());
}

#[test]
fn the_newer_revision_wins_and_a_tie_keeps_the_shipped_one() {
    let shipped = Rules {
        format: 1,
        revision: 5,
        ..Rules::default()
    };
    let newer = Rules {
        revision: 6,
        ..shipped.clone()
    };
    let older = Rules {
        revision: 4,
        ..shipped.clone()
    };
    let same = Rules {
        exclude: ExcludeRules {
            advert_hosts: vec!["x.example".into()],
            ..ExcludeRules::default()
        },
        ..shipped.clone()
    };
    assert_eq!(Rules::newest(shipped.clone(), Some(newer.clone())), newer);
    assert_eq!(Rules::newest(shipped.clone(), Some(older)), shipped);
    assert_eq!(Rules::newest(shipped.clone(), Some(same)), shipped);
    assert_eq!(Rules::newest(shipped.clone(), None), shipped);
}

#[test]
fn a_page_gets_the_answer_rules_of_its_site_and_the_sites_it_is_part_of() {
    let rules = Rules::parse(&document(
        r#""sites": [ { "host": "video.example", "answers": [ { "url_contains": "/play" } ] },
                      { "host": "other.example", "answers": [ { "url_contains": "/o" } ] } ]"#,
    ))
    .expect("a document");
    assert_eq!(rules.answers_for("video.example").len(), 1);
    assert_eq!(rules.answers_for("WWW.video.example").len(), 1);
    assert_eq!(rules.answers_for("m.video.example").len(), 1);
    assert!(rules.answers_for("notvideo.example").is_empty());
    assert!(rules.answers_for("example").is_empty());
}

#[test]
fn a_hello_carries_the_rules_and_an_old_hello_has_none() {
    let hello: crate::Hello =
        serde_json::from_str(r#"{ "engineVersion": "1.0.0" }"#).expect("an old hello");
    assert_eq!(hello.rules, None);
    let with = crate::Hello {
        rules: Some(Rules {
            format: 1,
            revision: 2,
            ..Rules::default()
        }),
        ..crate::Hello::default()
    };
    let text = serde_json::to_string(&with).expect("text");
    let back: crate::Hello = serde_json::from_str(&text).expect("a hello");
    assert_eq!(back, with);
    let none = serde_json::to_string(&crate::Hello::default()).expect("text");
    assert!(!none.contains("rules"), "{none}");
}

/// The YouTube clients a document lists are read in order, and one that is not whole (no
/// number, a number that is not one, a line break in its agent) is left out (gaps row Y1).
#[test]
fn youtube_clients_are_read_and_the_broken_ones_left_out() {
    let doc = format!(
        r#"{{"format": {RULES_FORMAT}, "revision": 9, "youtube": {{"clients": [
            {{"name": "TVHTML5", "version": "7.1", "id": "7", "agent": "TV", "context": {{"osName": "Tizen"}}}},
            {{"name": "WEB", "version": "2.1", "id": "x", "agent": "Web"}},
            {{"name": "IOS", "version": "1", "id": "5", "agent": "a\nb"}},
            {{"name": " ANDROID ", "version": "19.1", "id": "3", "agent": "Droid"}}
        ]}}}}"#
    );
    let rules = Rules::parse(&doc).expect("a document");
    let names: Vec<&str> = rules
        .youtube
        .clients
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, ["TVHTML5", "ANDROID"]);
    assert_eq!(rules.youtube.clients[0].context["osName"], "Tizen");
    // A document without it reads as before.
    let plain = Rules::parse(&format!(r#"{{"format": {RULES_FORMAT}, "revision": 1}}"#)).unwrap();
    assert!(plain.youtube.clients.is_empty());
}
