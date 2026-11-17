// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

#[test]
fn options_writes_only_what_it_changed() {
    let opened = Settings::default();
    // Options changed the connections.
    let mut draft = opened.clone();
    draft.connections = 4;
    // Meanwhile View › Language and the Scheduler changed others.
    let mut now = opened.clone();
    now.language = "de".into();
    now.schedule.enabled = true;
    let wanted = changed_only(&opened, &draft, &now).unwrap();
    assert_eq!(wanted.connections, 4, "Options' own change");
    assert_eq!(wanted.language, "de", "the language picked meanwhile");
    assert!(wanted.schedule.enabled, "the schedule set meanwhile");
}

#[test]
fn the_lists_typed_are_read_as_words_and_sites() {
    assert_eq!(words(" .ZIP; pdf,iso "), ["zip", "pdf", "iso"]);
    assert_eq!(
        sites(&[
            "https://Example.com/a".into(),
            "b.org:8080".into(),
            ".c.net".into()
        ]),
        ["example.com", "b.org", ".c.net"]
    );
    let opened = Settings::default();
    let typed = Typed {
        types: "zip pdf".into(),
        skip: "https://skip.example/page".into(),
        ..Typed::of(&opened)
    };
    let kept = ok(&opened, &opened, &typed, &opened).unwrap();
    assert_eq!(kept.catch.extensions, ["zip", "pdf"]);
    assert_eq!(kept.catch.skip_hosts, ["skip.example"]);
}

/// A site pasted as an address is kept by its host name,
/// which is what the rules match.
#[test]
fn sites_are_kept_by_their_host_name() {
    let given: Vec<String> = [
        "https://ads.example.com/path",
        "tracker.example:8080",
        "example.org/files",
        "plain.example",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(
        sites(&given),
        [
            "ads.example.com",
            "tracker.example",
            "example.org",
            "plain.example"
        ]
    );
}

#[test]
fn a_proxy_with_nowhere_to_go_is_refused_on_advanced() {
    let opened = Settings::default();
    let mut draft = opened.clone();
    draft.proxy.enabled = true;
    draft.proxy.kind = dm_core::ProxyKind::Http;
    draft.proxy.host = String::new();
    let refused = ok(&opened, &draft, &Typed::of(&draft), &opened).unwrap_err();
    assert_eq!(refused.tab, Some(Tab::Advanced));
    assert!(refused.to_proxy);
    draft.proxy.host = "http://proxy.example".into();
    assert!(ok(&opened, &draft, &Typed::of(&draft), &opened).is_err());
    draft.proxy.host = "proxy.example".into();
    draft.proxy.port = 3128;
    assert!(ok(&opened, &draft, &Typed::of(&draft), &opened).is_ok());
}

#[test]
fn a_category_that_cannot_be_a_folder_is_refused_on_folders() {
    let opened = Settings::default();
    let typed = Typed {
        custom: vec![["a/b".into(), "zip".into(), String::new()]],
        ..Typed::of(&opened)
    };
    let refused = ok(&opened, &opened, &typed, &opened).unwrap_err();
    assert_eq!(refused.tab, Some(Tab::SaveTo));
}

#[test]
fn restore_defaults_puts_back_one_page_alone() {
    let draft = Settings {
        connections: 3,
        speed_limit: 50,
        ..Settings::default()
    };
    let restored = restore_defaults(Tab::Downloads, &draft);
    assert_eq!(restored.connections, Settings::default().connections);
    assert_eq!(restored.speed_limit, 50, "Advanced's, not Downloads'");
}

#[test]
fn a_kinds_usual_port_follows_the_kind_and_a_typed_one_stays() {
    use dm_core::ProxyKind as Kind;
    assert_eq!(port_for_kind(Kind::Http, Kind::Socks5, 8080), 1080);
    assert_eq!(port_for_kind(Kind::Http, Kind::Socks5, 3128), 3128);
    assert_eq!(port_for_kind(Kind::System, Kind::Http, 0), 8080);
}
