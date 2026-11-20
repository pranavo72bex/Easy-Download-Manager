// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

#[test]
fn an_advertising_network_is_never_caught_whatever_it_serves() {
    for url in [
        "https://ad.doubleclick.net/ddm/trackclk/file.mp4",
        "https://pagead2.googlesyndication.com/pagead/ads/clip.mp4",
        "https://static.criteo.net/img/video.mp4",
        "https://cdn.example.com/adserver/promo.mp4",
    ] {
        assert_eq!(address(url), Some(Excluded::Advert), "{url}");
    }
}

#[test]
fn a_tracking_call_is_never_caught() {
    for url in [
        "https://www.google-analytics.com/collect.mp3",
        "https://stats.example.com/img/1x1.gif",
        "https://x.example.com/t/pixel.gif?id=3",
    ] {
        assert_eq!(address(url), Some(Excluded::Tracker), "{url}");
    }
}

#[test]
fn the_sounds_a_page_plays_for_itself_are_not_caught_but_a_song_is() {
    assert_eq!(
        address("https://site.example/assets/click.wav"),
        Some(Excluded::SiteSound)
    );
    assert_eq!(
        address("https://site.example/sounds/level-up.mid"),
        Some(Excluded::SiteSound)
    );
    // A recording named as music is a file a person may want.
    assert_eq!(address("https://site.example/music/track-01.wav"), None);
    assert_eq!(address("https://site.example/files/podcast.mp3"), None);
}

#[test]
fn a_pages_furniture_is_not_caught_but_a_picture_is() {
    assert_eq!(
        address("https://site.example/favicon.ico"),
        Some(Excluded::InterfaceImage)
    );
    assert_eq!(
        address("https://site.example/img/toolbar/save.png"),
        Some(Excluded::InterfaceImage)
    );
    assert_eq!(address("https://site.example/photos/holiday.jpg"), None);
}

#[test]
fn a_response_from_a_server_that_names_itself_an_ad_server_is_left() {
    assert_eq!(
        response(
            "https://a.example/x.mp4",
            "video/mp4",
            Some("DCLK-AdSvr"),
            Some(9_000_000)
        ),
        Some(Excluded::AdServer)
    );
    assert_eq!(
        response(
            "https://a.example/x.mp4",
            "video/mp4",
            Some("nginx"),
            Some(9_000_000)
        ),
        None
    );
}

#[test]
fn windows_media_of_100_kb_or_less_is_an_ad_stub_and_a_bigger_one_is_not() {
    assert_eq!(
        response(
            "https://a.example/promo.asf",
            "video/x-ms-asf",
            None,
            Some(100 * 1024)
        ),
        Some(Excluded::TinyWindowsMedia)
    );
    assert_eq!(
        response("https://a.example/clip", "audio/x-ms-wma", None, Some(2048)),
        Some(Excluded::TinyWindowsMedia)
    );
    assert_eq!(
        response(
            "https://a.example/film.wmv",
            "video/x-ms-wmv",
            None,
            Some(100 * 1024 + 1)
        ),
        None
    );
    // Size unknown: not excluded on size.
    assert_eq!(response("https://a.example/film.wmv", "", None, None), None);
    // Other formats are not held to the floor here (`STREAM_FLOOR` is).
    assert_eq!(
        response("https://a.example/tiny.mp4", "video/mp4", None, Some(2048)),
        None
    );
}

#[test]
fn an_ordinary_download_is_not_on_the_list() {
    for url in [
        "https://github.com/user/repo/releases/download/v1/setup.exe",
        "https://example.com/files/report.pdf",
        "https://cdn.example.com/video/talk.mp4",
    ] {
        assert_eq!(address(url), None, "{url}");
    }
}

fn served(json: &str) -> ExcludeRules {
    serde_json::from_str(json).expect("served rules")
}

