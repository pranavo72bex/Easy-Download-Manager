// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// The captions a person wrote are named, on YouTube's own address as WebVTT; a
/// speech-recognised track only where there is none, and none from elsewhere (gaps row Y6).
#[test]
fn a_videos_written_captions_are_named_as_webvtt() {
    let player = json!({"captions": {"playerCaptionsTracklistRenderer": {"captionTracks": [
        {"languageCode": "en", "baseUrl": "https://www.youtube.com/api/timedtext?v=x&lang=en&fmt=srv3"},
        {"languageCode": "en", "kind": "asr", "baseUrl": "https://www.youtube.com/api/timedtext?v=x&lang=en&kind=asr"},
        {"languageCode": "de", "baseUrl": "/api/timedtext?v=x&lang=de"},
        {"languageCode": "fr", "baseUrl": "https://elsewhere.example/timedtext?lang=fr"}
    ]}}});
    let named = captions_named(&player);
    let mut languages: Vec<&String> = named.keys().collect();
    languages.sort();
    assert_eq!(languages, ["de", "en"]);
    let en = named["en"]["url"].as_str().unwrap();
    assert!(
        en.ends_with("&fmt=vtt") && !en.contains("srv3") && !en.contains("asr"),
        "{en}"
    );
    assert_eq!(named["de"]["ext"], "vtt");
    assert!(named["de"]["url"]
        .as_str()
        .unwrap()
        .starts_with("https://www.youtube.com/"));
    let spoken = json!({"captions": {"playerCaptionsTracklistRenderer": {"captionTracks": [
        {"languageCode": "en", "kind": "asr", "baseUrl": "https://www.youtube.com/api/timedtext?v=x&kind=asr"}
    ]}}});
    assert_eq!(captions_named(&spoken).len(), 1);
    assert!(captions_named(&json!({})).is_empty());
}

/// The languages YouTube translates a chosen track into are listed (not fetched), as WebVTT
/// with its `tlang`, none a chosen track is already in, at most 8 (gaps row Y6).
#[test]
fn translated_tracks_are_listed_from_the_first_translatable_chosen_track() {
    let languages: Vec<Value> = ["de", "en", "fr", "es", "it", "ja", "ko", "pt", "ru", "zh", "nl"]
        .iter()
        .map(|code| json!({"languageCode": code, "languageName": {"simpleText": format!("name {code}")}}))
        .chain([json!({"languageCode": "../x"}), json!({"languageName": {"simpleText": "none"}})])
        .collect();
    let player = json!({"captions": {"playerCaptionsTracklistRenderer": {
        "captionTracks": [
            {"languageCode": "en", "baseUrl": "https://www.youtube.com/api/timedtext?v=x&lang=en&fmt=srv3&tlang=xx", "isTranslatable": true},
            {"languageCode": "de", "kind": "asr", "baseUrl": "https://www.youtube.com/api/timedtext?v=x&lang=de&kind=asr", "isTranslatable": true}
        ],
        "translationLanguages": languages
    }}});
    let listed = translations_named(&player);
    let codes: Vec<&String> = listed.keys().collect();
    assert_eq!(codes.len(), 8, "{codes:?}");
    assert!(
        !listed.contains_key("en"),
        "the chosen track's own language"
    );
    assert!(
        listed.contains_key("de"),
        "the speech-recognised track was not chosen"
    );
    assert!(!listed.contains_key("../x"));
    let fr = listed["fr"]["url"].as_str().unwrap();
    assert_eq!(
        fr,
        "https://www.youtube.com/api/timedtext?v=x&lang=en&fmt=vtt&tlang=fr"
    );
    assert_eq!(listed["fr"]["ext"], "vtt");
    assert_eq!(listed["fr"]["name"], "name fr");
    // The reading's own tracks are untouched by them.
    assert_eq!(captions_named(&player).len(), 1);

    // Nothing listed where no chosen track can be translated, or none is offered.
    let fixed = json!({"captions": {"playerCaptionsTracklistRenderer": {
        "captionTracks": [{"languageCode": "en", "baseUrl": "https://www.youtube.com/api/timedtext?v=x"}],
        "translationLanguages": [{"languageCode": "fr"}]
    }}});
    assert!(translations_named(&fixed).is_empty());
    let elsewhere = json!({"captions": {"playerCaptionsTracklistRenderer": {
        "captionTracks": [{"languageCode": "en", "baseUrl": "https://elsewhere.example/t?v=x", "isTranslatable": true}],
        "translationLanguages": [{"languageCode": "fr"}]
    }}});
    assert!(translations_named(&elsewhere).is_empty());
    assert!(translations_named(&json!({})).is_empty());
}

#[test]
fn a_link_says_where_it_starts_in_each_of_youtubes_ways() {
    let at = |u: &str| start_of(u);
    assert_eq!(at("https://www.youtube.com/watch?v=abc&t=34"), Some(34));
    assert_eq!(at("https://youtu.be/abc?t=1m30s"), Some(90));
    assert_eq!(
        at("https://www.youtube.com/watch?v=abc#t=1h2m3s"),
        Some(3723)
    );
    assert_eq!(at("https://www.youtube.com/embed/abc?start=75"), Some(75));
    assert_eq!(at("https://www.youtube.com/watch?v=abc&t=0"), None);
    assert_eq!(at("https://www.youtube.com/watch?v=abc"), None);
    assert_eq!(at("https://www.youtube.com/watch?v=abc&t=soon"), None);
}

