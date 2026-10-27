use super::*;

#[test]
fn a_message_survives_being_framed_and_read_back() {
    let value = serde_json::json!({ "protocolVersion": 1, "type": "engine.hello" });
    let bytes = frame(&value);

    // Four bytes of little-endian length, then the body.
    assert_eq!(&bytes[..4], &(bytes.len() as u32 - 4).to_le_bytes());
    assert_eq!(read_frame(&mut &bytes[..]), Some(value));
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
        assert!(!dm_core::queue::accepted(refused), "{refused}");
    }
    assert!(dm_core::queue::accepted("https://e.com/f.bin"));
    assert!(dm_core::queue::accepted("ftps://e.com/f.bin"));
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
    /* Ids that come to one name once made plain used to be written over
     * each other: the first download lost, both reported taken. */
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
/// records whether it was asked, and servers that answer dm or do not.
struct Pretend {
    running: bool,
    launches: Result<(), ()>,
    launched: bool,
    refuses: Option<&'static str>,
    asked: Vec<(String, String)>,
    /// The cookies each probe was asked with.
    cookies_seen: Vec<String>,
    /// The type the server sends dm.
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
            return Err("yt-dlp could not read that page".into());
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
    })
    .to_frame();
    let (reply, _) = ask(dir.path(), frame, true, Ok(()));
    assert_eq!(
        reply,
        Some(Reply::Started {
            id: "links-1".into()
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
    assert_eq!(reason.as_deref(), Some("yt-dlp could not read that page"));
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
    assert_eq!(reply, Some(Reply::Started { id: "c1".into() }));
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
            id: "panel-1".into()
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
            id: "pair-1".into()
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
    dm_protocol::Request::Start(Start {
        id: id.into(),
        url: url.into(),
        referrer: "https://e.com/page".into(),
        caught: true,
        ..Start::default()
    })
    .to_frame()
}

#[test]
fn a_hello_is_remembered_so_the_window_stops_offering_the_extension() {
    let home = tempfile::tempdir().unwrap();
    assert!(!browser_seen(home.path()));
    let _ = ask(
        home.path(),
        dm_protocol::Request::Hello.to_frame(),
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
        dm_protocol::Request::Hello.to_frame(),
        true,
        Ok(()),
    );
    let Some(Reply::Hello(hello)) = reply else {
        panic!("no hello")
    };
    assert_eq!(hello.engine_version, env!("CARGO_PKG_VERSION"));
    assert!(hello.ytdlp.is_some(), "the extension is told either way");
    // Said, so the extension hands this app the downloads the browser
    // holds: it judges a sign-in page served in the file's place.
    assert!(hello.judges_pages());
    assert_eq!(hello.protocol, Some(dm_protocol::PROTOCOL_VERSION));
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
    let asked = dm_protocol::Request::Settings.to_frame();

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
        assert_eq!(reply, Some(Reply::Started { id: id.into() }));
        serde_json::from_str(
            &std::fs::read_to_string(inbox(home).join(format!("{id}.json"))).unwrap(),
        )
        .unwrap()
    };
    // A new install starts a download at once.
    let home = tempfile::tempdir().unwrap();
    let new = filed(home.path(), "c1");
    assert!(new.caught && !new.ask);
    // So does a settings file from before the choice existed.
    std::fs::write(home.path().join("settings.json"), r#"{"connections":4}"#).unwrap();
    let old = filed(home.path(), "c2");
    assert!(old.caught && !old.ask);
    std::fs::write(
        home.path().join("settings.json"),
        r#"{"ask_caught_downloads":true}"#,
    )
    .unwrap();
    assert!(filed(home.path(), "c3").ask);
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
    assert_eq!(reply, Some(Reply::Started { id: "b1-2".into() }));
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
    assert_eq!(reply, Some(Reply::Started { id: "a".into() }));

    let (reply, _) = ask(
        dir.path(),
        start("b", "https://e.com/f.zip"),
        false,
        Err(()),
    );
    let Some(Reply::Failed { id, failure }) = reply else {
        panic!("a launch that failed was reported as taken")
    };
    assert_eq!(id, "b");
    assert!(failure.unwrap().message.contains("could not be started"));
    // The browser keeps it, so nothing of it is left for a dm started
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
    // A .torrent on this computer is opened with dm by the person, never
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
fn a_link_dm_cannot_fetch_itself_is_left_with_the_browser() {
    /* The browser has the site's cookies and dm does not. Taken anyway,
     * the browser's copy is cancelled and dm's fails: the download is
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
        panic!("a link dm cannot reach was taken")
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
/// asked about before dm takes its download; another site still is.
#[test]
fn a_link_from_a_site_whose_links_work_once_is_taken_unasked() {
    let dir = tempfile::tempdir().unwrap();
    let mut settings = crate::Settings::default();
    settings.catch.once_hosts = vec!["files.e.com".into()];
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
    dm_protocol::Request::Start(Start {
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
    /* The site sends dm, without the browser's cookies, a redirect to
     * its sign-in page: a 200 of HTML, which a check for errors alone
     * passed. Taken, the browser's copy was cancelled and dm saved the
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

    // A page the browser was saving as a page is the same page to dm.
    let reply = answer(
        dir.path(),
        &caught("s3", "https://e.com/article", "text/html", None),
        &mut world,
    )
    .map(|r| Reply::from_frame(&r).unwrap());
    assert_eq!(reply, Some(Reply::Started { id: "s3".into() }));

    // And the file itself, served as one, is taken.
    let mut world = pretend("application/zip", 0, 0);
    let reply = answer(
        dir.path(),
        &caught("s4", "https://e.com/get?id=1", "application/zip", None),
        &mut world,
    )
    .map(|r| Reply::from_frame(&r).unwrap());
    assert_eq!(reply, Some(Reply::Started { id: "s4".into() }));
}

#[test]
fn a_hand_off_the_extension_has_stopped_waiting_for_is_not_filed() {
    /* The extension waits so long for an answer and then gives the
     * download back to the browser. A host that took it after that had
     * the file downloaded twice: by the browser and by dm. */
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
    assert_eq!(reply, Some(Reply::Started { id: "t2".into() }));
    assert_eq!(world.budgets, [5_000 - dm_protocol::ANSWER_MARGIN_MS]);
    let mut world = pretend("application/zip", now, 0);
    let _ = answer(
        dir.path(),
        &caught("t3", "https://e.com/f.zip", "", Some(now + 60_000)),
        &mut world,
    );
    assert_eq!(world.budgets, [dm_protocol::PROBE_WITHIN_MS]);

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
    let late = dm_protocol::Request::Start(Start {
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
    let asked = dm_protocol::Request::Start(Start {
        id: "m1".into(),
        url: "https://e.com/f.zip".into(),
        ..Default::default()
    })
    .to_frame();
    let reply = answer(dir.path(), &asked, &mut world).map(|r| Reply::from_frame(&r).unwrap());
    assert_eq!(reply, Some(Reply::Started { id: "m1".into() }));
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

/// What the browser's host leaves for the helper that reads a page behind the
/// panel's list: only an http(s) page is read, and nothing else is made.
#[test]
fn the_page_reading_helper_ignores_what_is_not_a_page() {
    let home = tempfile::tempdir().unwrap();
    for said in [
        "not json",
        "{}",
        r#"{"url": "file:///etc/passwd"}"#,
        r#"{"url": "javascript:alert(1)"}"#,
    ] {
        warm_page(home.path(), said);
    }
    assert!(
        !home.path().join("readings").exists(),
        "a reading was made of what is not a page"
    );
}