#[test]
fn what_the_app_served_is_added_to_the_built_in_list() {
    let extra = served(
        r#"{ "advert_hosts": ["promo.example"], "tracker_hosts": ["stats.example"],
             "advert_segments": ["sponsor"], "tracker_files": ["hit.gif"],
             "sound_names": ["blip"], "sound_folders": ["jingles"], "sound_extensions": ["ogg"],
             "furniture_folders": ["chrome"], "furniture_names": ["divider"],
             "ad_server_names": ["adsrv"] }"#,
    );
    let cases = [
        ("https://cdn.promo.example/a/clip.mp4", Excluded::Advert),
        ("https://stats.example/c.mp3", Excluded::Tracker),
        ("https://site.example/sponsor/clip.mp4", Excluded::Advert),
        ("https://site.example/t/hit.gif", Excluded::Tracker),
        ("https://site.example/a/blip.ogg", Excluded::SiteSound),
        ("https://site.example/jingles/one.ogg", Excluded::SiteSound),
        (
            "https://site.example/chrome/a.png",
            Excluded::InterfaceImage,
        ),
        (
            "https://site.example/i/divider.gif",
            Excluded::InterfaceImage,
        ),
    ];
    for (url, why) in cases {
        assert_eq!(address_with(url, &extra), Some(why), "{url}");
        // Without what was served the same address is an ordinary one.
        assert_eq!(address_with(url, &ExcludeRules::default()), None, "{url}");
    }
    assert_eq!(
        response_with(
            "https://a.example/x.mp4",
            "video/mp4",
            Some("MyAdSrv/2"),
            None,
            &extra
        ),
        Some(Excluded::AdServer)
    );
    // The built-in list still holds when the served one is empty.
    assert_eq!(
        address_with("https://ad.doubleclick.net/x.mp4", &ExcludeRules::default()),
        Some(Excluded::Advert)
    );
}

#[test]
fn a_served_floor_raises_the_stub_size_and_never_lowers_it() {
    let raised = served(r#"{ "windows_media_floor": 300000 }"#);
    let lowered = served(r#"{ "windows_media_floor": 10 }"#);
    let url = "https://a.example/promo.wmv";
    let mime = "video/x-ms-wmv";
    assert_eq!(
        response_with(url, mime, None, Some(200_000), &raised),
        Some(Excluded::TinyWindowsMedia)
    );
    assert_eq!(
        response_with(url, mime, None, Some(200_000), &ExcludeRules::default()),
        None
    );
    assert_eq!(
        response_with(url, mime, None, Some(WINDOWS_MEDIA_FLOOR), &lowered),
        Some(Excluded::TinyWindowsMedia)
    );
}

#[test]
fn the_rules_installed_from_a_hello_are_the_ones_address_uses() {
    use super::super::rules;
    let list = dm_protocol::Rules {
        format: dm_protocol::RULES_FORMAT,
        revision: 2,
        exclude: served(r#"{ "advert_hosts": ["Promo.Example"] }"#),
        ..dm_protocol::Rules::default()
    };
    rules::install(Some(list));
    assert_eq!(
        address("https://promo.example/a.mp4"),
        Some(Excluded::Advert)
    );
    // A document in a format this side does not read is not used.
    rules::install(Some(dm_protocol::Rules {
        format: 99,
        ..dm_protocol::Rules::default()
    }));
    assert_eq!(address("https://promo.example/a.mp4"), None);
    rules::install(None);
    assert!(!rules::installed());
}

#[test]
fn the_smallest_player_is_the_one_the_notes_give() {
    assert_eq!(SMALLEST_PLAYER, (15.0, 10.0));
}

#[test]
fn os_update_servers_and_sticker_cdns_are_never_caught() {
    for url in [
        "https://update.microsoft.com/windowsupdate/v6/v5controls/en/x86/client/wuwebv.cab",
        "https://download.windowsupdate.com/c/msdownload/update/software/secu/2024/x.msu",
        "https://gstatic.com/a.mp4",
        "https://media.tenor.com/abc123/sticker.mp4",
        "https://media.tenor.co/abc123.mp4",
        "https://media.giphy.com/media/abc/giphy.mp4",
    ] {
        assert!(
            address(url).is_some(),
            "{url} should be excluded but was not"
        );
    }
}

#[test]
fn each_reason_is_said_in_its_own_words() {
    let all = [
        Excluded::Advert,
        Excluded::Tracker,
        Excluded::SiteSound,
        Excluded::InterfaceImage,
        Excluded::AdServer,
        Excluded::TinyWindowsMedia,
    ];
    let said: std::collections::HashSet<&str> = all.iter().map(|e| e.reason()).collect();
    assert_eq!(said.len(), all.len(), "two reasons said alike");
    assert!(said.iter().all(|s| !s.is_empty()));
    assert_eq!(Excluded::AdServer.reason(), "sent by an ad server");
}
