use super::*;
use serde_json::json;

fn file(url: &str) -> Candidate<'_> {
    Candidate {
        url,
        ..Default::default()
    }
}

#[test]
fn every_request_carries_the_version_and_reads_back() {
    for request in [
        Request::Hello,
        Request::Settings,
        Request::Start(Start {
            id: "b7".into(),
            url: "https://e.com/f.zip".into(),
            referrer: "https://e.com/".into(),
            caught: true,
            cookies: "session=1; theme=dark".into(),
            ..Start::default()
        }),
        Request::Start(Start {
            id: "m1".into(),
            url: "https://e.com/f.zip".into(),
            ..Default::default()
        }),
    ] {
        let frame = request.to_frame();
        assert_eq!(frame["protocolVersion"], request.needs());
        assert_eq!(Request::from_frame(&frame), Ok(request));
    }
}

#[test]
fn the_wire_names_are_the_ones_the_host_has_always_used() {
    // An extension and a host of different builds meet in the field; the
    // strings are the contract, not the Rust names.
    assert_eq!(Request::Hello.to_frame()["type"], kind::HELLO);
    assert_eq!(Request::Settings.to_frame()["type"], kind::SETTINGS);
    let start = Request::Start(Start::default()).to_frame();
    assert_eq!(start["type"], kind::START);
    assert_eq!(
        Reply::Started { id: "a".into() }.to_frame()["type"],
        kind::STARTED
    );
    let failed = Reply::failed("a", "no").to_frame();
    assert_eq!(failed["type"], kind::FAILED);
    assert_eq!(failed["failure"]["message"], "no");
    let hello = Reply::Hello(Hello {
        engine_version: "1.2".into(),
        ytdlp: Some(false),
        protocol: None,
    })
    .to_frame();
    assert_eq!(hello["engineVersion"], "1.2");
    assert_eq!(hello["ytdlp"], false);
}

#[test]
fn a_frame_of_another_version_is_not_read() {
    let frame = json!({ "protocolVersion": 3, "type": "engine.hello" });
    assert_eq!(
        Request::from_frame(&frame),
        Err(Unreadable::Version(Some(3)))
    );
    let frame = json!({ "protocolVersion": 0, "type": "engine.hello" });
    assert_eq!(
        Request::from_frame(&frame),
        Err(Unreadable::Version(Some(0)))
    );
    let frame = json!({ "type": "engine.hello" });
    assert_eq!(Request::from_frame(&frame), Err(Unreadable::Version(None)));
}

#[test]
fn an_older_extensions_extra_keys_are_ignored_and_an_unknown_type_refused() {
    let frame = json!({
        "protocolVersion": 1, "type": "download.start", "id": "x",
        "url": "https://e.com/f", "filename": "f", "connections": 8, "via": "auto",
    });
    assert!(
        matches!(Request::from_frame(&frame), Ok(Request::Start(s)) if s.url == "https://e.com/f")
    );
    let Ok(Request::Start(older)) = Request::from_frame(&frame) else {
        unreachable!()
    };
    assert!(
        !older.caught,
        "an older extension's hand-off is not checked"
    );
    let pause = json!({ "protocolVersion": 1, "type": "download.pause", "id": "x" });
    assert!(matches!(
        Request::from_frame(&pause),
        Err(Unreadable::Shape(_))
    ));
}

#[test]
fn a_caught_download_goes_out_at_the_version_that_judges_pages() {
    // Version 1 hosts answer only version 1 frames, and ignore `mime`:
    // a download the browser holds is sent where one cannot take it.
    let caught = Request::Start(Start {
        id: "b1".into(),
        url: "https://e.com/f.zip".into(),
        caught: true,
        held: true,
        mime: "application/zip".into(),
        ..Start::default()
    });
    assert_eq!(caught.to_frame()["protocolVersion"], JUDGES_PAGES_SINCE);
    // A file asked for from a menu or the panel is checked too, but the
    // browser holds nothing: a version-1 host must still answer it.
    let chosen = Request::Start(Start {
        id: "m1".into(),
        url: "https://e.com/f.zip".into(),
        caught: true,
        ..Start::default()
    });
    assert_eq!(chosen.to_frame()["protocolVersion"], 1);
    // Everything an older host reads correctly still reaches it.
    for old in [
        Request::Hello,
        Request::Settings,
        Request::Show,
        Request::Start(Start {
            id: "m1".into(),
            url: "https://e.com/watch".into(),
            ..Start::default()
        }),
    ] {
        assert_eq!(old.to_frame()["protocolVersion"], 1);
    }
    // Both versions are read here, and replies go out at 1, which an
    // older extension reads.
    for v in [1, 2] {
        let frame = json!({ "protocolVersion": v, "type": "engine.hello" });
        assert_eq!(Request::from_frame(&frame), Ok(Request::Hello));
    }
    let hello = Reply::Hello(Hello {
        protocol: Some(PROTOCOL_VERSION),
        ..Hello::default()
    })
    .to_frame();
    assert_eq!(hello["protocolVersion"], 1);
    assert_eq!(hello["protocol"], PROTOCOL_VERSION);
}