#[test]
fn a_video_id_is_read_from_every_address_shape() {
    for (url, id) in [
        (
            "https://www.youtube.com/watch?v=jNQXAC9IVRw",
            Some("jNQXAC9IVRw"),
        ),
        (
            "https://youtube.com/watch?t=5&v=jNQXAC9IVRw&list=x",
            Some("jNQXAC9IVRw"),
        ),
        ("https://youtu.be/jNQXAC9IVRw?t=3", Some("jNQXAC9IVRw")),
        (
            "https://m.youtube.com/shorts/jNQXAC9IVRw",
            Some("jNQXAC9IVRw"),
        ),
        (
            "https://www.youtube.com/embed/jNQXAC9IVRw",
            Some("jNQXAC9IVRw"),
        ),
        (
            "https://www.youtube-nocookie.com/embed/jNQXAC9IVRw",
            Some("jNQXAC9IVRw"),
        ),
        (
            "https://www.youtube.com/live/jNQXAC9IVRw",
            Some("jNQXAC9IVRw"),
        ),
        ("https://www.youtube.com/playlist?list=PL123", None),
        ("https://www.youtube.com/watch?v=short", None),
        ("https://example.com/watch?v=jNQXAC9IVRw", None),
        ("not an address", None),
    ] {
        assert_eq!(video_id(url).as_deref(), id, "{url}");
    }
}

#[test]
fn the_pages_facts_are_read_from_its_config() {
    let html = r#"<html><script>ytcfg.set({"CLIENT":"x"});ytcfg.set({"INNERTUBE_CONTEXT":{"client":{"visitorData":"Cgt2aXNpdG9y"}},"PLAYER_JS_URL":"/s/player/11a45ffb/player_ias.vflset/en_US/base.js","STS":20753,"TEXT":"a } brace in a string"});</script>"#;
    assert_eq!(
        page_facts(html),
        PageFacts {
            visitor: Some("Cgt2aXNpdG9y".into()),
            player_js: Some("/s/player/11a45ffb/player_ias.vflset/en_US/base.js".into()),
            sts: Some(20753),
        }
    );
    assert_eq!(page_facts("<html>nothing</html>"), PageFacts::default());
    assert_eq!(
        sts_in_player("var a={signatureTimestamp:20753,b:1}"),
        Some(20753)
    );
    assert_eq!(sts_in_player("var a={sts:123}"), None);
}

const PLAYER: &str = r#"{"playabilityStatus":{"status":"OK"},"videoDetails":{"title":"Me at the zoo","lengthSeconds":"19"},
  "streamingData":{"adaptiveFormats":[
    {"itag":137,"mimeType":"video/mp4; codecs=\"avc1.640028\"","width":1920,"height":1080,"fps":30,"bitrate":4000000,"contentLength":"8000","url":"https://r1.example/v?itag=137&n=aaa"},
    {"itag":248,"mimeType":"video/webm; codecs=\"vp9\"","width":1920,"height":1080,"bitrate":3000000,"contentLength":"3000","url":"https://r1.example/v?itag=248&n=aaa"},
    {"itag":136,"mimeType":"video/mp4; codecs=\"avc1.4d401f\"","width":1280,"height":720,"bitrate":2000000,"contentLength":"2000","signatureCipher":"s=ABCsig&sp=sig&url=https%3A%2F%2Fr1.example%2Fv%3Fitag%3D136%26n%3Dbbb"},
    {"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","averageBitrate":130000,"contentLength":"500","url":"https://r1.example/a?itag=140&n=aaa"},
    {"itag":251,"mimeType":"audio/webm; codecs=\"opus\"","averageBitrate":140000,"contentLength":"400","url":"https://r1.example/a?itag=251&n=ccc"},
    {"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","averageBitrate":130000,"contentLength":"500","isDrc":true,"url":"https://r1.example/a?itag=140&n=aaa&drc=1"},
    {"itag":22,"mimeType":"video/mp4; codecs=\"avc1\"","width":1280,"height":720,"bitrate":1,"drmFamilies":["w"],"url":"https://r1.example/drm"},
    {"itag":99,"mimeType":"video/mp4","width":1,"height":1,"bitrate":1,"type":"FORMAT_STREAM_TYPE_OTF","url":"https://r1.example/otf"}
  ]}}"#;

fn formats() -> Vec<Format> {
    let player: Value = serde_json::from_str(PLAYER).unwrap();
    player
        .pointer("/streamingData/adaptiveFormats")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .filter_map(Format::of)
        .collect()
}

#[test]
fn only_youtubes_own_player_script_is_fetched() {
    assert!(player_path(
        "/s/player/abcd1234/player_ias.vflset/en_US/base.js"
    ));
    assert!(!player_path("/s/player/../../evil.js"));
    assert!(!player_path("//evil.example/s/player/x/base.js"));
    assert!(!player_path("/elsewhere/base.js"));
    assert!(!player_path("/s/player/x/base.js?y=1"));
}

#[test]
fn a_playlist_address_is_told_from_a_watch_address_with_a_list() {
    assert_eq!(
        playlist_id("https://www.youtube.com/playlist?list=PLabc-_123"),
        Some("PLabc-_123".to_string())
    );
    assert_eq!(
        playlist_id("https://m.youtube.com/playlist?list=PL1&hl=en"),
        Some("PL1".into())
    );
    assert_eq!(
        playlist_id("https://www.youtube.com/watch?v=abc&list=PL1"),
        None
    );
    assert_eq!(playlist_id("https://www.youtube.com/playlist?list="), None);
    assert_eq!(
        playlist_id("https://www.youtube.com/playlist?list=a/b"),
        None
    );
    assert_eq!(playlist_id("https://example.com/playlist?list=PL1"), None);
}

