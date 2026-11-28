// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

#[test]
fn a_message_survives_being_framed_and_read_back() {
    let value = serde_json::json!({ "protocolVersion": 1, "type": "engine.hello" });
    let bytes = frame(&value);

    // Four bytes of little-endian length, then the body.
    assert_eq!(&bytes[..4], &(bytes.len() as u32 - 4).to_le_bytes());
    assert_eq!(read_frame(&mut &bytes[..]), Some(value));
}

/// Started by the Safari extension, the host opens the app it is the program of
/// (`launch_desktop`); a program outside a bundle is started as before.
#[test]
fn the_app_a_program_is_in_is_the_bundle_around_its_macos_folder() {
    assert_eq!(
        app_bundle(Path::new("/Applications/EDM.app/Contents/MacOS/edm-gpui")),
        Some(PathBuf::from("/Applications/EDM.app"))
    );
    for elsewhere in [
        "/usr/bin/edm-desktop",
        "/Users/x/target/debug/edm-gpui",
        "/Applications/EDM/Contents/MacOS/edm-gpui",
        "/Applications/EDM.app/Contents/Resources/edm-gpui",
    ] {
        assert_eq!(app_bundle(Path::new(elsewhere)), None, "{elsewhere}");
    }
}

/// Only a sandboxed host inside a bundle opens EDM through the system; else EDM is started.
#[test]
fn only_a_sandboxed_host_in_a_bundle_opens_edm_through_the_system() {
    let bundled = Path::new("/Applications/EDM.app/Contents/MacOS/edm-gpui");
    assert_eq!(
        opened_through_system(bundled, true),
        Some(PathBuf::from("/Applications/EDM.app"))
    );
    assert_eq!(opened_through_system(bundled, false), None);
    assert_eq!(
        opened_through_system(Path::new("/usr/bin/edm-desktop"), true),
        None
    );
}

#[test]
fn a_truncated_stream_ends_rather_than_blocking() {
    // The browser going away mid-message is how this normally ends.
    let bytes = frame(&serde_json::json!({ "type": "engine.hello" }));
    assert_eq!(read_frame(&mut &bytes[..6]), None);
}

#[test]
fn a_frame_that_is_not_json_does_not_end_the_conversation() {
    let mut bytes = 5u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(b"{oops");
    let next = serde_json::json!({ "protocolVersion": 1, "type": "engine.hello" });
    bytes.extend_from_slice(&frame(&next));

    let mut input = &bytes[..];
    assert_eq!(read_frame(&mut input), Some(serde_json::Value::Null));
    assert_eq!(read_frame(&mut input), Some(next));
}

#[test]
fn the_host_refuses_what_the_queue_would_drop() {
    for refused in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "data:text/plain,x",
    ] {
        assert!(!edm_core::accepted(refused), "{refused}");
    }
    assert!(edm_core::accepted("https://e.com/f.bin"));
    assert!(edm_core::accepted("ftps://e.com/f.bin"));
}

#[test]
fn an_implausible_length_is_refused_rather_than_allocated() {
    // A desynchronised stream otherwise asks for gigabytes.
    let mut bytes = u32::MAX.to_le_bytes().to_vec();
    bytes.extend_from_slice(b"{}");
    assert_eq!(read_frame(&mut &bytes[..]), None);
}

#[test]
fn a_request_lands_where_the_app_looks() {
    let dir = tempfile::tempdir().unwrap();
    let request = Request {
        url: "https://example.com/f.bin".into(),
        referrer: "https://example.com/page".into(),
        ..Request::default()
    };
    deposit(dir.path(), "abc", &request).unwrap();

    let file = inbox(dir.path()).join("abc.json");
    let read: Request = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    assert_eq!(read, request);
}

#[test]
fn a_browser_does_not_get_to_choose_where_a_request_is_written() {
    /* The id arrives in a message and becomes a filename. Anything that
     * writes where a message tells it to is a program that writes
     * wherever the sender likes — and `write_atomic` creates the parent,
     * so a traversal would make the directory as well as the file. */
    let dir = tempfile::tempdir().unwrap();
    let request = Request {
        url: "https://e.com/f.bin".into(),
        referrer: String::new(),
        ..Request::default()
    };

    for hostile in ["../../escaped", "..", "/etc/passwd", "a/b"] {
        let _ = deposit(dir.path(), hostile, &request);
        let escaped = dir.path().join("escaped.json");
        assert!(!escaped.exists(), "{hostile} wrote outside the inbox");
    }

    // Everything written stayed in the inbox, under a plain name.
    for entry in std::fs::read_dir(inbox(dir.path())).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(
            name.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)),
            "{name} is not a plain name"
        );
    }

    // An id with nothing usable in it is refused rather than guessed at.
    assert!(deposit(dir.path(), "../..", &request).is_err());
}