#[test]
fn an_app_that_does_not_say_its_protocol_does_not_judge_pages() {
    let old = json!({ "protocolVersion": 1, "type": "engine.hello", "engineVersion": "2.0.0" });
    let Ok(Reply::Hello(hello)) = Reply::from_frame(&old) else {
        panic!("not read")
    };
    assert_eq!(hello.protocol(), 1);
    assert!(!hello.judges_pages());
    let new = json!({ "protocolVersion": 1, "type": "engine.hello", "protocol": 2 });
    let Ok(Reply::Hello(hello)) = Reply::from_frame(&new) else {
        panic!("not read")
    };
    assert!(hello.judges_pages());
}

#[test]
fn a_hello_from_a_host_too_old_to_mention_ytdlp_says_unknown() {
    let frame = json!({ "protocolVersion": 1, "type": "engine.hello", "engineVersion": "0.1" });
    let Ok(Reply::Hello(hello)) = Reply::from_frame(&frame) else {
        panic!("not read")
    };
    assert_eq!(hello.ytdlp, None);
}

#[test]
fn the_rules_cross_as_written_and_read_from_either_spelling() {
    let reply = Reply::Settings(SettingsReply {
        rules: CatchRules {
            enabled: false,
            extensions: vec!["zip".into()],
            defaults_version: 1,
            min_size_kb: 5,
            skip_hosts: vec!["e.com".into()],
            keep_key: "ctrl".into(),
            take_key: String::new(),
            once_hosts: Vec::new(),
            video_panel: false,
            sign_in_everywhere: true,
        },
    });
    let frame = reply.to_frame();
    assert_eq!(frame["catch"]["videoPanel"], false);
    assert_eq!(frame["catch"]["signInEverywhere"], true);
    assert_eq!(frame["catch"]["minSizeKb"], 5);
    assert_eq!(frame["catch"]["keepKey"], "ctrl");
    assert_eq!(frame["catch"]["takeKey"], "");
    assert_eq!(frame["catch"]["skipHosts"][0], "e.com");
    assert_eq!(Reply::from_frame(&frame), Ok(reply));

    // A hand-edited settings file may well use the file's own spelling.
    let typed: CatchRules =
        serde_json::from_value(json!({ "min_size_kb": 3, "skip_hosts": ["a.b"] })).unwrap();
    assert_eq!(typed.min_size_kb, 3);
    assert_eq!(typed.skip_hosts, vec!["a.b".to_string()]);
    assert!(typed.enabled, "a missing field takes its default");
    assert!(
        typed.video_panel,
        "rules from an older dm keep the panel showing"
    );
    assert!(
        !typed.sign_in_everywhere,
        "a sign-in goes only where chosen"
    );
    assert!(typed.extensions.is_empty(), "no list chosen is every type");
}

#[test]
fn a_chosen_list_of_types_catches_files_not_pages() {
    let rules = CatchRules {
        extensions: DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect(),
        ..CatchRules::default()
    };
    for caught in [
        "https://e.com/setup.exe",
        "https://e.com/a/b/archive.ZIP?token=1",
        "https://e.com/film.mkv#t=3",
        "https://e.com/book.pdf",
        "https://e.com/linux.iso",
        "https://e.com/x.torrent",
    ] {
        assert_eq!(rules.catches(&file(caught)), Ok(()), "{caught}");
    }
    for left in [
        "https://e.com/",
        "https://e.com/page.html",
        "https://e.com/notes.txt",
        "https://e.com/data.json",
        "https://e.com/download?id=5",
    ] {
        assert_eq!(
            rules.catches(&file(left)),
            Err(Left::NotAFileType),
            "{left}"
        );
    }
}

