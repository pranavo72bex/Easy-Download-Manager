// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

#[test]
fn options_writes_only_what_it_changed() {
    let opened = Settings::default();
    // Options changed the connections.
    let mut draft = opened.clone();
    draft.downloads.connections = 4;
    // Meanwhile View › Language and the Scheduler changed others.
    let mut now = opened.clone();
    now.general.language = "de".into();
    now.schedule.enabled = true;
    let wanted = changed_only(&opened, &draft, &now).unwrap();
    assert_eq!(wanted.downloads.connections, 4, "Options' own change");
    assert_eq!(
        wanted.general.language, "de",
        "the language picked meanwhile"
    );
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
    assert_eq!(kept.browser.catch.extensions, ["zip", "pdf"]);
    assert_eq!(kept.browser.catch.skip_hosts, ["skip.example"]);
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
    draft.advanced.proxy.enabled = true;
    draft.advanced.proxy.kind = edm_core::ProxyKind::Http;
    draft.advanced.proxy.host = String::new();
    let refused = ok(&opened, &draft, &Typed::of(&draft), &opened).unwrap_err();
    assert_eq!(refused.tab, Some(Tab::Advanced));
    assert!(refused.to_proxy);
    draft.advanced.proxy.host = "http://proxy.example".into();
    assert!(ok(&opened, &draft, &Typed::of(&draft), &opened).is_err());
    draft.advanced.proxy.host = "proxy.example".into();
    draft.advanced.proxy.port = 3128;
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
        downloads: edm_shell::settings::DownloadSettings {
            connections: 3,
            ..Default::default()
        },
        advanced: edm_shell::settings::AdvancedSettings {
            speed_limit: 50,
            ..Default::default()
        },
        ..Settings::default()
    };
    let restored = restore_defaults(Tab::Downloads, &draft);
    assert_eq!(
        restored.downloads.connections,
        Settings::default().downloads.connections
    );
    assert_eq!(
        restored.advanced.speed_limit, 50,
        "Advanced's, not Downloads'"
    );
}

#[test]
fn a_kinds_usual_port_follows_the_kind_and_a_typed_one_stays() {
    use edm_core::ProxyKind as Kind;
    assert_eq!(port_for_kind(Kind::Http, Kind::Socks5, 8080), 1080);
    assert_eq!(port_for_kind(Kind::Http, Kind::Socks5, 3128), 3128);
    assert_eq!(port_for_kind(Kind::System, Kind::Http, 0), 8080);
}

/// The sound language's pop-up (gaps row Y10): the video's own first, each language after it,
/// and a value the list lacks at no place, so it is shown as itself and kept.
#[test]
fn a_sound_language_has_its_place_in_the_pop_up_and_back() {
    assert_eq!(sound_language_at(""), Some(0));
    assert_eq!(sound_language_of(0), Some(""));
    for (i, (tag, _)) in SOUND_LANGUAGES.iter().enumerate() {
        assert_eq!(sound_language_at(tag), Some(i + 1), "{tag}");
        assert_eq!(sound_language_of(i + 1), Some(*tag));
    }
    assert_eq!(sound_language_at("es"), Some(2));
    assert_eq!(sound_language_at("eo"), None);
    assert_eq!(sound_language_of(SOUND_LANGUAGES.len() + 1), None);
}

/// The metered connection's pop-up (gaps row W11): go on, ask and wait in turn, and a word
/// `settings.json` has that is none of them at go on's place.
#[test]
fn on_a_metered_connection_has_its_place_in_the_pop_up() {
    assert_eq!(ON_METERED, ["go", "ask", "wait"]);
    assert_eq!(on_metered_at("go"), 0);
    assert_eq!(on_metered_at("ask"), 1);
    assert_eq!(on_metered_at("wait"), 2);
    assert_eq!(on_metered_at("sometimes"), 0);
    assert!(tab_settings(Tab::Downloads).contains(&"on_metered"));
    assert!(tab_settings(Tab::Downloads).contains(&"battery_below"));
}