#[test]
fn two_hand_offs_never_share_one_file() {
    /* Ids that come to one name once made plain must not be written over
     * each other: the first download would be lost, both reported taken. */
    let dir = tempfile::tempdir().unwrap();
    let long = "X".repeat(64);
    let ids = ["a.1", "a1", "a1", &format!("{long}A"), &format!("{long}B")];
    for (n, id) in ids.iter().enumerate() {
        let request = Request {
            url: format!("https://e.com/{n}.zip"),
            ..Request::default()
        };
        deposit(dir.path(), id, &request).unwrap();
    }
    let mut urls: Vec<String> = std::fs::read_dir(inbox(dir.path()))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .map(|p| {
            serde_json::from_str::<Request>(&std::fs::read_to_string(p).unwrap())
                .unwrap()
                .url
        })
        .collect();
    urls.sort();
    assert_eq!(
        urls,
        (0..ids.len())
            .map(|n| format!("https://e.com/{n}.zip"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn the_same_hand_off_sent_twice_is_filed_once() {
    let dir = tempfile::tempdir().unwrap();
    let request = Request {
        url: "https://e.com/f.zip".into(),
        ..Request::default()
    };
    let first = deposit(dir.path(), "a1", &request).unwrap();
    let again = deposit(dir.path(), "a1", &request).unwrap();
    assert_eq!(first.path, again.path);
    assert!(first.new && !again.new);
    let filed = std::fs::read_dir(inbox(dir.path()))
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .count();
    assert_eq!(filed, 1, "one download, filed twice");
}

#[test]
fn no_half_written_request_is_left_for_the_app_to_read() {
    // The app polls a directory; a partial file there is a download
    // dropped without a word.
    let dir = tempfile::tempdir().unwrap();
    deposit(
        dir.path(),
        "abc",
        &Request {
            url: "https://e.com/f".into(),
            referrer: String::new(),
            ..Request::default()
        },
    )
    .unwrap();

    let leftovers: Vec<_> = std::fs::read_dir(inbox(dir.path()))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "a temp file was left behind");
}

/// A world with an app that is (or is not) running, a launcher that
/// records whether it was asked, and servers that answer EDM or do not.
struct Pretend {
    running: bool,
    launches: Result<(), ()>,
    launched: bool,
    refuses: Option<&'static str>,
    asked: Vec<(String, String)>,
    /// The cookies each probe was asked with.
    cookies_seen: Vec<String>,
    /// The type the server sends EDM.
    serves: &'static str,
    /// The time, as milliseconds since the epoch, and how far a probe
    /// moves it on.
    clock: u64,
    probe_takes_ms: u64,
    /// How long each probe was allowed.
    budgets: Vec<u64>,
}

impl World for Pretend {
    fn app_running(&mut self) -> bool {
        self.running
    }
    fn launch(&mut self) -> std::io::Result<()> {
        self.launched = true;
        self.launches
            .map_err(|()| std::io::Error::other("no such program"))
    }
    fn reachable(
        &mut self,
        url: &str,
        referrer: &str,
        cookies: &str,
        within_ms: u64,
    ) -> Result<String, String> {
        self.asked.push((url.into(), referrer.into()));
        self.cookies_seen.push(cookies.into());
        self.budgets.push(within_ms);
        self.clock += self.probe_takes_ms;
        self.refuses
            .map_or(Ok(self.serves.to_string()), |why| Err(why.into()))
    }
    fn now_ms(&mut self) -> u64 {
        self.clock
    }
    fn formats(
        &mut self,
        url: &str,
        _referrer: &str,
        _cookies: &str,
    ) -> Result<Vec<Choice>, String> {
        if url.contains("unreadable") {
            return Err("EDM could not read that page".into());
        }
        Ok(vec![
            Choice {
                label: "MP4 1280×720".into(),
                quality: "720".into(),
                size: Some(10),
            },
            Choice {
                label: "M4A audio".into(),
                quality: "audio".into(),
                size: None,
            },
        ])
    }
}

#[test]
fn the_toolbar_button_shows_the_app_and_downloads_nothing() {
    // Running: asked to show itself through the inbox, nothing filed.
    let dir = tempfile::tempdir().unwrap();
    let (reply, launched) = ask(dir.path(), Asked::Show.to_frame(), true, Ok(()));
    assert_eq!(reply, Some(Reply::Shown { reason: None }));
    assert!(inbox(dir.path()).join("show").exists());
    assert!(!launched);
    let filed = std::fs::read_dir(inbox(dir.path()))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
        .count();
    assert_eq!(filed, 0, "the toolbar button handed something over");

    // Not running: started, which opens the window.
    let dir = tempfile::tempdir().unwrap();
    let (reply, launched) = ask(dir.path(), Asked::Show.to_frame(), false, Ok(()));
    assert_eq!(reply, Some(Reply::Shown { reason: None }));
    assert!(launched);

    // Not running and cannot be started: said, for the browser to show.
    let dir = tempfile::tempdir().unwrap();
    let (reply, _) = ask(dir.path(), Asked::Show.to_frame(), false, Err(()));
    assert!(
        matches!(reply, Some(Reply::Shown { reason: Some(_) })),
        "{reply:?}"
    );
}

#[test]
fn a_pages_links_reach_the_app_together_to_be_picked_from() {
    let dir = tempfile::tempdir().unwrap();
    let frame = Asked::Batch(Batch {
        id: "links-1".into(),
        page: "https://e.com/list.html".into(),
        urls: vec![
            "https://e.com/a.zip".into(),
            "mailto:a@e.com".into(),
            "https://e.com/b.iso".into(),
        ],
        cookies: "sid=1".into(),
        texts: [
            (
                "https://e.com/a.zip".to_string(),
                "The A archive".to_string(),
            ),
            ("mailto:a@e.com".to_string(), "Write to us".to_string()),
        ]
        .into_iter()
        .collect(),
        names: [
            ("https://e.com/a.zip".to_string(), "A.zip".to_string()),
            ("mailto:a@e.com".to_string(), "us.txt".to_string()),
        ]
        .into_iter()
        .collect(),
    })
    .to_frame();
    let (reply, _) = ask(dir.path(), frame, true, Ok(()));
    assert_eq!(
        reply,
        Some(Reply::Started {
            id: "links-1".into(),
            shown: true,
            notice: None,
        })
    );
    let filed: Request = serde_json::from_str(
        &std::fs::read_to_string(inbox(dir.path()).join("links-1.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(filed.links, ["https://e.com/a.zip", "https://e.com/b.iso"]);
    assert_eq!(filed.referrer, "https://e.com/list.html");
    assert!(filed.ask);
    // The site's sign-in goes with them, for the dialog to scope.
    assert_eq!(filed.cookies, "sid=1");
    // And the links' words, of the links sent only.
    assert_eq!(
        filed.texts.into_iter().collect::<Vec<_>>(),
        [(
            "https://e.com/a.zip".to_string(),
            "The A archive".to_string()
        )]
    );
    // And the names their `download` attributes give, of the links sent only.
    assert_eq!(
        filed.names.into_iter().collect::<Vec<_>>(),
        [("https://e.com/a.zip".to_string(), "A.zip".to_string())]
    );

    let none = Asked::Batch(Batch {
        id: "links-2".into(),
        page: "https://e.com/".into(),
        urls: vec!["mailto:a@e.com".into()],
        ..Batch::default()
    })
    .to_frame();
    let (reply, _) = ask(dir.path(), none, true, Ok(()));
    assert!(matches!(reply, Some(Reply::Failed { .. })));
}

#[test]
fn the_panel_is_told_the_qualities_a_page_comes_in_or_why_not() {
    let dir = tempfile::tempdir().unwrap();
    let frame = |url: &str| {
        Asked::Formats(FormatsAsked {
            url: url.into(),
            referrer: String::new(),
            cookies: String::new(),
        })
        .to_frame()
    };
    let (reply, _) = ask(
        dir.path(),
        frame("https://video.example/watch?v=1"),
        true,
        Ok(()),
    );
    let Some(Reply::Formats { choices, reason }) = reply else {
        panic!("no formats answer");
    };
    assert_eq!(reason, None);
    assert_eq!(choices[0].quality, "720");
    assert_eq!(choices.len(), 2);

    let (reply, _) = ask(
        dir.path(),
        frame("https://unreadable.example/"),
        true,
        Ok(()),
    );
    let Some(Reply::Formats { choices, reason }) = reply else {
        panic!("no formats answer");
    };
    assert!(choices.is_empty());
    assert_eq!(reason.as_deref(), Some("EDM could not read that page"));
}

#[test]
fn a_sites_cookies_handed_over_reach_the_app_and_the_probe() {
    let dir = tempfile::tempdir().unwrap();
    let frame = Asked::Start(Start {
        id: "c1".into(),
        url: "https://files.example/private.zip".into(),
        referrer: "https://files.example/".into(),
        caught: true,
        cookies: "session=abc; theme=dark".into(),
        ..Start::default()
    })
    .to_frame();
    let mut world = Pretend {
        running: true,
        launches: Ok(()),
        launched: false,
        refuses: None,
        asked: Vec::new(),
        cookies_seen: Vec::new(),
        serves: "application/zip",
        clock: 0,
        probe_takes_ms: 0,
        budgets: Vec::new(),
    };
    let reply = answer(dir.path(), &frame, &mut world).map(|r| Reply::from_frame(&r).unwrap());
    assert_eq!(
        reply,
        Some(Reply::Started {
            id: "c1".into(),
            shown: true,
            notice: None,
        })
    );
    // The probe before taking it asked as the signed-in browser would.
    assert_eq!(world.cookies_seen, ["session=abc; theme=dark"]);
    let filed: Request =
        serde_json::from_str(&std::fs::read_to_string(inbox(dir.path()).join("c1.json")).unwrap())
            .unwrap();
    assert_eq!(filed.cookies, "session=abc; theme=dark");
    assert!(!filed.cookies_once, "a kept sign-in was filed as one-off");

    // A video page's session is filed as that download's alone.
    let frame = Asked::Start(Start {
        id: "c3".into(),
        url: "https://video.example/watch?v=1".into(),
        cookies: "SID=1".into(),
        cookies_once: true,
        ..Start::default()
    })
    .to_frame();
    answer(dir.path(), &frame, &mut world);
    let filed: Request =
        serde_json::from_str(&std::fs::read_to_string(inbox(dir.path()).join("c3.json")).unwrap())
            .unwrap();
    assert!(filed.cookies_once && filed.cookies == "SID=1", "{filed:?}");
    // A start without any hands none over.
    let (_, _) = ask(
        dir.path(),
        start("c2", "https://plain.example/f.zip"),
        true,
        Ok(()),
    );
    let plain: Request =
        serde_json::from_str(&std::fs::read_to_string(inbox(dir.path()).join("c2.json")).unwrap())
            .unwrap();
    assert!(plain.cookies.is_empty());
}

#[test]
fn a_video_from_the_panel_reaches_the_app_with_its_quality_and_asks_first() {
    let dir = tempfile::tempdir().unwrap();
    let frame = Asked::Start(Start {
        id: "panel-1".into(),
        url: "https://video.example/watch?v=1".into(),
        quality: Some("720".into()),
        label: Some("MP4 1280×720".into()),
        ask: true,
        ..Start::default()
    })
    .to_frame();
    let (reply, _) = ask(dir.path(), frame, true, Ok(()));
    assert_eq!(
        reply,
        Some(Reply::Started {
            id: "panel-1".into(),
            shown: true,
            notice: None,
        })
    );
    let filed: Request = serde_json::from_str(
        &std::fs::read_to_string(inbox(dir.path()).join("panel-1.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(filed.quality.as_deref(), Some("720"));
    assert_eq!(filed.label.as_deref(), Some("MP4 1280×720"));
    assert!(filed.ask);
}

#[test]
fn a_picture_and_the_sound_that_goes_with_it_reach_the_app_together() {
    let dir = tempfile::tempdir().unwrap();
    let frame = Asked::Start(Start {
        id: "pair-1".into(),
        url: "https://cdn.example/picture".into(),
        sound: Some(" https://cdn.example/sound ".into()),
        ..Start::default()
    })
    .to_frame();
    let (reply, _) = ask(dir.path(), frame, true, Ok(()));
    assert_eq!(
        reply,
        Some(Reply::Started {
            id: "pair-1".into(),
            shown: false,
            notice: None,
        })
    );
    let filed: Request = serde_json::from_str(
        &std::fs::read_to_string(inbox(dir.path()).join("pair-1.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(filed.sound.as_deref(), Some("https://cdn.example/sound"));
    let options = crate::shell::add_options(&filed);
    assert_eq!(options.sound.as_deref(), Some("https://cdn.example/sound"));
}

/// A part the browser panel's "Part" line asked for goes with a download no Download details asks
/// about (it was dropped, and the whole video fetched): an end alone starts at 0:00, as the dialog
/// takes it; a start alone or no span is the whole video.
#[test]
fn a_part_handed_over_goes_with_a_download_no_dialog_asks_about() {
    let with = |from: &str, to: &str| {
        crate::shell::add_options(&Request {
            url: "https://cdn.example/v.mp4".into(),
            from: from.into(),
            to: to.into(),
            ..Default::default()
        })
        .span
        .map(|s| (s.from, s.to))
    };
    assert_eq!(with("1:30", "2:00"), Some((90.0, 120.0)));
    assert_eq!(with("", "45"), Some((0.0, 45.0)));
    assert_eq!(with("1:30", ""), None);
    assert_eq!(with("2:00", "1:30"), None);
    assert_eq!(with("", ""), None);
}

fn ask(
    home: &Path,
    frame: serde_json::Value,
    running: bool,
    launches: Result<(), ()>,
) -> (Option<Reply>, bool) {
    let mut world = Pretend {
        running,
        launches,
        launched: false,
        refuses: None,
        asked: Vec::new(),
        cookies_seen: Vec::new(),
        serves: "application/zip",
        clock: 0,
        probe_takes_ms: 0,
        budgets: Vec::new(),
    };
    let reply = answer(home, &frame, &mut world);
    (
        reply.map(|r| Reply::from_frame(&r).unwrap()),
        world.launched,
    )
}

fn start(id: &str, url: &str) -> serde_json::Value {
    edm_protocol::Request::Start(Start {
        id: id.into(),
        url: url.into(),
        referrer: "https://e.com/page".into(),
        caught: true,
        ..Start::default()
    })
    .to_frame()
}

/// A browser's hand-off is filed as one, for a row waiting for a fresh address to take; a
/// renewal (row 121) never asks, is never probed, and never starts EDM.
#[test]
fn a_renewal_is_filed_for_a_running_edm_only_and_never_asks_or_launches() {
    let dir = tempfile::tempdir().unwrap();
    let (hello, _) = ask(
        dir.path(),
        edm_protocol::Request::Hello(Default::default()).to_frame(),
        true,
        Ok(()),
    );
    let Some(Reply::Hello(hello)) = hello else {
        panic!("no hello")
    };
    assert!(hello.renews, "the app does not say it takes renewals");

    let renewal = |id: &str| {
        edm_protocol::Request::Start(Start {
            id: id.into(),
            url: "https://cdn.example/v.mp4?sig=new".into(),
            referrer: "https://e.com/watch".into(),
            ask: true,
            renews: true,
            ..Start::default()
        })
        .to_frame()
    };
    let (reply, launched) = ask(dir.path(), renewal("renew-1"), false, Ok(()));
    assert!(matches!(reply, Some(Reply::Failed { .. })), "{reply:?}");
    assert!(!launched, "EDM was started for a renewal");
    assert!(!inbox(dir.path()).join("renew-1.json").exists());

    let (reply, launched) = ask(dir.path(), renewal("renew-2"), true, Ok(()));
    assert_eq!(
        reply,
        Some(Reply::Started {
            id: "renew-2".into(),
            shown: false,
            notice: None,
        })
    );
    assert!(!launched);
    let filed: Request = serde_json::from_str(
        &std::fs::read_to_string(inbox(dir.path()).join("renew-2.json")).unwrap(),
    )
    .unwrap();
    assert!(
        filed.renews && filed.from_browser && !filed.ask,
        "{filed:?}"
    );

    let (_, _) = ask(
        dir.path(),
        start("plain-1", "https://e.com/a.zip"),
        true,
        Ok(()),
    );
    let filed: Request = serde_json::from_str(
        &std::fs::read_to_string(inbox(dir.path()).join("plain-1.json")).unwrap(),
    )
    .unwrap();
    assert!(filed.from_browser && !filed.renews);
}

#[test]
fn a_hello_is_remembered_so_the_window_stops_offering_the_extension() {
    let home = tempfile::tempdir().unwrap();
    assert!(!browser_seen(home.path()));
    let _ = ask(
        home.path(),
        edm_protocol::Request::Hello(Default::default()).to_frame(),
        true,
        Ok(()),
    );
    assert!(browser_seen(home.path()));
}

#[test]
fn a_hello_says_what_this_app_is() {
    let dir = tempfile::tempdir().unwrap();
    let (reply, _) = ask(
        dir.path(),
        edm_protocol::Request::Hello(Default::default()).to_frame(),
        true,
        Ok(()),
    );
    let Some(Reply::Hello(hello)) = reply else {
        panic!("no hello")
    };
    assert_eq!(hello.engine_version, env!("CARGO_PKG_VERSION"));
    // Said, so the extension hands this app the downloads the browser
    // holds: it judges a sign-in page served in the file's place.
    assert!(hello.judges_pages());
    assert_eq!(hello.protocol, Some(edm_protocol::PROTOCOL_VERSION));
}

#[test]
fn a_hello_serves_the_site_rules_and_a_newer_file_in_the_home_replaces_them() {
    let dir = tempfile::tempdir().unwrap();
    let asked = |dir: &std::path::Path| {
        let (reply, _) = ask(
            dir,
            edm_protocol::Request::Hello(Default::default()).to_frame(),
            true,
            Ok(()),
        );
        let Some(Reply::Hello(hello)) = reply else {
            panic!("no hello")
        };
        hello.rules.expect("a hello carries the rules")
    };
    let shipped = asked(dir.path());
    assert_eq!(shipped.format, edm_protocol::RULES_FORMAT);
    std::fs::write(
        dir.path().join("rules.json"),
        format!(
            r#"{{ "format": 1, "revision": {}, "exclude": {{ "tracker_hosts": ["beacon.example"] }} }}"#,
            shipped.revision + 1
        ),
    )
    .unwrap();
    let newer = asked(dir.path());
    assert_eq!(newer.exclude.tracker_hosts, vec!["beacon.example"]);
    // The frame the extension reads names the field `rules`.
    let (reply, _) = ask(
        dir.path(),
        edm_protocol::Request::Hello(Default::default()).to_frame(),
        true,
        Ok(()),
    );
    let frame = reply.expect("a hello").to_frame();
    assert_eq!(frame["rules"]["revision"], shipped.revision + 1);
}

#[test]
fn a_caught_hand_off_at_the_new_version_is_answered_at_the_old() {
    // The extension sends a caught download at version 2; the answer
    // goes back at 1, which an older extension also reads.
    let dir = tempfile::tempdir().unwrap();
    let frame = caught("b9-1", "https://e.com/f.zip", "application/zip", None);
    assert_eq!(frame["protocolVersion"], 2);
    // A file a person asked for, checked but not held by the browser,
    // goes at 1, which an app of version 1 also answers.
    assert_eq!(start("m9-1", "https://e.com/f.zip")["protocolVersion"], 1);
    let mut world = pretend("application/zip", 0, 0);
    let reply = answer(dir.path(), &frame, &mut world).unwrap();
    assert_eq!(reply["protocolVersion"], 1);
    assert_eq!(reply["type"], "download.started");
}

#[test]
fn the_extension_is_sent_the_catch_rules_in_the_apps_settings() {
    let dir = tempfile::tempdir().unwrap();
    let asked = edm_protocol::Request::Settings.to_frame();

    // No settings file: the defaults the app would run with.
    let (reply, _) = ask(dir.path(), asked.clone(), true, Ok(()));
    assert_eq!(
        reply,
        Some(Reply::Settings(SettingsReply {
            rules: CatchRules::default()
        }))
    );

    std::fs::write(
        dir.path().join("settings.json"),
        r#"{"connections":4,"catch":{"enabled":true,"extensions":["bin"],"defaultsVersion":1,"skipHosts":["e.com"]}}"#,
    )
    .unwrap();
    let (reply, _) = ask(dir.path(), asked.clone(), true, Ok(()));
    let Some(Reply::Settings(SettingsReply { rules })) = reply else {
        panic!("no settings")
    };
    assert_eq!(rules.extensions, vec!["bin".to_string()]);
    assert_eq!(rules.skip_hosts, vec!["e.com".to_string()]);

    // A file the app could not read is left where it is: this process
    // has no window to say it moved anything.
    std::fs::write(dir.path().join("settings.json"), "{ oops").unwrap();
    let (reply, _) = ask(dir.path(), asked, true, Ok(()));
    assert_eq!(
        reply,
        Some(Reply::Settings(SettingsReply {
            rules: CatchRules::default()
        }))
    );
    assert!(dir.path().join("settings.json").exists());
}

#[test]
fn a_caught_download_asks_where_to_save_only_where_options_say() {
    let filed = |home: &Path, id: &str| -> Request {
        let (reply, _) = ask(home, start(id, "https://e.com/a.zip"), true, Ok(()));
        let request: Request = serde_json::from_str(
            &std::fs::read_to_string(inbox(home).join(format!("{id}.json"))).unwrap(),
        )
        .unwrap();
        // The reply says the app's window comes forward exactly where it asks (gaps row U11).
        assert_eq!(
            reply,
            Some(Reply::Started {
                id: id.into(),
                shown: request.ask,
                notice: None,
            })
        );
        request
    };
    // A new install asks first, as reference A's File Info does.
    let home = tempfile::tempdir().unwrap();
    let new = filed(home.path(), "c1");
    assert!(new.caught && new.ask);
    // So does a settings file from an older edition of the defaults.
    std::fs::write(home.path().join("settings.json"), r#"{"connections":4}"#).unwrap();
    let old = filed(home.path(), "c2");
    assert!(old.caught && old.ask);
    // A person who turned it off since keeps it off.
    std::fs::write(
        home.path().join("settings.json"),
        format!(
            r#"{{"ask_caught_downloads":false,"popups_version":{}}}"#,
            crate::settings::POPUPS_VERSION
        ),
    )
    .unwrap();
    assert!(!filed(home.path(), "c3").ask);
}

#[test]
fn a_hand_off_is_filed_and_taken_by_the_running_app() {
    let dir = tempfile::tempdir().unwrap();
    let (reply, launched) = ask(
        dir.path(),
        start("b1-2", "https://e.com/f.zip"),
        true,
        Ok(()),
    );
    assert_eq!(
        reply,
        Some(Reply::Started {
            id: "b1-2".into(),
            shown: true,
            notice: None,
        })
    );
    assert!(!launched, "a running app is not started twice");

    let filed: Request = serde_json::from_str(
        &std::fs::read_to_string(inbox(dir.path()).join("b1-2.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(filed.url, "https://e.com/f.zip");
    assert_eq!(filed.referrer, "https://e.com/page");
}

#[test]
fn with_no_app_running_one_is_started_and_a_failure_to_start_is_said() {
    let dir = tempfile::tempdir().unwrap();
    let (reply, launched) = ask(dir.path(), start("a", "https://e.com/f.zip"), false, Ok(()));
    assert!(launched);
    assert_eq!(
        reply,
        Some(Reply::Started {
            id: "a".into(),
            shown: true,
            notice: None,
        })
    );

    let (reply, _) = ask(
        dir.path(),
        start("b", "https://e.com/f.zip"),
        false,
        Err(()),
    );
    let Some(Reply::Failed { id, failure, .. }) = reply else {
        panic!("a launch that failed was reported as taken")
    };
    assert_eq!(id, "b");
    assert!(failure.unwrap().message.contains("could not be started"));
    // The browser keeps it, so nothing of it is left for an EDM started
    // later to fetch a second time: only "a" is filed.
    let left: Vec<_> = std::fs::read_dir(inbox(dir.path()))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".json"))
        .collect();
    assert_eq!(left, ["a.json"]);
}

#[test]
fn what_the_queue_would_drop_is_refused_by_name() {
    let dir = tempfile::tempdir().unwrap();
    // A .torrent on this computer is opened with EDM by the person, never
    // handed over by a page.
    for url in [
        "",
        "javascript:alert(1)",
        "file:///etc/passwd",
        "file:///home/a/x.torrent",
    ] {
        let (reply, launched) = ask(dir.path(), start("x", url), true, Ok(()));
        assert!(matches!(reply, Some(Reply::Failed { .. })), "{url}");
        assert!(!launched);
    }
    assert!(!inbox(dir.path()).join("x.json").exists());
}

#[test]
fn a_link_edm_cannot_fetch_itself_is_left_with_the_browser() {
    /* The browser has the site's cookies and EDM does not. Taken anyway,
     * the browser's copy is cancelled and EDM's fails: the download is
     * lost. Refused, the extension gives it back to the browser. */
    let dir = tempfile::tempdir().unwrap();
    let mut world = Pretend {
        running: true,
        launches: Ok(()),
        launched: false,
        refuses: Some("server returned 403 Forbidden"),
        asked: Vec::new(),
        cookies_seen: Vec::new(),
        serves: "application/zip",
        clock: 0,
        probe_takes_ms: 0,
        budgets: Vec::new(),
    };
    let reply = answer(
        dir.path(),
        &start("p", "https://e.com/private.zip"),
        &mut world,
    )
    .map(|r| Reply::from_frame(&r).unwrap());
    let Some(Reply::Failed { failure, .. }) = reply else {
        panic!("a link EDM cannot reach was taken")
    };
    let said = failure.unwrap().message;
    assert!(
        said.contains("without the browser") && said.contains("403"),
        "{said}"
    );
    assert_eq!(
        world.asked,
        vec![(
            "https://e.com/private.zip".into(),
            "https://e.com/page".into()
        )],
        "asked once, the way the queue will ask, referrer and all"
    );
    assert!(!inbox(dir.path()).join("p.json").exists(), "nothing filed");
}

/// A site the person said gives links that work once is not
/// asked about before EDM takes its download; another site still is.
#[test]
fn a_link_from_a_site_whose_links_work_once_is_taken_unasked() {
    let dir = tempfile::tempdir().unwrap();
    let mut settings = crate::Settings::default();
    settings.browser.catch.once_hosts = vec!["files.e.com".into()];
    std::fs::write(
        dir.path().join("settings.json"),
        serde_json::to_string(&settings).unwrap(),
    )
    .unwrap();
    let mut world = pretend("application/zip", 0, 0);
    world.refuses = Some("server returned 410 Gone");
    let reply = answer(
        dir.path(),
        &start("p", "https://files.e.com/once.zip"),
        &mut world,
    )
    .map(|r| Reply::from_frame(&r).unwrap());
    assert!(
        world.asked.is_empty(),
        "the server was asked: {:?}",
        world.asked
    );
    assert!(
        !matches!(reply, Some(Reply::Failed { .. })),
        "refused: {reply:?}"
    );
    assert!(inbox(dir.path()).join("p.json").exists(), "not filed");
    // Another site is still asked first.
    let mut world = pretend("application/zip", 0, 0);
    answer(
        dir.path(),
        &start("q", "https://other.org/f.zip"),
        &mut world,
    );
    assert_eq!(world.asked.len(), 1);
}

fn pretend(serves: &'static str, clock: u64, probe_takes_ms: u64) -> Pretend {
    Pretend {
        running: true,
        launches: Ok(()),
        launched: false,
        refuses: None,
        asked: Vec::new(),
        cookies_seen: Vec::new(),
        serves,
        clock,
        probe_takes_ms,
        budgets: Vec::new(),
    }
}

fn caught(id: &str, url: &str, mime: &str, deadline: Option<u64>) -> serde_json::Value {
    edm_protocol::Request::Start(Start {
        id: id.into(),
        url: url.into(),
        referrer: "https://e.com/page".into(),
        caught: true,
        held: true,
        mime: mime.into(),
        deadline,
        ..Start::default()
    })
    .to_frame()
}

fn refused(reply: Option<serde_json::Value>) -> String {
    match reply.map(|r| Reply::from_frame(&r).unwrap()) {
        Some(Reply::Failed { failure, .. }) => failure.unwrap().message,
        other => panic!("taken: {other:?}"),
    }
}

#[test]
fn a_sign_in_page_served_in_place_of_the_file_leaves_it_with_the_browser() {
    /* The site sends EDM, without the browser's cookies, a redirect to
     * its sign-in page: a 200 of HTML, which a check for errors alone
     * passed. Taken, the browser's copy was cancelled and EDM saved the
     * sign-in page. */
    let html = "text/html; charset=utf-8";
    let dir = tempfile::tempdir().unwrap();
    let mut world = pretend(html, 0, 0);
    let said = refused(answer(
        dir.path(),
        &caught("s1", "https://e.com/get?id=1", "application/zip", None),
        &mut world,
    ));
    assert!(
        said.contains("without the browser") && said.contains("text/html"),
        "{said}"
    );
    assert!(!inbox(dir.path()).join("s1.json").exists(), "nothing filed");

    // The browser did not say, but the address names a file.
    let said = refused(answer(
        dir.path(),
        &caught("s2", "https://e.com/files/report.pdf", "", None),
        &mut world,
    ));
    assert!(said.contains("rather than the file"), "{said}");
    assert!(!inbox(dir.path()).join("s2.json").exists());

    // A page the browser was saving as a page is the same page to EDM.
    let reply = answer(
        dir.path(),
        &caught("s3", "https://e.com/article", "text/html", None),
        &mut world,
    )
    .map(|r| Reply::from_frame(&r).unwrap());
    assert_eq!(
        reply,
        Some(Reply::Started {
            id: "s3".into(),
            shown: true,
            notice: None,
        })
    );

    // And the file itself, served as one, is taken.
    let mut world = pretend("application/zip", 0, 0);
    let reply = answer(
        dir.path(),
        &caught("s4", "https://e.com/get?id=1", "application/zip", None),
        &mut world,
    )
    .map(|r| Reply::from_frame(&r).unwrap());
    assert_eq!(
        reply,
        Some(Reply::Started {
            id: "s4".into(),
            shown: true,
            notice: None,
        })
    );
}

#[test]
fn a_hand_off_the_extension_has_stopped_waiting_for_is_not_filed() {
    /* The extension waits so long for an answer and then gives the
     * download back to the browser. A host that took it after that had
     * the file downloaded twice: by the browser and by EDM. */
    let dir = tempfile::tempdir().unwrap();
    let now = 1_700_000_000_000;

    // Already too late on arrival: not asked about, not filed.
    let mut world = pretend("application/zip", now, 0);
    let said = refused(answer(
        dir.path(),
        &caught("t1", "https://e.com/f.zip", "", Some(now + 500)),
        &mut world,
    ));
    assert!(said.contains("stopped waiting"), "{said}");
    assert!(world.asked.is_empty(), "the server was asked anyway");
    assert!(!inbox(dir.path()).join("t1.json").exists());

    // The probe is given what is left, short of the margin for answering,
    // and never more than its own limit.
    let mut world = pretend("application/zip", now, 0);
    let reply = answer(
        dir.path(),
        &caught("t2", "https://e.com/f.zip", "", Some(now + 5_000)),
        &mut world,
    )
    .map(|r| Reply::from_frame(&r).unwrap());
    assert_eq!(
        reply,
        Some(Reply::Started {
            id: "t2".into(),
            shown: true,
            notice: None,
        })
    );
    assert_eq!(world.budgets, [5_000 - edm_protocol::ANSWER_MARGIN_MS]);
    let mut world = pretend("application/zip", now, 0);
    let _ = answer(
        dir.path(),
        &caught("t3", "https://e.com/f.zip", "", Some(now + 60_000)),
        &mut world,
    );
    assert_eq!(world.budgets, [edm_protocol::PROBE_WITHIN_MS]);

    // A server slow enough to use it all: too late once it answers.
    let mut world = pretend("application/zip", now, 4_000);
    let said = refused(answer(
        dir.path(),
        &caught("t4", "https://e.com/f.zip", "", Some(now + 5_000)),
        &mut world,
    ));
    assert!(said.contains("stopped waiting"), "{said}");
    assert!(!inbox(dir.path()).join("t4.json").exists());

    // A link asked for from a menu is not filed late either.
    let mut world = pretend("application/zip", now, 0);
    let late = edm_protocol::Request::Start(Start {
        id: "t5".into(),
        url: "https://e.com/watch".into(),
        deadline: Some(now - 1),
        ..Start::default()
    })
    .to_frame();
    let _ = refused(answer(dir.path(), &late, &mut world));
    assert!(!inbox(dir.path()).join("t5.json").exists());
}

#[test]
fn a_link_handed_over_on_request_is_taken_without_asking_the_server() {
    // The browser holds nothing to give back, and asking the server now
    // would contact it outside a schedule the app would have kept to.
    let dir = tempfile::tempdir().unwrap();
    let mut world = Pretend {
        running: true,
        launches: Ok(()),
        launched: false,
        refuses: Some("unreachable"),
        asked: Vec::new(),
        cookies_seen: Vec::new(),
        serves: "application/zip",
        clock: 0,
        probe_takes_ms: 0,
        budgets: Vec::new(),
    };
    let asked = edm_protocol::Request::Start(Start {
        id: "m1".into(),
        url: "https://e.com/f.zip".into(),
        ..Default::default()
    })
    .to_frame();
    let reply = answer(dir.path(), &asked, &mut world).map(|r| Reply::from_frame(&r).unwrap());
    assert_eq!(
        reply,
        Some(Reply::Started {
            id: "m1".into(),
            shown: false,
            notice: None,
        })
    );
    assert!(world.asked.is_empty(), "the server was asked");
    assert!(inbox(dir.path()).join("m1.json").exists());
}

#[test]
fn a_proxy_that_cannot_be_used_is_not_gone_around_to_ask_the_server() {
    /* The server would see the address the person set a proxy to hide:
     * the check refuses, and the browser keeps the download. A server
     * on loopback counts every request it gets. */
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("settings.json"),
        serde_json::json!({
            "proxy": { "enabled": true, "type": "http", "host": "", "port": 8080 }
        })
        .to_string(),
    )
    .unwrap();
    let mut here = Here {
        home: dir.path(),
        started: false,
    };
    let asked = here.reachable(&format!("http://{addr}/f.zip"), "", "", 2000);
    assert!(asked.is_err(), "{asked:?}");
    let mut contacted = false;
    if let Ok((mut stream, _)) = listener.accept() {
        contacted = true;
        let mut buf = [0u8; 64];
        let _ = stream.read(&mut buf);
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n");
    }
    assert!(
        !contacted,
        "the server was asked directly, around the proxy"
    );
}

#[test]
fn a_magnet_is_not_probed_as_though_it_had_a_server() {
    let dir = tempfile::tempdir().unwrap();
    let mut here = Here {
        home: dir.path(),
        started: false,
    };
    assert_eq!(
        here.reachable("magnet:?xt=urn:btih:abc", "", "", 1000),
        Ok(String::new())
    );
}

#[test]
fn a_frame_this_host_does_not_answer_gets_nothing() {
    let dir = tempfile::tempdir().unwrap();
    for frame in [
        serde_json::json!({ "protocolVersion": 1, "type": "download.pause", "id": "a" }),
        serde_json::json!({ "protocolVersion": 3, "type": "engine.hello" }),
        serde_json::json!({ "protocolVersion": 0, "type": "engine.hello" }),
        serde_json::Value::Null,
    ] {
        assert_eq!(ask(dir.path(), frame, true, Ok(())).0, None);
    }
}

fn choice(label: &str) -> Choice {
    Choice {
        label: label.into(),
        quality: "best".into(),
        size: Some(1),
    }
}

#[test]
fn a_page_read_a_moment_ago_is_not_read_again_and_an_old_reading_is_not_believed() {
    let home = tempfile::tempdir().unwrap();
    let key = formats_key("https://v.example/watch?v=1#t=5", "sid=1");
    assert!(recalled_formats(home.path(), &key, 1000).is_none());
    remember_formats(home.path(), &key, &[choice("MP4 720")], 1000);
    let got = recalled_formats(home.path(), &key, 1000 + FORMATS_KEPT_SECS - 1).unwrap();
    assert_eq!(got[0].label, "MP4 720");
    assert!(recalled_formats(home.path(), &key, 1000 + FORMATS_KEPT_SECS).is_none());
    // A site restamping a cookie's value does not make it another session;
    // other cookies do.
    assert_eq!(key, formats_key("https://v.example/watch?v=1", "sid=9"));
    let other = formats_key("https://v.example/watch?v=1", "sid=2; login=1");
    assert!(recalled_formats(home.path(), &other, 1001).is_none());
    // The fragment is not part of the page.
    assert_eq!(key, formats_key("https://v.example/watch?v=1", "sid=1"));
    // An empty reading is never kept.
    remember_formats(home.path(), &other, &[], 1002);
    assert!(recalled_formats(home.path(), &other, 1003).is_none());
    // Only the latest pages are kept.
    for n in 0..FORMATS_KEPT_PAGES + 5 {
        let k = formats_key(&format!("https://v.example/{n}"), "");
        remember_formats(home.path(), &k, &[choice("x")], 2000);
    }
    let first = formats_key("https://v.example/0", "");
    assert!(recalled_formats(home.path(), &first, 2001).is_none());
    let last = formats_key(&format!("https://v.example/{}", FORMATS_KEPT_PAGES + 4), "");
    assert!(recalled_formats(home.path(), &last, 2001).is_some());
}

/// The browser asks for the rules without the app: a list saved before
/// documents joined the defaults reaches it with them, and one that is then
/// only the old defaults reaches it as every type.
#[test]
fn the_rules_the_browser_asks_for_have_the_new_default_types() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join("settings.json"),
        r#"{"catch": {"extensions": ["zip", "xyz"]}}"#,
    )
    .unwrap();
    let rules = catch_rules(home.path());
    assert!(rules.extensions.iter().any(|k| k == "docx"));
    assert!(rules.extensions.iter().any(|k| k == "xyz"));
    std::fs::write(
        home.path().join("settings.json"),
        r#"{"catch": {"extensions": ["zip"]}}"#,
    )
    .unwrap();
    assert!(catch_rules(home.path()).extensions.is_empty());
}

#[cfg(unix)]
#[test]
fn a_folder_edm_may_not_write_to_is_said_before_a_download_is_taken() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        unwritable(&dir.path().join("made/now")),
        None,
        "a missing folder is made"
    );
    let locked = dir.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    // As root (a container's test run) no folder is locked: nothing to see.
    let probe = locked.join(".probe");
    if std::fs::File::create(&probe).is_ok() {
        let _ = std::fs::remove_file(&probe);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    let said = unwritable(&locked);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    let said = said.expect("refused");
    assert!(said.contains("the browser keeps the download"), "{said}");
    assert!(
        std::fs::read_dir(&locked).unwrap().next().is_none(),
        "nothing left behind"
    );
}

#[test]
fn each_browser_that_said_hello_is_noted_once() {
    let home = tempfile::tempdir().unwrap();
    assert!(browsers_seen(home.path()).is_empty());
    assert!(!browser_seen(home.path()));
    // One that cannot be named still counts as a browser seen.
    note_browser(home.path(), None);
    assert!(browser_seen(home.path()));
    assert!(browsers_seen(home.path()).is_empty());
    note_browser(home.path(), Some("Microsoft Edge"));
    note_browser(home.path(), Some("Firefox"));
    note_browser(home.path(), Some("Microsoft Edge"));
    assert_eq!(browsers_seen(home.path()), ["Microsoft Edge", "Firefox"]);
}

/// Each browser is kept with when it was last heard from, written again a minute on; a file
/// from before, of names alone, still reads (gaps row U24).
#[test]
fn each_browser_is_kept_with_when_it_was_last_heard() {
    let home = tempfile::tempdir().unwrap();
    note_browser_at(home.path(), Some("Firefox"), 1_000);
    note_browser_at(home.path(), Some("Firefox"), 1_030);
    assert_eq!(
        browsers_heard(home.path()),
        [("Firefox".to_string(), Some(1_000))],
        "written again within the minute"
    );
    note_browser_at(home.path(), Some("Firefox"), 1_100);
    note_browser_at(home.path(), None, 1_100);
    assert_eq!(
        browsers_heard(home.path()),
        [("Firefox".to_string(), Some(1_100))]
    );
    std::fs::write(home.path().join("browser.seen"), "Microsoft Edge\n").unwrap();
    assert_eq!(
        browsers_heard(home.path()),
        [("Microsoft Edge".to_string(), None)]
    );
    assert_eq!(browsers_seen(home.path()), ["Microsoft Edge"]);
}

#[test]
fn the_part_asked_for_in_the_panel_reaches_the_app_and_a_caught_download_has_none() {
    let dir = tempfile::tempdir().unwrap();
    let frame = Asked::Start(Start {
        id: "part-1".into(),
        url: "https://video.example/stream.m3u8".into(),
        ask: true,
        from: " 1:30 ".into(),
        to: "2:00".into(),
        ..Start::default()
    })
    .to_frame();
    let (_, _) = ask(dir.path(), frame, true, Ok(()));
    let filed: Request = serde_json::from_str(
        &std::fs::read_to_string(inbox(dir.path()).join("part-1.json")).unwrap(),
    )
    .unwrap();
    assert_eq!((filed.from.as_str(), filed.to.as_str()), ("1:30", "2:00"));
    // None asked for: nothing is written, and an older file reads as none.
    let frame = Asked::Start(Start {
        id: "part-2".into(),
        url: "https://video.example/b.mp4".into(),
        ask: true,
        ..Start::default()
    })
    .to_frame();
    let (_, _) = ask(dir.path(), frame, true, Ok(()));
    let text = std::fs::read_to_string(inbox(dir.path()).join("part-2.json")).unwrap();
    assert!(
        !text.contains("\"from\"") && !text.contains("\"to\""),
        "{text}"
    );
    let old: Request = serde_json::from_str(r#"{"url":"https://e.com/a"}"#).unwrap();
    assert!(old.from.is_empty() && old.to.is_empty());
}

/// A caught download that is a form's answer is taken without asking its
/// server first (asking would send the form again), and the form is filed
/// with it for EDM to send once.
#[test]
fn a_forms_answer_is_taken_without_asking_and_its_form_goes_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let frame = Asked::Start(Start {
        id: "f1".into(),
        url: "https://files.example/export".into(),
        referrer: "https://files.example/report".into(),
        caught: true,
        post: Some(edm_protocol::Posted {
            form: "report=7&format=csv".into(),
            ..edm_protocol::Posted::default()
        }),
        ..Start::default()
    })
    .to_frame();
    let mut world = Pretend {
        running: true,
        launches: Ok(()),
        launched: false,
        refuses: None,
        asked: Vec::new(),
        cookies_seen: Vec::new(),
        serves: "text/csv",
        clock: 0,
        probe_takes_ms: 0,
        budgets: Vec::new(),
    };
    let reply = answer(dir.path(), &frame, &mut world).map(|r| Reply::from_frame(&r).unwrap());
    assert_eq!(
        reply,
        Some(Reply::Started {
            id: "f1".into(),
            shown: true,
            notice: None,
        })
    );
    assert!(
        world.asked.is_empty(),
        "the form's address was asked: {:?}",
        world.asked
    );
    let filed: Request =
        serde_json::from_str(&std::fs::read_to_string(inbox(dir.path()).join("f1.json")).unwrap())
            .unwrap();
    let form = crate::shell::posted(&filed.url, filed.post.as_ref()).expect("the form was filed");
    assert_eq!(form.body, b"report=7&format=csv");
    assert_eq!(form.content_type, "application/x-www-form-urlencoded");
    assert_eq!(form.url, "https://files.example/export");
    // A raw body arrives as base64, with the type the browser's request named.
    let raw = edm_protocol::Posted {
        raw: "eyJhIjoxfQ==".into(),
        content_type: "application/json".into(),
        ..edm_protocol::Posted::default()
    };
    let form = crate::shell::posted("https://e.com/api", Some(&raw)).unwrap();
    assert_eq!(form.body, br#"{"a":1}"#);
    assert_eq!(form.content_type, "application/json");
    // A body that is not base64 is no form to send.
    let bad = edm_protocol::Posted {
        raw: "not base64!".into(),
        ..edm_protocol::Posted::default()
    };
    assert!(crate::shell::posted("https://e.com/api", Some(&bad)).is_none());
}

/// A page's links are taken without logins written into them, and none inside this computer or
/// its network where the page is public; an inside page's own inside links are kept (security
/// audit).
#[test]
fn a_public_pages_links_inside_are_dropped_and_logins_taken_out() {
    let page = "https://evil.example/list.html";
    assert_eq!(page_link(page, "http://192.168.1.1/apply.cgi?x=1"), None);
    let (user, password) = (
        crate::test_secrets::secret("router-user"),
        crate::test_secrets::secret("router-password"),
    );
    assert_eq!(
        page_link(page, &format!("http://{user}:{password}@192.168.1.1/a")),
        None
    );
    assert_eq!(page_link(page, "http://169.254.169.254/latest/"), None);
    assert_eq!(page_link(page, "file:///etc/passwd"), None);
    assert_eq!(
        page_link(
            page,
            &format!(" https://{user}:{password}@files.example/a.zip ")
        )
        .as_deref(),
        Some("https://files.example/a.zip")
    );
    assert_eq!(
        page_link(page, "https://files.example/b.zip").as_deref(),
        Some("https://files.example/b.zip")
    );
    assert_eq!(
        page_link("http://nas.local/", "http://nas.local/c.iso").as_deref(),
        Some("http://nas.local/c.iso")
    );
}

/// A released extension keeps working with this app: every frame extension 3.4.0 sends (the
/// fixed copy in edm-protocol's `tests/wire`) is answered, at version 1, with the reply of its
/// kind, and its bare hello is kept as an extension that says nothing of itself.
#[test]
fn every_frame_a_released_extension_sends_is_answered() {
    for (release, text) in [
        (
            "3.4.0",
            include_str!("../../../edm-protocol/tests/wire/extension-3.4.0.json"),
        ),
        (
            "3.5.0",
            include_str!("../../../edm-protocol/tests/wire/extension-3.5.0.json"),
        ),
    ] {
        every_frame_of_a_release_is_answered(release, text);
    }
}

fn every_frame_of_a_release_is_answered(release: &str, text: &str) {
    let file: serde_json::Value = serde_json::from_str(text).unwrap();
    for frame in file["requests"].as_array().unwrap() {
        let dir = tempfile::tempdir().unwrap();
        let mut world = Pretend {
            running: true,
            launches: Ok(()),
            launched: false,
            refuses: None,
            asked: Vec::new(),
            cookies_seen: Vec::new(),
            serves: "application/octet-stream",
            clock: 0,
            probe_takes_ms: 0,
            budgets: Vec::new(),
        };
        let answer = answer(dir.path(), frame, &mut world)
            .unwrap_or_else(|| panic!("{frame} got no answer"));
        assert_eq!(answer["protocolVersion"], 1, "{frame}: {answer}");
        let reply = Reply::from_frame(&answer).unwrap();
        let kind = frame["type"].as_str().unwrap();
        let fits = match (kind, &reply) {
            ("engine.hello", Reply::Hello(h)) => h.judges_pages() && h.takes_renewals(),
            ("settings.get", Reply::Settings(_)) => true,
            ("download.start" | "download.batch" | "menu.clicked", Reply::Started { id, .. }) => {
                *id == frame["id"]
            }
            ("media.formats", Reply::Formats { .. }) => true,
            ("app.show", Reply::Shown { .. }) => true,
            ("catch.judge", Reply::Judged { id, .. }) => *id == frame["id"],
            ("media.streams", Reply::Streams { streams }) => !streams.is_empty(),
            _ => false,
        };
        assert!(fits, "{release}: {frame} was answered {answer}");
        if kind == "engine.hello" {
            let heard = extension_heard(dir.path()).expect("the hello is kept");
            assert_eq!(
                heard.extension_version,
                frame["extensionVersion"].as_str().unwrap_or("")
            );
        }
    }
}

/// What a newer extension says of itself is kept for the app, and written again only when it
/// changes.
#[test]
fn an_extensions_greeting_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(extension_heard(dir.path()), None);
    let greeting = edm_protocol::Greeting {
        extension_version: "3.5.0".into(),
        features: vec![edm_protocol::feature::READS_ANY_REPLY.into()],
    };
    let (reply, _) = ask(
        dir.path(),
        edm_protocol::Request::Hello(greeting.clone()).to_frame(),
        true,
        Ok(()),
    );
    let Some(Reply::Hello(hello)) = reply else {
        panic!("no hello")
    };
    assert!(hello.has(edm_protocol::feature::RENEWS) && hello.renews);
    assert_eq!(extension_heard(dir.path()), Some(greeting));
}

/// The app drives the menu, the catch verdict and the panel's list (gaps row A22): its hello
/// serves the menu less what Options leaves out, a caught download is judged by the rules saved
/// now, a click on an item it added is filed as a hand-off, and the panel's list is its own.
#[test]
fn the_app_serves_the_menu_judges_catches_and_lists_streams() {
    let dir = tempfile::tempdir().unwrap();
    let said =
        |frame: serde_json::Value| ask(dir.path(), frame, true, Ok(())).0.expect("an answer");
    let hello = serde_json::json!({ "protocolVersion": 1, "type": "engine.hello" });
    let Reply::Hello(first) = said(hello.clone()) else {
        panic!("no hello")
    };
    for name in [
        edm_protocol::feature::MENUS,
        edm_protocol::feature::JUDGES_CATCH,
        edm_protocol::feature::LISTS_STREAMS,
        edm_protocol::feature::LISTS_DOWNLOADS,
        edm_protocol::feature::ACTS_ON_DOWNLOADS,
        edm_protocol::feature::KEEPS_NO_RECORD,
    ] {
        assert!(first.has(name), "{name}");
    }
    assert_eq!(first.menus.as_ref().map(Vec::len), Some(4));
    assert_eq!(
        first.status, None,
        "the extension's own words say how the app is"
    );

    // Options changed: the next hello serves the menu without the item, and the next catch
    // question is judged by the rules as saved now.
    std::fs::write(
        dir.path().join("settings.json"),
        r#"{"catch":{"enabled":true,"extensions":[],"defaultsVersion":2,"minSizeKb":1024,"hiddenMenus":["edm-media"]}}"#,
    )
    .unwrap();
    let Reply::Hello(changed) = said(hello) else {
        panic!("no hello")
    };
    let ids: Vec<String> = changed.menus.unwrap().into_iter().map(|m| m.id).collect();
    assert_eq!(ids, ["edm-link", "edm-all-links", "edm-selected-links"]);
    let judge = |size: u64| {
        serde_json::json!({
            "protocolVersion": 1, "type": "catch.judge", "id": "j1",
            "url": "https://example.com/a.zip", "size": size
        })
    };
    match said(judge(10)) {
        Reply::Judged { id, take, reason } => {
            assert_eq!(id, "j1");
            assert!(!take);
            assert!(reason.unwrap().contains("minimum size"));
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        said(judge(5 << 20)),
        Reply::Judged { take: true, .. }
    ));
    let filed = || -> Vec<std::path::PathBuf> {
        std::fs::read_dir(inbox(dir.path()))
            .map(|d| d.flatten().map(|e| e.path()).collect())
            .unwrap_or_default()
    };
    assert!(filed().is_empty(), "a question files nothing");

    // A click on an item the app added: its link, filed as a hand-off.
    let click = serde_json::json!({
        "protocolVersion": 1, "type": "menu.clicked", "id": "c1", "item": "edm-new",
        "linkUrl": "https://example.com/b.zip", "pageUrl": "https://example.com/list"
    });
    assert!(matches!(said(click), Reply::Started { ref id, .. } if id == "c1"));
    let request: Request =
        serde_json::from_str(&std::fs::read_to_string(inbox(dir.path()).join("c1.json")).unwrap())
            .unwrap();
    assert_eq!(request.url, "https://example.com/b.zip");
    assert_eq!(request.referrer, "https://example.com/list");
    assert!(!request.caught, "asked for, as a menu's link is");
    let nothing = serde_json::json!({
        "protocolVersion": 1, "type": "menu.clicked", "id": "c2", "item": "edm-new",
        "pageUrl": "chrome://settings"
    });
    assert!(matches!(said(nothing), Reply::Failed { .. }));

    // The panel's list: a picture and a sound fetched apart are one line.
    let streams = serde_json::json!({
        "protocolVersion": 1, "type": "media.streams", "page": "https://example.com/watch",
        "streams": [
            { "url": "https://cdn.example/v.mp4", "kind": "Picture", "size": 5000000, "label": "720p video file" },
            { "url": "https://cdn.example/a.mp4", "kind": "Sound", "size": 900000, "label": "audio file" }
        ]
    });
    let Reply::Streams { streams } = said(streams) else {
        panic!("no list")
    };
    assert_eq!(streams.len(), 1);
    assert_eq!(
        streams[0].sound.as_deref(),
        Some("https://cdn.example/a.mp4")
    );
}

/// The host as the browser runs it: frames in on one pipe, answers out on the other, in turn;
/// a frame that gets no answer (another version's) is skipped and the next one still answered.
#[test]
fn the_host_answers_each_frame_on_its_pipe_in_turn_and_skips_what_it_does_not_take() {
    let dir = tempfile::tempdir().unwrap();
    let mut input = Vec::new();
    for value in [
        serde_json::json!({ "protocolVersion": 1, "type": "engine.hello", "extensionVersion": "3.5.0" }),
        serde_json::json!({ "protocolVersion": 99, "type": "engine.hello" }),
        serde_json::json!({ "protocolVersion": 1, "type": "catch.judge", "id": "j", "url": "https://e.com/a.zip" }),
    ] {
        input.extend(frame(&value));
    }
    let mut output = Vec::new();
    serve(
        dir.path(),
        &mut input.as_slice(),
        &mut output,
        &mut pretend("application/zip", 0, 0),
    );
    let mut answers = output.as_slice();
    let first = read_frame(&mut answers).expect("the hello's answer");
    assert_eq!(first["type"], "engine.hello");
    let second = read_frame(&mut answers).expect("the verdict");
    assert_eq!(second["type"], "catch.judged");
    assert_eq!(second["take"], true);
    assert!(
        read_frame(&mut answers).is_none(),
        "the unread frame got nothing"
    );
}

/// A browser that closed its end stops the host at once, with nothing written half.
#[test]
fn the_host_stops_when_the_browser_stops_reading() {
    struct Closed;
    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let mut input = Vec::new();
    for _ in 0..3 {
        input.extend(frame(
            &serde_json::json!({ "protocolVersion": 1, "type": "settings.get" }),
        ));
    }
    let mut reader = input.as_slice();
    serve(
        dir.path(),
        &mut reader,
        &mut Closed,
        &mut pretend("application/zip", 0, 0),
    );
    assert!(
        !reader.is_empty(),
        "the frames after the failed write are not read"
    );
}

/// A click on an item the app added: its link, else the media's own address, else the page (or
/// the frame it was in) read for its video; nothing where none is a web address.
#[test]
fn a_click_on_the_apps_own_item_hands_over_the_best_address_it_has() {
    let click = |value: serde_json::Value| -> edm_protocol::MenuClicked {
        serde_json::from_value(value).unwrap()
    };
    let on_media = menu_start(&click(serde_json::json!({
        "id": "c1", "linkUrl": "javascript:void(0)", "srcUrl": " https://cdn.e.com/v.mp4 ",
        "pageUrl": "https://e.com/watch", "cookies": "s=1", "cookiesOnce": true, "title": "Watch"
    })))
    .unwrap();
    assert_eq!(on_media.url, "https://cdn.e.com/v.mp4");
    assert_eq!(on_media.referrer, "https://e.com/watch");
    assert_eq!(
        (on_media.cookies.as_str(), on_media.cookies_once),
        ("s=1", true)
    );
    assert_eq!(on_media.title, "Watch");
    assert_eq!(on_media.quality, None, "a file is not read as a page");
    let magnet = menu_start(&click(
        serde_json::json!({ "id": "c2", "linkUrl": "magnet:?xt=urn:btih:00" }),
    ))
    .unwrap();
    assert!(magnet.url.starts_with("magnet:"));
    let in_frame = menu_start(&click(serde_json::json!({
        "id": "c3", "srcUrl": "blob:https://e.com/1", "pageUrl": "https://e.com/",
        "frameUrl": "https://player.e.com/embed/1"
    })))
    .unwrap();
    assert_eq!(in_frame.url, "https://player.e.com/embed/1");
    assert_eq!(in_frame.referrer, "");
    assert_eq!(
        in_frame.quality.as_deref(),
        Some("best"),
        "a page is read for its video"
    );
    let on_page = menu_start(&click(
        serde_json::json!({ "id": "c4", "pageUrl": "ftp://e.com/pub/" }),
    ))
    .unwrap();
    assert_eq!(on_page.url, "ftp://e.com/pub/");
    assert!(menu_start(&click(
        serde_json::json!({ "id": "c5", "pageUrl": "about:blank" })
    ))
    .is_err());
    assert!(menu_start(&click(serde_json::json!({ "id": "c6" }))).is_err());
}

/// The app's hello is the same whichever extension asks, and an older one is answered whole:
/// the fields 3.4.0 reads keep their meaning beside the ones added for 3.5.0.
#[test]
fn the_apps_hello_keeps_every_field_an_older_extension_reads() {
    let dir = tempfile::tempdir().unwrap();
    let bare = serde_json::json!({ "protocolVersion": 1, "type": "engine.hello" });
    let answer = answer(dir.path(), &bare, &mut pretend("application/zip", 0, 0)).unwrap();
    assert_eq!(answer["protocolVersion"], 1);
    assert_eq!(answer["renews"], true);
    assert_eq!(answer["protocol"], edm_protocol::PROTOCOL_VERSION);
    assert!(answer["engineVersion"]
        .as_str()
        .is_some_and(|v| !v.is_empty()));
    assert!(answer["rules"].is_object());
    let menus = answer["menus"].as_array().unwrap();
    assert_eq!(menus.len(), edm_protocol::MENU_ITEMS.len());
    assert!(answer.get("status").is_none());
}

/// The toolbar popup's list (gaps row U18): what the window last wrote, while EDM runs; with EDM
/// gone, nothing running, and its last failure still said.
#[test]
fn the_popup_is_told_what_the_window_is_fetching() {
    let dir = tempfile::tempdir().unwrap();
    let frame = edm_protocol::Request::Downloads.to_frame();
    // Nothing written yet: nothing running.
    let (reply, _) = ask(dir.path(), frame.clone(), true, Ok(()));
    assert_eq!(
        reply,
        Some(Reply::Downloads {
            downloads: Vec::new(),
            running: 0,
            failed: None,
            recent: Vec::new(),
        })
    );
    let published = crate::app::going::Published {
        at_ms: 0,
        downloads: vec![edm_protocol::Going {
            name: "a.iso".into(),
            percent: Some(12),
            speed: 3_000_000,
            id: Some(4),
        }],
        running: 7,
        failed: Some(edm_protocol::LastFailure {
            name: "b.zip".into(),
            why: "the server refused it".into(),
        }),
        recent: vec![edm_protocol::Recent {
            id: 2,
            name: "c.pdf".into(),
            state: edm_protocol::recent_state::FINISHED.into(),
        }],
    };
    std::fs::write(
        crate::app::going::file(dir.path()),
        serde_json::to_vec(&published).unwrap(),
    )
    .unwrap();
    let (
        Some(Reply::Downloads {
            downloads,
            running,
            failed,
            recent,
        }),
        _,
    ) = ask(dir.path(), frame.clone(), true, Ok(()))
    else {
        panic!("no list")
    };
    assert_eq!((downloads, running), (published.downloads.clone(), 7));
    assert_eq!(failed, published.failed);
    assert_eq!(recent, published.recent);
    // EDM is not running: what it wrote no longer runs, and nothing is started to say so.
    let (
        Some(Reply::Downloads {
            downloads,
            failed,
            recent,
            ..
        }),
        launched,
    ) = ask(dir.path(), frame, false, Ok(()))
    else {
        panic!("no list")
    };
    assert!(downloads.is_empty());
    // Nothing can be done to them with no EDM: no recent downloads.
    assert!(recent.is_empty());
    assert_eq!(failed, published.failed);
    assert!(!launched, "asking for the list starts no EDM");
}

/// The popup's buttons (gaps row C12): an ask left in the inbox for the window, which a running EDM
/// takes; refused, and nothing left, with no EDM running or for an ask it does not know.
#[test]
fn the_popups_ask_is_left_for_the_window() {
    use edm_protocol::{act_does, Act};
    let dir = tempfile::tempdir().unwrap();
    let frame = |does: &str, id: Option<u64>| {
        edm_protocol::Request::Act(Act {
            does: does.into(),
            id,
        })
        .to_frame()
    };
    let (reply, launched) = ask(dir.path(), frame(act_does::OPEN, Some(7)), true, Ok(()));
    assert_eq!(reply, Some(Reply::Acted { reason: None }));
    assert!(!launched);
    let (reply, _) = ask(
        dir.path(),
        frame(act_does::CLEAR_FINISHED, None),
        true,
        Ok(()),
    );
    assert_eq!(reply, Some(Reply::Acted { reason: None }));
    assert!(crate::app::going::window_asked(dir.path()));
    let taken = crate::app::going::asked_of_window(dir.path());
    assert_eq!(taken.len(), 2);
    assert!(taken.contains(&Act {
        does: act_does::OPEN.into(),
        id: Some(7),
    }));
    assert!(taken.contains(&Act {
        does: act_does::CLEAR_FINISHED.into(),
        id: None,
    }));
    // Taken once.
    assert!(crate::app::going::asked_of_window(dir.path()).is_empty());
    assert!(!crate::app::going::window_asked(dir.path()));

    // No EDM running: not left, and EDM is not started for it.
    let (reply, launched) = ask(dir.path(), frame(act_does::STOP, Some(1)), false, Ok(()));
    assert!(
        matches!(reply, Some(Reply::Acted { reason: Some(_) })),
        "{reply:?}"
    );
    assert!(!launched);
    // An ask this EDM does not know, or one missing its row.
    for (does, id) in [("delete-file", Some(1)), (act_does::RESUME, None)] {
        let (reply, _) = ask(dir.path(), frame(does, id), true, Ok(()));
        assert!(
            matches!(reply, Some(Reply::Acted { reason: Some(_) })),
            "{does}"
        );
    }
    assert!(!crate::app::going::window_asked(dir.path()));
}

/// A hand-over from a private window is filed keeping no record, and added so (gaps row W17); one
/// from an ordinary window, as every released extension sends, keeps its row as before.
#[test]
fn a_private_windows_hand_over_is_filed_keeping_no_record() {
    let dir = tempfile::tempdir().unwrap();
    let filed = |id: &str, private_window: bool| -> Request {
        let frame = edm_protocol::Request::Start(Start {
            id: id.into(),
            url: "https://e.com/f.zip".into(),
            referrer: "https://e.com/page".into(),
            private_window,
            ..Start::default()
        })
        .to_frame();
        let (reply, _) = ask(dir.path(), frame, true, Ok(()));
        assert!(matches!(reply, Some(Reply::Started { .. })), "{reply:?}");
        let kept = std::fs::read_to_string(inbox(dir.path()).join(format!("{id}.json"))).unwrap();
        serde_json::from_str(&kept).unwrap()
    };
    let private = filed("p1", true);
    assert!(private.keep_no_record);
    assert!(crate::shell::add_options(&private).keep_no_record);
    let ordinary = filed("p2", false);
    assert!(!ordinary.keep_no_record);
    assert!(!crate::shell::add_options(&ordinary).keep_no_record);
}