#[test]
fn the_browsers_name_and_the_servers_type_count_as_well_as_the_url() {
    let rules = CatchRules::default();
    let named = Candidate {
        url: "https://e.com/download?id=5",
        filename: "/Users/x/Downloads/report.pdf",
        ..Default::default()
    };
    assert_eq!(rules.catches(&named), Ok(()));
    let typed = Candidate {
        url: "https://e.com/download?id=5",
        mime: "application/zip; charset=binary",
        ..Default::default()
    };
    assert_eq!(rules.catches(&typed), Ok(()));
    let windows = Candidate {
        url: "https://e.com/get",
        filename: "C:\\Users\\x\\Downloads\\tool.msi",
        ..Default::default()
    };
    assert_eq!(rules.catches(&windows), Ok(()));
}

#[test]
fn a_list_written_by_hand_is_read_generously() {
    let rules = CatchRules {
        extensions: vec![".BIN".into(), " Zip ".into(), String::new()],
        ..Default::default()
    };
    assert_eq!(rules.catches(&file("https://e.com/f.bin")), Ok(()));
    assert_eq!(rules.catches(&file("https://e.com/f.zip")), Ok(()));
    assert_eq!(
        rules.catches(&file("https://e.com/f.exe")),
        Err(Left::NotAFileType)
    );
}

#[test]
fn an_empty_list_catches_every_type() {
    let rules = CatchRules {
        extensions: vec![],
        ..Default::default()
    };
    assert_eq!(rules.catches(&file("https://e.com/anything")), Ok(()));
}

#[test]
fn turned_off_catches_nothing() {
    let rules = CatchRules {
        enabled: false,
        ..Default::default()
    };
    assert_eq!(rules.catches(&file("https://e.com/f.zip")), Err(Left::Off));
}

#[test]
fn a_site_whose_links_work_once_is_known_by_its_host_or_the_page() {
    let rules = CatchRules {
        once_hosts: vec!["*.files.example".into(), "Once.example".into()],
        ..CatchRules::default()
    };
    assert!(rules.works_once("https://dl.files.example/a.zip", ""));
    assert!(rules.works_once("https://cdn.other/a.zip", "https://once.example/page"));
    assert!(!rules.works_once("https://notfiles.example/a.zip", ""));
    assert!(!CatchRules::default().works_once("https://once.example/a", ""));
    // An older settings file without the list still reads.
    let old: CatchRules = serde_json::from_value(serde_json::json!({ "enabled": true })).unwrap();
    assert!(old.once_hosts.is_empty());
}

#[test]
fn a_skipped_host_covers_its_subdomains_and_nothing_that_merely_ends_alike() {
    let rules = CatchRules {
        skip_hosts: vec!["Example.com".into(), "*.cdn.net".into()],
        ..Default::default()
    };
    let signed_in = format!(
        "https://user:{}@EXAMPLE.com:8443/f.zip",
        test_secrets::secret("password")
    );
    for left in [
        "https://example.com/f.zip",
        "https://dl.example.com/f.zip",
        signed_in.as_str(),
        "https://a.cdn.net/f.zip",
    ] {
        assert_eq!(rules.catches(&file(left)), Err(Left::SkippedHost), "{left}");
    }
    for caught in ["https://notexample.com/f.zip", "https://example.org/f.zip"] {
        assert_eq!(rules.catches(&file(caught)), Ok(()), "{caught}");
    }
    // An address saved whole, as one pasted in before Options cut it
    // down to its host.
    let pasted = CatchRules {
        skip_hosts: vec!["https://Ads.Example.com/path".into()],
        ..Default::default()
    };
    assert_eq!(
        pasted.catches(&file("https://ads.example.com/f.zip")),
        Err(Left::SkippedHost)
    );
}

#[test]
fn a_size_floor_leaves_small_files_but_not_unknown_ones() {
    let rules = CatchRules {
        min_size_kb: 100,
        ..Default::default()
    };
    let sized = |size| Candidate {
        url: "https://e.com/f.zip",
        size,
        ..Default::default()
    };
    assert_eq!(rules.catches(&sized(Some(99 * 1024))), Err(Left::TooSmall));
    assert_eq!(rules.catches(&sized(Some(100 * 1024))), Ok(()));
    assert_eq!(rules.catches(&sized(None)), Ok(()));
}