#[test]
fn a_channels_address_and_its_tabs_are_told_from_other_pages() {
    let id = "UCBR8-60-B28hp2BmDPdntcQ";
    for (url, channel) in [
        (
            "https://www.youtube.com/@YouTube",
            Some(Channel::Page("/@YouTube".into())),
        ),
        (
            "https://m.youtube.com/@YouTube/videos?view=0",
            Some(Channel::Page("/@YouTube".into())),
        ),
        (
            "https://www.youtube.com/@YouTube/shorts/",
            Some(Channel::Page("/@YouTube".into())),
        ),
        (
            &format!("https://www.youtube.com/channel/{id}"),
            Some(Channel::Id(id.into())),
        ),
        (
            &format!("https://www.youtube.com/channel/{id}/streams"),
            Some(Channel::Id(id.into())),
        ),
        (
            "https://www.youtube.com/c/Makers/videos",
            Some(Channel::Page("/c/Makers".into())),
        ),
        (
            "https://www.youtube.com/user/old",
            Some(Channel::Page("/user/old".into())),
        ),
        // Not a channel's uploads: another tab, a video, a short channel id, another site.
        ("https://www.youtube.com/@YouTube/playlists", None),
        ("https://www.youtube.com/@YouTube/videos/more", None),
        ("https://www.youtube.com/channel/UCshort", None),
        ("https://www.youtube.com/watch?v=jNQXAC9IVRw", None),
        ("https://www.youtube.com/", None),
        ("https://www.youtube.com/@", None),
        ("https://example.com/@YouTube", None),
    ] {
        assert_eq!(channel_of(url), channel, "{url}");
    }
    assert!(is_list("https://music.youtube.com/playlist?list=PL1"));
    assert!(is_list("https://www.youtube.com/@YouTube"));
    assert!(!is_list(
        "https://www.youtube.com/watch?v=jNQXAC9IVRw&list=PL1"
    ));
    let page = format!(r#"<link rel="canonical" href="https://www.youtube.com/channel/{id}">"#);
    assert_eq!(channel_id_in(&page).as_deref(), Some(id));
    let data = format!(r#"{{"externalId":"{id}"}}"#);
    assert_eq!(channel_id_in(&data).as_deref(), Some(id));
    assert_eq!(channel_id_in(r#""externalId":"UCshort""#), None);
}

#[test]
fn a_playlist_pages_videos_and_its_next_page_are_gathered() {
    let html = r#"<html><script>ytcfg.set({"INNERTUBE_CLIENT_VERSION":"2.20261001.01.00"});</script>
    <script>var ytInitialData = {"metadata":{"playlistMetadataRenderer":{"title":"Northern Series"}},
    "contents":{"list":[{"playlistVideoRenderer":{"videoId":"vid1_______","title":{"runs":[{"text":"Northern Light"}]},"lengthSeconds":"634","index":{"simpleText":"1"}}},
    {"playlistVideoRenderer":{"videoId":"vid2_______","title":{"simpleText":"Still Water"}}},
    {"playlistVideoRenderer":{"videoId":"vid4_______","title":{"simpleText":"[Private video]"}}},
    {"playlistVideoRenderer":{"videoId":"vid5_______","isPlayable":false,"title":{"simpleText":"Gone"}}},
    {"playlistVideoRenderer":{"videoId":"abc&list=PL","title":{"simpleText":"Odd id"}}},
    {"playlistVideoRenderer":{"videoId":"vid1_______","title":{"runs":[{"text":"again"}]}}},
    {"lockupViewModel":{"contentId":"vid3_______","contentType":"LOCKUP_CONTENT_TYPE_VIDEO","contentImage":{"thumbnailViewModel":{"overlays":[{"thumbnailBottomOverlayViewModel":{"badges":[{"thumbnailBadgeViewModel":{"text":"12:15"}}]}}]}},"metadata":{"lockupMetadataViewModel":{"title":{"content":"Tears of Steel"}}}}},
    {"lockupViewModel":{"contentId":"PLx________","contentType":"LOCKUP_CONTENT_TYPE_PLAYLIST","metadata":{"lockupMetadataViewModel":{"title":{"content":"Another list"}}}}},
    {"continuationItemViewModel":{"continuationCommand":{"innertubeCommand":{"continuationCommand":{"token":"NEXT-1"}}}}}]}};</script></html>"#;
    let data = initial_data(html).unwrap();
    assert_eq!(playlist_title(&data).as_deref(), Some("Northern Series"));
    assert_eq!(client_version(html).as_deref(), Some("2.20261001.01.00"));
    let mut entries = Vec::new();
    let token = gather(&data, &mut entries);
    assert_eq!(token.as_deref(), Some("NEXT-1"));
    assert_eq!(
        entries,
        [
            PlaylistEntry {
                id: "vid1_______".into(),
                title: "Northern Light".into(),
                seconds: Some(634),
                page: None
            },
            PlaylistEntry {
                id: "vid2_______".into(),
                title: "Still Water".into(),
                seconds: None,
                page: None
            },
            PlaylistEntry {
                id: "vid3_______".into(),
                title: "Tears of Steel".into(),
                seconds: Some(735),
                page: None
            },
        ]
    );
    assert_eq!(
        entries[0].url(),
        "https://www.youtube.com/watch?v=vid1_______"
    );
    assert_eq!(initial_data("<html>nothing</html>"), None);
}

#[test]
fn the_title_comes_from_the_page_where_the_api_gives_none() {
    let html = r#"<html><title>Me &amp; the zoo - YouTube</title><script>var ytInitialPlayerResponse = {"videoDetails":{"title":"Me at the zoo"}};</script>"#;
    assert_eq!(page_title(html).as_deref(), Some("Me at the zoo"));
    let bare = "<html><head><title>Me &amp; the zoo - YouTube</title></head></html>";
    assert_eq!(page_title(bare).as_deref(), Some("Me & the zoo"));
    assert_eq!(page_title("<html></html>"), None);
}

#[test]
fn a_picture_listed_by_its_format_number_alone_is_read_from_the_table() {
    let f = |json: &str| Format::of(&serde_json::from_str::<Value>(json).unwrap()).unwrap();
    let bare = f(
        r#"{"itag":299,"mimeType":"video/mp4; codecs=\"avc1.64002a\"","url":"https://r1.example/v"}"#,
    );
    assert_eq!((bare.height, bare.fps), (1080, 60));
    let told = f(
        r#"{"itag":299,"mimeType":"video/mp4; codecs=\"avc1\"","width":1920,"height":1000,"fps":24,"url":"https://r1.example/v"}"#,
    );
    assert_eq!(
        (told.height, told.fps),
        (1000, 24),
        "what the answer says wins"
    );
    let sound = f(
        r#"{"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","url":"https://r1.example/a"}"#,
    );
    assert_eq!((sound.height, sound.fps), (0, 0));
}

/// The sound tracks as the player API gave them for a Spanish video with
/// an English automatic dub (2026-11-09): the dub marked as the one to
/// play, the original not, and the original in three copies (plain,
/// loudness-evened, voice-boosted) at one bitrate.
fn dubbed_video(with_xtags: bool) -> Vec<Format> {
    dubbed_video_wanting(with_xtags, None)
}

/// [`dubbed_video`] with a sound language asked for (gaps row Y10), and a described Spanish
/// track beside the original.
fn dubbed_video_wanting(with_xtags: bool, want: Option<&str>) -> Vec<Format> {
    let sound = |track: &str, name: &str, default: bool, tags: &str, extra: &str| {
        let address = if with_xtags {
            format!(
                "https://r1.example/a?itag=140&xtags={}",
                tags.replace(':', "%3A").replace('=', "%3D")
            )
        } else {
            format!(
                "https://r1.example/a?itag=140&t={}",
                tags.replace([':', '='], "-")
            )
        };
        format!(
            r#"{{"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","averageBitrate":129483,"audioTrack":{{"id":"{track}","displayName":"{name}","audioIsDefault":{default}}}{extra},"url":"{address}"}}"#
        )
    };
    let mut formats: Vec<Format> = [
        r#"{"itag":137,"mimeType":"video/mp4; codecs=\"avc1.640028\"","width":1920,"height":1080,"bitrate":4000000,"url":"https://r1.example/v?itag=137"}"#.to_string(),
        sound("en-US.10", "English (US)", true, "acont=dubbed-auto:lang=en-US", ""),
        sound("es-US.4", "Spanish (US) original", false, "acont=original:drc=1:lang=es-US", r#","isDrc":true"#),
        sound("es-US.4", "Spanish (US) original", false, "acont=original:lang=es-US", ""),
        sound("es-US.4", "Spanish (US) original", false, "acont=original:lang=es-US:vb=1", ""),
        sound("es-US.7", "Spanish (US) descriptive", false, "acont=descriptive:lang=es-US", ""),
        sound("fr-FR.10", "French (France)", false, "acont=dubbed-auto:lang=fr-FR", ""),
    ]
    .iter()
    .filter_map(|j| Format::of(&serde_json::from_str(j).unwrap()))
    .collect();
    set_dubs_aside(&mut formats, want);
    formats
}

fn sound_chosen(formats: &[Format], quality: Quality) -> String {
    choose(formats, quality, None, None)
        .unwrap()
        .into_iter()
        .find(|f| !f.video)
        .and_then(|f| f.url)
        .unwrap()
}

#[test]
fn a_dubbed_videos_own_language_is_chosen_not_the_dub_marked_to_play() {
    let formats = dubbed_video(true);
    for quality in [Quality::Best, Quality::UpTo(1080), Quality::Audio] {
        let chosen = sound_chosen(&formats, quality);
        assert!(
            chosen.contains("acont%3Doriginal%3Alang%3Des-US") && !chosen.contains("vb%3D1"),
            "{quality:?}: {chosen}"
        );
    }
}

#[test]
fn a_sound_language_asked_for_picks_its_dub_or_else_the_videos_own() {
    for with_xtags in [true, false] {
        let dub = sound_chosen(&dubbed_video_wanting(with_xtags, Some("fr")), Quality::Best);
        assert!(dub.contains("fr"), "{with_xtags}: {dub}");
        let english = sound_chosen(
            &dubbed_video_wanting(with_xtags, Some("en")),
            Quality::Audio,
        );
        assert!(
            english.contains("dubbed-auto") && english.contains("en-US"),
            "{english}"
        );
        // Spanish asked for: the original, not its described track or a copy set aside.
        let own = sound_chosen(&dubbed_video_wanting(with_xtags, Some("es")), Quality::Best);
        assert!(own.contains("original"), "{with_xtags}: {own}");
        // Only the address tells the voice-boosted copy (as with nothing asked).
        assert!(!with_xtags || !own.contains("vb"), "{own}");
        // No track speaks German: the video's own, as with nothing asked.
        let none = sound_chosen(&dubbed_video_wanting(with_xtags, Some("de")), Quality::Best);
        assert_eq!(none, sound_chosen(&dubbed_video(with_xtags), Quality::Best));
    }
}

#[test]
fn without_xtags_the_track_id_names_the_original() {
    let formats = dubbed_video(false);
    assert_eq!(
        formats.iter().filter(|f| !f.video && !f.secondary).count(),
        2,
        "the dub is set aside, the original's two copies kept (only the address named the boosted one)"
    );
    assert!(sound_chosen(&formats, Quality::Audio).contains("lang-es-US"));
    assert!(!sound_chosen(&formats, Quality::Audio).contains("dubbed"));
}

#[test]
fn where_no_original_is_named_the_track_marked_to_play_is_the_videos_own() {
    let mut formats: Vec<Format> = [
        r#"{"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","averageBitrate":130000,"audioTrack":{"id":"de.3","audioIsDefault":false},"url":"https://r1.example/a?d=de"}"#,
        r#"{"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","averageBitrate":120000,"audioTrack":{"id":"en.3","audioIsDefault":true},"url":"https://r1.example/a?d=en"}"#,
    ]
    .iter()
    .filter_map(|j| Format::of(&serde_json::from_str(j).unwrap()))
    .collect();
    set_dubs_aside(&mut formats, None);
    assert_eq!(
        sound_chosen(&formats, Quality::Audio),
        "https://r1.example/a?d=en"
    );
}

#[test]
fn tracks_none_of_which_is_the_videos_own_are_all_kept() {
    let mut formats: Vec<Format> = [
        r#"{"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","averageBitrate":130000,"audioTrack":{"id":"de.3","audioIsDefault":false},"url":"https://r1.example/a?d=de"}"#,
        r#"{"itag":251,"mimeType":"audio/webm; codecs=\"opus\"","averageBitrate":120000,"audioTrack":{"id":"fr.3","audioIsDefault":false},"url":"https://r1.example/a?d=fr"}"#,
    ]
    .iter()
    .filter_map(|j| Format::of(&serde_json::from_str(j).unwrap()))
    .collect();
    set_dubs_aside(&mut formats, None);
    assert!(
        formats.iter().all(|f| !f.secondary),
        "a video is not left without sound"
    );
}

#[test]
fn xtags_are_read_from_the_address_or_from_inside_its_cipher() {
    let pairs = |v: &[(&str, &str)]| {
        v.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        xtags(
            Some("https://r1.example/a?xtags=acont%3Doriginal%3Alang%3Des%3Avb%3D1&n=x"),
            None
        ),
        pairs(&[("acont", "original"), ("lang", "es"), ("vb", "1")])
    );
    let cipher =
        "s=abc&sp=sig&url=https%3A%2F%2Fr1.example%2Fa%3Fxtags%3Dacont%253Ddubbed%253Alang%253Den";
    assert_eq!(
        xtags(None, Some(cipher)),
        pairs(&[("acont", "dubbed"), ("lang", "en")])
    );
    assert!(xtags(Some("https://r1.example/a?itag=140"), None).is_empty());
    assert!(xtags(None, None).is_empty());
}

#[test]
fn locked_formats_are_left_out_and_the_rest_read() {
    let formats = formats();
    let itags: Vec<u64> = formats.iter().map(|f| f.itag).collect();
    // The one served only as numbered pieces is read too, marked as such (gaps row Y14).
    assert_eq!(itags, [137, 248, 136, 140, 251, 140, 99]);
    assert!(formats[6].numbered);
    assert!(formats.iter().take(6).all(|f| !f.numbered));
    assert!(formats[5].secondary, "the loudness variant is not chosen");
    assert_eq!(formats[3].ext(), "m4a");
    assert_eq!(formats[1].ext(), "webm");
}

#[test]
fn the_quality_asked_chooses_the_stream_it_names() {
    let formats = formats();
    let itags =
        |chosen: Option<Vec<Format>>| chosen.map(|c| c.iter().map(|f| f.itag).collect::<Vec<_>>());
    assert_eq!(
        itags(choose(&formats, Quality::Best, None, None)),
        Some(vec![137, 140])
    );
    assert_eq!(
        itags(choose(&formats, Quality::Best, Some("webm"), None)),
        Some(vec![248, 251])
    );
    assert_eq!(
        itags(choose(&formats, Quality::UpTo(720), None, None)),
        Some(vec![136, 140])
    );
    assert_eq!(
        itags(choose(&formats, Quality::Audio, None, None)),
        Some(vec![140])
    );
    assert_eq!(
        itags(choose(&formats, Quality::Audio, Some("webm"), None)),
        Some(vec![251])
    );
    assert_eq!(
        itags(choose(&formats, Quality::UpTo(100), None, None)),
        Some(vec![99, 140]),
        "the numbered pieces' picture, the only one that small"
    );
    assert_eq!(itags(choose(&formats, Quality::UpTo(0), None, None)), None);
}

#[test]
fn a_portrait_video_is_chosen_by_its_shorter_side() {
    // A Short: 720p is 720 wide and 1280 tall, as YouTube names it, not
    // the 405×720 picture that is 720 tall.
    let player: Value = serde_json::from_str(
        r#"[
        {"itag":301,"mimeType":"video/mp4; codecs=\"avc1.640028\"","width":1080,"height":1920,"bitrate":5000000,"url":"https://r1.example/v?itag=301&n=aaa"},
        {"itag":302,"mimeType":"video/mp4; codecs=\"avc1.4d401f\"","width":720,"height":1280,"bitrate":2500000,"url":"https://r1.example/v?itag=302&n=aaa"},
        {"itag":303,"mimeType":"video/mp4; codecs=\"avc1.4d401e\"","width":405,"height":720,"bitrate":1000000,"url":"https://r1.example/v?itag=303&n=aaa"},
        {"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","averageBitrate":130000,"url":"https://r1.example/a?itag=140&n=aaa"}
        ]"#,
    )
    .unwrap();
    let formats: Vec<Format> = player
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Format::of)
        .collect();
    let itags =
        |chosen: Option<Vec<Format>>| chosen.map(|c| c.iter().map(|f| f.itag).collect::<Vec<_>>());
    assert_eq!(
        itags(choose(&formats, Quality::UpTo(720), None, None)),
        Some(vec![302, 140])
    );
    assert_eq!(
        itags(choose(&formats, Quality::UpTo(1080), None, None)),
        Some(vec![301, 140])
    );
    assert_eq!(
        itags(choose(&formats, Quality::UpTo(480), None, None)),
        Some(vec![303, 140])
    );
    assert_eq!(
        itags(choose(&formats, Quality::Best, None, None)),
        Some(vec![301, 140])
    );
}

#[test]
fn an_address_is_unlocked_by_what_the_solver_answered() {
    let formats = formats();
    let solved = Solved {
        n: [
            ("aaa".to_string(), "AAA".to_string()),
            ("bbb".into(), "BBB".into()),
        ]
        .into_iter()
        .collect(),
        sig: [("ABCsig".to_string(), "SOLVEDSIG".to_string())]
            .into_iter()
            .collect(),
    };
    assert_eq!(
        unlocked(&formats[0], &solved).unwrap(),
        "https://r1.example/v?itag=137&n=AAA"
    );
    assert_eq!(
        unlocked(&formats[2], &solved).unwrap(),
        "https://r1.example/v?itag=136&n=BBB&sig=SOLVEDSIG"
    );
    // An `n` the solver did not answer stays as it was.
    assert_eq!(
        unlocked(&formats[4], &solved).unwrap(),
        "https://r1.example/a?itag=251&n=ccc"
    );
    assert!(has_n("https://r1.example/a?itag=251&n=ccc"));
    assert!(!has_n("https://r1.example/a?itag=251"));
}

/// A caption line is the track the page names, fetched as TTML into a file named after the
/// video and its language; a track elsewhere than YouTube is refused (gaps row 72c).
#[test]
fn a_caption_line_is_its_track_as_a_ttml_file() {
    let page = r#"<script>var ytInitialPlayerResponse = {"videoDetails":{"title":"A talk"},
      "captions":{"playerCaptionsTracklistRenderer":{"captionTracks":[
        {"baseUrl":"https://www.youtube.com/api/timedtext?v=abc&lang=hi&kind=asr&fmt=srv3","languageCode":"hi","kind":"asr"},
        {"baseUrl":"https://evil.example/t?lang=en","languageCode":"en"}]}}};</script>"#;
    let dir = std::path::Path::new("/staging");
    let reading: Value =
        serde_json::from_str(&caption_reading("abc", page, "hi", true, dir).unwrap()).unwrap();
    let url = reading["url"].as_str().unwrap();
    assert!(
        url.starts_with("https://www.youtube.com/api/timedtext?"),
        "{url}"
    );
    assert!(
        url.contains("lang=hi") && url.ends_with("fmt=ttml") && !url.contains("srv3"),
        "{url}"
    );
    assert_eq!(reading["ext"], "ttml");
    assert!(reading["filename"]
        .as_str()
        .unwrap()
        .ends_with("A talk [abc].hi.ttml"));
    assert!(
        caption_reading("abc", page, "en", false, dir).is_err(),
        "not YouTube's"
    );
    assert!(
        caption_reading("abc", page, "hi", false, dir).is_err(),
        "not the ASR one"
    );
}

/// The line's frame rate is the picture's: "720p 60 fps" gets the faster
/// picture, "720p" the other, and with no rate named the faster (gaps row 72d).
#[test]
fn the_line_picked_names_the_frame_rate_fetched() {
    let formats: Vec<Format> = [
        r#"{"itag":136,"mimeType":"video/mp4; codecs=\"avc1\"","width":1280,"height":720,"fps":30,"bitrate":1000,"url":"https://r1.example/v?itag=136"}"#,
        r#"{"itag":298,"mimeType":"video/mp4; codecs=\"avc1\"","width":1280,"height":720,"fps":60,"bitrate":2000,"url":"https://r1.example/v?itag=298"}"#,
        r#"{"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","bitrate":128000,"url":"https://r1.example/v?itag=140"}"#,
    ]
    .iter()
    .filter_map(|j| Format::of(&serde_json::from_str(j).unwrap()))
    .collect();
    let picture = |rate| {
        choose(&formats, Quality::UpTo(720), Some("mp4"), rate)
            .unwrap()
            .first()
            .map(|f| f.itag)
    };
    assert_eq!(picture(Some(60)), Some(298));
    assert_eq!(picture(Some(30)), Some(136));
    assert_eq!(picture(None), Some(298));
}

/// A list line's picture is taken by its format number where the reading has it, though the
/// height's rules would take another; where it has not, the line's height and frame rate, the
/// next lower, and with nothing that low the nearest above (gaps row Y9).
#[test]
fn the_line_picked_is_taken_by_its_format_number_else_the_nearest() {
    let formats: Vec<Format> = [
        r#"{"itag":136,"mimeType":"video/mp4; codecs=\"avc1\"","width":1280,"height":720,"fps":30,"bitrate":1000,"url":"https://r1.example/v?itag=136"}"#,
        r#"{"itag":398,"mimeType":"video/mp4; codecs=\"av01\"","width":1280,"height":720,"fps":30,"bitrate":3000,"url":"https://r1.example/v?itag=398"}"#,
        r#"{"itag":298,"mimeType":"video/mp4; codecs=\"avc1\"","width":1280,"height":720,"fps":60,"bitrate":2000,"url":"https://r1.example/v?itag=298"}"#,
        r#"{"itag":247,"mimeType":"video/webm; codecs=\"vp9\"","width":1280,"height":720,"fps":30,"bitrate":1500,"url":"https://r1.example/v?itag=247"}"#,
        r#"{"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","bitrate":128000,"url":"https://r1.example/v?itag=140"}"#,
        r#"{"itag":251,"mimeType":"audio/webm; codecs=\"opus\"","bitrate":140000,"url":"https://r1.example/v?itag=251"}"#,
    ]
    .iter()
    .filter_map(|j| Format::of(&serde_json::from_str(j).unwrap()))
    .collect();
    let took = |format, quality, container, rate| {
        choose_named(&formats, format, quality, container, rate)
            .map(|chosen| chosen.iter().map(|f| f.itag).collect::<Vec<_>>())
    };
    // The height's rules alone take the biggest bitrate at 720 and 30 frames a second.
    assert_eq!(
        took(None, Quality::UpTo(720), Some("mp4"), Some(30)),
        Some(vec![398, 140])
    );
    // The line's own picture, with the sound its container takes.
    assert_eq!(
        took(Some(136), Quality::UpTo(720), Some("mp4"), Some(30)),
        Some(vec![136, 140])
    );
    assert_eq!(
        took(Some(247), Quality::UpTo(720), Some("webm"), None),
        Some(vec![247, 251])
    );
    // A 1080p line whose picture this reading lacks: the next lower at its frame rate.
    assert_eq!(
        took(Some(137), Quality::UpTo(1080), Some("mp4"), Some(60)),
        Some(vec![298, 140])
    );
    assert_eq!(
        took(Some(137), Quality::UpTo(1080), Some("mp4"), None),
        Some(vec![298, 140])
    );
    // A 360p line with nothing that low: the nearest above, not no stream at all.
    assert_eq!(
        took(Some(134), Quality::UpTo(360), Some("mp4"), None),
        Some(vec![298, 140])
    );
    // Without a format number nothing that low is still nothing, as before.
    assert_eq!(took(None, Quality::UpTo(360), Some("mp4"), None), None);
    // The sound alone is never a picture's number.
    assert_eq!(
        took(Some(136), Quality::Audio, Some("m4a"), None),
        Some(vec![140])
    );
}

#[test]
fn the_reading_is_in_the_shape_the_downloader_reads() {
    let mut formats = formats();
    for f in &mut formats {
        f.url = Some("https://r1.example/x".into());
        f.cipher = None;
    }
    let chosen = choose(&formats, Quality::Best, None, None).unwrap();
    let reading = reading_json(
        "jNQXAC9IVRw",
        "Me at the zoo: a \"first\" / video",
        Some(19),
        &formats,
        &chosen,
        Path::new("/tmp/staging"),
    );
    // Joined as the system joins paths (a backslash on Windows).
    assert_eq!(
        reading["filename"],
        Path::new("/tmp/staging")
            .join("Me at the zoo_ a _first_ _ video [jNQXAC9IVRw].mp4")
            .display()
            .to_string()
    );
    let pair = reading["requested_formats"].as_array().unwrap();
    assert_eq!(pair.len(), 2);
    assert_eq!(pair[0]["ext"], "mp4");
    assert_eq!(pair[0]["protocol"], "https");
    assert_eq!(pair[0]["downloader_options"]["http_chunk_size"], 10_485_760);
    assert_eq!(pair[1]["ext"], "m4a");
    assert_eq!(pair[1]["vcodec"], "none");
    assert_eq!(reading["formats"].as_array().unwrap().len(), 7);
    // Numbered pieces are said to be so, with a size about from the bitrate and length.
    let numbered = &reading["formats"][6];
    assert_eq!(numbered["protocol"], "http_dash_segments");
    assert_eq!(numbered["filesize"], Value::Null);
    assert_eq!(
        numbered["filesize_approx"], 0,
        "1 bit a second for 19 seconds"
    );
    assert!(reading["formats"][0].get("filesize_approx").is_none());
    let sound = choose(&formats, Quality::Audio, Some("webm"), None).unwrap();
    let alone = reading_json("jNQXAC9IVRw", "x", None, &formats, &sound, Path::new("/s"));
    assert_eq!(alone["ext"], "webm");
    assert_eq!(alone["url"], "https://r1.example/x");
    assert!(alone.get("requested_formats").is_none());
    assert_eq!(safe_title(&"é".repeat(150)).len(), 200);
    assert_eq!(safe_title("  ... "), "video");
}

/// A client YouTube answers with anything but a video that plays is passed over for the
/// next, each asked as itself (its name, number and agent); the served ones come before
/// EDM's own (gaps row Y1).
#[tokio::test]
async fn the_next_client_is_asked_where_one_is_not_served() {
    use axum::routing::post;
    use std::sync::{Arc, Mutex};
    let asked: Arc<Mutex<Vec<(String, String, String)>>> = Arc::default();
    let seen = Arc::clone(&asked);
    let app = axum::Router::new().route(
        "/youtubei/v1/player",
        post(move |headers: axum::http::HeaderMap, body: String| {
            let seen = Arc::clone(&seen);
            async move {
                let body: Value = serde_json::from_str(&body).unwrap();
                let name = body["context"]["client"]["clientName"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let header = |n: &str| {
                    headers
                        .get(n)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_string()
                };
                seen.lock().unwrap().push((
                    name.clone(),
                    header("x-youtube-client-name"),
                    header("user-agent"),
                ));
                if name == "GOOD" {
                    r#"{"playabilityStatus":{"status":"OK"},"videoDetails":{"title":"t"}}"#
                } else {
                    r#"{"playabilityStatus":{"status":"UNPLAYABLE","reason":"not for this client"}}"#
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = |name: &str, id: &str| PlayerClient {
        name: name.into(),
        version: "1.0".into(),
        id: id.into(),
        agent: format!("{name} agent"),
        context: Vec::new(),
    };
    let http = crate::reqwest_defaults();
    let facts = PageFacts::default();
    let player = ask_player(
        &http,
        &origin,
        "abc",
        &facts,
        &[client("BROKEN", "1"), client("GOOD", "2")],
    )
    .await
    .expect("the second client answers");
    assert_eq!(player["videoDetails"]["title"], "t");
    assert_eq!(
        *asked.lock().unwrap(),
        vec![
            ("BROKEN".into(), "1".into(), "BROKEN agent".into()),
            ("GOOD".into(), "2".into(), "GOOD agent".into()),
        ]
    );
    // None serving: the last one's reason.
    let why = ask_player(&http, &origin, "abc", &facts, &[client("BROKEN", "1")])
        .await
        .unwrap_err();
    assert!(why.to_string().contains("not for this client"), "{why}");
    // The served clients come first, EDM's own after them.
    set_player_clients(vec![client("GOOD", "2")]);
    let names: Vec<String> = player_clients().into_iter().map(|c| c.name).collect();
    set_player_clients(Vec::new());
    assert_eq!(names, ["GOOD", TV_CLIENT_NAME]);
}

/// A signed-in request's `Authorization`, as YouTube's web clients make it: the time, and the
/// SHA-1 of "time SAPISID origin" in hex (gaps row Y4).
#[test]
fn a_sapisid_hash_is_the_time_and_its_sha1() {
    assert_eq!(
        sapisid_hash("sapisid-value", "https://www.youtube.com", 1_700_000_000),
        "SAPISIDHASH 1700000000_ade2239a1ec948ec8b27fe884ba5c1cdc9388057"
    );
    assert_ne!(
        sapisid_hash("sapisid-value", "https://music.youtube.com", 1_700_000_000),
        sapisid_hash("sapisid-value", "https://www.youtube.com", 1_700_000_000),
        "the origin is in the hash"
    );
}

#[test]
fn a_refusal_for_want_of_a_sign_in_is_told_from_others() {
    let said = |status: &str, reason: &str| {
        wants_sign_in(&json!({"playabilityStatus": {"status": status, "reason": reason}}))
    };
    assert!(said("LOGIN_REQUIRED", "Sign in to confirm your age"));
    assert!(said("LOGIN_REQUIRED", "This video is private"));
    assert!(said("AGE_CHECK_REQUIRED", ""));
    assert!(said("AGE_VERIFICATION_REQUIRED", ""));
    assert!(said(
        "UNPLAYABLE",
        "Join this channel to get access to members-only content like this video"
    ));
    assert!(said("ERROR", "This video is private."));
    assert!(!said("OK", ""));
    assert!(!said("UNPLAYABLE", "Video unavailable"));
    assert!(!said(
        "ERROR",
        "This video has been removed by the uploader"
    ));
    assert!(!wants_sign_in(&json!({})));
}

#[test]
fn a_session_is_one_that_names_a_sapisid() {
    let at = "https://www.youtube.com/watch?v=abc";
    let signed = crate::cookies::SignIn::new(at, "SID=s; SAPISID=sap; HSID=h").unwrap();
    let session = Session::for_player(Some(&signed), "https://www.youtube.com").unwrap();
    assert_eq!(session.sapisid, "sap");
    assert_eq!(
        session.cookie,
        "SID=s; SAPISID=sap; HSID=h; PREF=hl=en&tz=UTC; SOCS=CAI"
    );
    // The name newer sessions give it.
    let newer = crate::cookies::SignIn::new(at, "__Secure-3PAPISID=three; SID=s").unwrap();
    let session = Session::for_player(Some(&newer), "https://www.youtube.com").unwrap();
    assert_eq!(session.sapisid, "three");
    // None that can be signed with, and none for another host.
    let unsigned = crate::cookies::SignIn::new(at, "PREF=f6=1; SID=s").unwrap();
    assert!(Session::for_player(Some(&unsigned), "https://www.youtube.com").is_none());
    assert!(Session::for_player(Some(&signed), "https://m.youtube.com").is_none());
    assert!(Session::for_player(None, "https://www.youtube.com").is_none());
}

#[test]
fn a_signed_in_read_asks_the_tv_client_then_the_web_one_at_the_pages_version() {
    let page = r#"<script>ytcfg.set({"INNERTUBE_CLIENT_VERSION":"2.20261101.00.00"});</script>"#;
    let asked: Vec<(String, String, String)> = signed_in_clients(page)
        .into_iter()
        .map(|c| (c.name, c.version, c.id))
        .collect();
    assert_eq!(
        asked,
        [
            (
                TV_CLIENT_NAME.into(),
                TV_CLIENT_VERSION.into(),
                TV_CLIENT_ID.into()
            ),
            ("WEB".into(), "2.20261101.00.00".into(), "1".into()),
        ]
    );
    assert_eq!(
        signed_in_clients("<html></html>")[1].version,
        WEB_CLIENT_VERSION
    );
}

/// A stream that has just ended, as the player answers while YouTube makes the video of it
/// (gaps row Y5): its streams listed as a live one's, and its HLS playlist.
fn ended_stream(post_live: bool, live_content: bool, playlist: &str) -> Value {
    json!({
        "videoDetails": {
            "title": "Launch: the whole stream",
            "isLive": false,
            "isPostLiveDvr": post_live,
            "isLiveContent": live_content,
        },
        "streamingData": {
            "hlsManifestUrl": playlist,
            "adaptiveFormats": [{
                "itag": 137,
                "mimeType": "video/mp4; codecs=\"avc1.640028\"",
                "height": 1080,
                "targetDurationSec": 5,
                "url": "https://r1.example/v?itag=137",
            }],
        },
    })
}

#[test]
fn a_stream_just_ended_is_played_from_its_hls_playlist() {
    let playlist = "https://manifest.googlevideo.com/api/manifest/hls_variant/x/index.m3u8";
    let player = ended_stream(true, true, playlist);
    // None of its streams is read as a file: they are still a live one's.
    let read: Vec<Format> = player
        .pointer("/streamingData/adaptiveFormats")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Format::of)
        .collect();
    assert!(read.is_empty());
    assert_eq!(ended_stream_playlist(&player), Some(playlist));
    assert_eq!(
        ended_stream_playlist(&ended_stream(false, true, playlist)),
        Some(playlist),
        "a live video's that is no longer live"
    );
    // A video never live, and an address that is not YouTube's over https, are not.
    assert_eq!(
        ended_stream_playlist(&ended_stream(false, false, playlist)),
        None
    );
    assert_eq!(
        ended_stream_playlist(&ended_stream(true, true, "http://x.example/a.m3u8")),
        None
    );
    let staging = Path::new("staging");
    let answer: Value =
        serde_json::from_str(&played_from_playlist("abc", &player, playlist, staging)).unwrap();
    assert_eq!(
        answer["is_live"], true,
        "handed to the engine's HLS download"
    );
    assert_eq!(answer["url"], playlist);
    assert_eq!(answer["protocol"], "m3u8_native");
    let filename = answer["filename"].as_str().unwrap();
    assert!(filename.ends_with("[abc].mp4"), "{filename}");
}