#[test]
fn hosts_are_read_out_of_every_shape_of_url() {
    assert_eq!(host_of("https://A.B.com/x"), "a.b.com");
    assert_eq!(host_of(concat!("http://u:", "p", "@h.com:80?x")), "h.com");
    assert_eq!(host_of("http://[::1]:8080/x"), "::1");
    assert_eq!(host_of("https://h.com./"), "h.com");
    assert_eq!(host_of("not a url"), "");
}

/* Chrome names an unpacked extension that carries a `key` by the key:
 * the first 32 hex digits of its SHA-256, each digit moved into a–p. The
 * host registration assumes that id, so a key replaced without the
 * constant would leave the unpacked extension talking to nothing. */
#[test]
fn the_manifest_key_yields_the_id_the_host_is_registered_for() {
    use sha2::{Digest, Sha256};

    let manifest: Value =
        serde_json::from_str(include_str!("../../../../extension/manifest.json")).unwrap();
    let key = manifest["key"].as_str().expect("the manifest has no key");

    let mut bits = 0u32;
    let mut held = 0;
    let mut der = Vec::new();
    for c in key.bytes().filter(|c| *c != b'=') {
        let six = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            other => panic!("{} is not base64", other as char),
        };
        bits = (bits << 6) | six as u32;
        held += 6;
        if held >= 8 {
            held -= 8;
            der.push((bits >> held) as u8);
        }
    }

    let id: String = Sha256::digest(&der)[..16]
        .iter()
        .flat_map(|b| [b >> 4, b & 0xf])
        .map(|n| (b'a' + n) as char)
        .collect();
    assert_eq!(id, CHROME_EXTENSION_ID);
}

#[test]
fn a_site_on_the_skip_list_keeps_downloads_served_from_elsewhere_and_its_pages() {
    let rules = CatchRules {
        skip_hosts: vec!["github.com".into()],
        ..Default::default()
    };
    // Asked for on the site, redirected to its file host.
    let redirected = Candidate {
        url: "https://objects.githubusercontent.com/f.zip",
        asked_url: "https://github.com/o/r/releases/download/v1/f.zip",
        ..Default::default()
    };
    assert_eq!(rules.catches(&redirected), Err(Left::SkippedHost));
    // Started from one of the site's pages.
    let from_page = Candidate {
        url: "https://cdn.example/f.zip",
        referrer: "https://github.com/o/r/releases",
        ..Default::default()
    };
    assert_eq!(rules.catches(&from_page), Err(Left::SkippedHost));
    let elsewhere = Candidate {
        url: "https://cdn.example/f.zip",
        asked_url: "https://cdn.example/f.zip",
        referrer: "https://example.org/",
        ..Default::default()
    };
    assert_eq!(rules.catches(&elsewhere), Ok(()));
}

#[test]
fn taking_every_download_from_a_site_overrides_type_and_size_only() {
    let page = Candidate {
        url: "https://e.com/notes.txt",
        size: Some(10),
        ..Default::default()
    };
    let rules = CatchRules {
        min_size_kb: 100,
        ..Default::default()
    };
    assert!(rules.catches(&page).is_err());
    assert_eq!(rules.takes(&page, true), Ok(()));
    // The app's own master switch and skip list still stand.
    let off = CatchRules {
        enabled: false,
        ..Default::default()
    };
    assert_eq!(off.takes(&page, true), Err(Left::Off));
    let skipping = CatchRules {
        skip_hosts: vec!["e.com".into()],
        ..Default::default()
    };
    assert_eq!(skipping.takes(&page, true), Err(Left::SkippedHost));
}

#[test]
fn a_page_served_to_dm_for_a_file_served_to_the_browser_is_told_apart() {
    let html = "text/html; charset=utf-8";
    // The browser was getting a zip; dm, without its cookies, a login page.
    assert!(page_instead_of_file(
        "application/zip",
        "https://e.com/get?id=1",
        html
    ));
    assert!(page_instead_of_file(
        "",
        "https://e.com/files/report.pdf",
        html
    ));
    assert!(page_instead_of_file(
        "application/octet-stream",
        "https://e.com/x",
        "application/xhtml+xml"
    ));
    // A page the browser was saving as a page, the same thing to dm.
    assert!(!page_instead_of_file(
        "text/html",
        "https://e.com/f.zip",
        html
    ));
    // Nothing said and nothing in the name: no way to tell, so taken.
    assert!(!page_instead_of_file(
        "",
        "https://e.com/download?id=5",
        html
    ));
    assert!(!page_instead_of_file(
        "",
        "https://e.com/get.php?id=5",
        html
    ));
    // A file served as a file.
    assert!(!page_instead_of_file(
        "application/zip",
        "https://e.com/f.zip",
        "application/zip"
    ));
    assert!(!page_instead_of_file("", "https://e.com/f.zip", ""));
}

#[test]
fn a_hand_off_is_given_only_the_time_the_extension_still_waits() {
    // An older extension says nothing: the probe's own limit.
    assert_eq!(time_left(5_000, None), Some(PROBE_WITHIN_MS));
    // Plenty left: the probe's limit, no more.
    assert_eq!(time_left(0, Some(60_000)), Some(PROBE_WITHIN_MS));
    // Little left: what remains short of the margin.
    assert_eq!(time_left(10_000, Some(15_000)), Some(3_000));
    // Within the margin or past it: too late to take.
    assert_eq!(time_left(10_000, Some(10_000 + ANSWER_MARGIN_MS)), None);
    assert_eq!(time_left(30_000, Some(20_000)), None);
}

#[test]
fn a_hand_offs_type_and_deadline_cross_and_an_older_one_has_neither() {
    let start = Request::Start(Start {
        id: "b1".into(),
        url: "https://e.com/f".into(),
        mime: "application/zip".into(),
        deadline: Some(1_700_000_000_000),
        ..Start::default()
    });
    let frame = start.to_frame();
    assert_eq!(frame["mime"], "application/zip");
    assert_eq!(frame["deadline"], 1_700_000_000_000u64);
    assert_eq!(Request::from_frame(&frame), Ok(start));
    let older = json!({ "protocolVersion": 1, "type": "download.start", "id": "x", "url": "u" });
    let Ok(Request::Start(older)) = Request::from_frame(&older) else {
        panic!("not read")
    };
    assert_eq!((older.mime.as_str(), older.deadline), ("", None));
}

#[test]
fn a_dot_in_a_directory_or_a_hidden_file_is_not_a_type() {
    assert_eq!(extension_of("archive.tar.gz"), Some("gz".into()));
    assert_eq!(extension_of(".bashrc"), None);
    assert_eq!(extension_of("v1.0"), Some("0".into()));
    assert_eq!(extension_of("noext"), None);
    assert_eq!(extension_of("weird.a-b"), None);
    assert_eq!(
        CatchRules {
            extensions: DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect(),
            ..CatchRules::default()
        }
        .catches(&file("https://e.com/v1.2/download")),
        Err(Left::NotAFileType)
    );
}

#[test]
fn out_of_the_box_every_type_is_caught() {
    let rules = CatchRules::default();
    for any in [
        "https://e.com/notes.txt",
        "https://e.com/a.zip",
        "https://e.com/data.json",
        "https://e.com/download?id=5",
        "https://e.com/page.html",
        "https://e.com/odd.xyz123",
    ] {
        assert_eq!(rules.catches(&file(any)), Ok(()), "{any}");
    }
}

#[test]
fn an_unchosen_list_from_an_earlier_edition_becomes_every_type_and_a_chosen_one_stays() {
    let old = |extensions: Vec<String>, defaults_version| CatchRules {
        extensions,
        defaults_version,
        ..CatchRules::default()
    };
    let defaults = || {
        DEFAULT_EXTENSIONS
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    };
    // Still the old defaults, from either edition.
    for version in [0, 1] {
        let mut rules = old(defaults(), version);
        rules.bring_up_to_date();
        assert!(rules.extensions.is_empty(), "version {version}");
        assert_eq!(rules.defaults_version, CATCH_DEFAULTS_VERSION);
    }
    // One the person changed keeps their choice, and a version-0 list still
    // gains the types added since.
    let mut mine = old(defaults(), 1);
    mine.extensions.retain(|e| e != "zip");
    mine.bring_up_to_date();
    assert!(!mine.extensions.is_empty() && !mine.extensions.contains(&"zip".to_string()));
    let mut short = old(vec!["zip".into(), "pdf".into()], 1);
    short.bring_up_to_date();
    assert_eq!(short.extensions, ["zip", "pdf"]);
    // Brought up once.
    let mut again = old(vec!["zip".into()], CATCH_DEFAULTS_VERSION);
    again.bring_up_to_date();
    assert_eq!(again.extensions, ["zip"]);
}
