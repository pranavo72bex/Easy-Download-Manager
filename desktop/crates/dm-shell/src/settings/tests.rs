use super::*;

#[test]
fn a_marker_beside_the_program_or_its_app_makes_it_portable() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    assert_eq!(portable_home_in(root), None, "no marker, not portable");
    std::fs::create_dir(root.join(PORTABLE_MARKER)).unwrap();
    assert_eq!(portable_home_in(root), None, "a folder is not the marker");
    std::fs::remove_dir(root.join(PORTABLE_MARKER)).unwrap();
    std::fs::write(root.join(PORTABLE_MARKER), "").unwrap();
    assert_eq!(portable_home_in(root), Some(root.join(PORTABLE_DATA)));

    let inside = root.join("dm.app/Contents/MacOS");
    std::fs::create_dir_all(&inside).unwrap();
    assert_eq!(
        portable_home_in(&inside),
        Some(root.join(PORTABLE_DATA)),
        "the marker beside dm.app"
    );
}

#[test]
fn a_site_login_goes_on_the_hosts_it_names_and_no_others() {
    use crate::test_secrets::{escaped, marked, secret};
    let password = marked("password");
    let settings = Settings {
        site_logins: vec![SiteLogin {
            host: "example.com".into(),
            username: "me".into(),
            password: password.clone(),
        }],
        ..Settings::default()
    };
    assert_eq!(
        settings.with_login("https://example.com/f.zip"),
        format!("https://me:{}@example.com/f.zip", escaped(&password))
    );
    // Not a subdomain, and not over plain http.
    assert_eq!(
        settings.with_login("https://dl.example.com/f.zip"),
        "https://dl.example.com/f.zip"
    );
    assert_eq!(
        settings.with_login("http://example.com/f.zip"),
        "http://example.com/f.zip"
    );
    let dotted = Settings {
        site_logins: vec![SiteLogin {
            host: ".example.com".into(),
            username: "me".into(),
            password: password.clone(),
        }],
        ..Settings::default()
    };
    assert!(dotted
        .with_login("https://dl.example.com/f.zip")
        .starts_with("https://me:"));
    assert_eq!(
        dotted.with_login("https://notexample.com/f.zip"),
        "https://notexample.com/f.zip"
    );
    assert_eq!(
        settings.with_login("https://notexample.com/f.zip"),
        "https://notexample.com/f.zip"
    );
    // An address with its own login keeps it.
    let theirs = format!("https://you:{}@example.com/f", secret("other"));
    assert_eq!(settings.with_login(&theirs), theirs);
    assert_eq!(settings.with_login("magnet:?xt=1"), "magnet:?xt=1");
    // A host saved as a whole address.
    let pasted = Settings {
        site_logins: vec![SiteLogin {
            host: "http://127.0.0.1:8000/basic.bin".into(),
            username: "me".into(),
            password: crate::test_secrets::secret("password"),
        }],
        ..Settings::default()
    };
    assert!(pasted
        .with_login("http://127.0.0.1:8000/other.bin")
        .starts_with("http://me:"));
}

#[test]
fn a_settings_file_that_cannot_be_read_is_kept_and_said() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, "{ \"connections\": 8, }").unwrap();

    let (settings, notice) = Settings::load_from(&path);
    assert_eq!(
        settings.connections,
        Settings::default().clamped().connections
    );
    let notice = notice.expect("a file that could not be read went unmentioned");
    assert!(notice.contains("could not be read"), "{notice}");
    assert!(
        !path.exists(),
        "the unreadable file is still where a save would replace it"
    );
    let kept = dir.path().join("settings.json.unreadable");
    assert_eq!(
        std::fs::read_to_string(kept).unwrap(),
        "{ \"connections\": 8, }"
    );
}

/// A category saved on a Mac or Linux with a name Windows
/// cannot make ("Live: 2024", "Misc.") was dropped at load, and the
/// next save deleted it. It is kept where it works; only a new or
/// renamed name is held to every computer's rules. A kind's folder
/// that cannot be used here is said, not filed into the root silently.
#[test]
fn a_saved_category_that_works_here_is_kept_and_a_new_one_is_portable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{"custom_categories": [{"name": "Live: 2024", "extensions": ["mkv"]},
                {"name": "Misc."}, {"name": "a/b"}],
                "category_folders": ["Videos/HD", "Music"]}"#,
    )
    .unwrap();
    let (read, notice) = Settings::load_from(&path);
    let names: Vec<&str> = read
        .custom_categories
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    if cfg!(windows) {
        assert!(names.is_empty(), "{names:?}");
    } else {
        assert_eq!(names, ["Live: 2024", "Misc."]);
        assert_eq!(read.custom_categories[0].extensions, ["mkv"]);
    }
    let notice = notice.expect("nothing said");
    assert!(notice.contains("\"a/b\""), "{notice}");
    assert!(notice.contains("\"Videos/HD\""), "{notice}");

    // Kept, it does not block an unrelated change; a new one is held to
    // every computer's rules, and so is a renamed kind's folder.
    let mut changed = read.clone();
    changed.connections = 3;
    assert_eq!(changed.refusal(&read), None);
    changed.custom_categories.push(dm_core::Custom {
        name: "Docs: work".into(),
        ..Default::default()
    });
    assert!(changed
        .refusal(&read)
        .is_some_and(|r| r.contains("Windows")));
    let mut renamed = read.clone();
    renamed.category_folders[1] = "CON".into();
    assert!(renamed.refusal(&read).is_some());
}

#[test]
fn one_odd_entry_costs_that_entry_and_not_every_setting() {
    /* A file edited by hand, or written by a later dm: a proxy type this
     * one does not know, a schedule and limits missing fields, a login
     * with no password, and a number written as a word. Each of these
     * used to move the whole file aside and start from the defaults. */
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let text = r#"{
            "connections": 4,
            "language": "de",
            "proxy": {"enabled": true, "type": "quic", "host": "p.example", "port": 3128},
            "schedule": {"enabled": true, "from": "02:00"},
            "host_limits": [{"host": "slow.example"}],
            "queue_limits": [{"queue": "Nightly"}],
            "site_logins": [{"host": "files.example", "username": "me"}],
            "custom_categories": [{"name": "Books"}],
            "concurrency": "three"
        }"#;
    std::fs::write(&path, text).unwrap();

    let (read, notice) = Settings::load_from(&path);
    assert_eq!(read.connections, 4);
    assert_eq!(read.language, "de");
    assert!(
        !read.proxy.enabled,
        "an unknown proxy type is off, not another kind"
    );
    assert_eq!(read.proxy.host, "p.example");
    assert!(read.schedule.enabled);
    assert_eq!(read.schedule.from, "02:00");
    assert_eq!(read.schedule.to, Schedule::default().to);
    assert_eq!(read.host_limits[0].host, "slow.example");
    assert_eq!(read.host_limits[0].connections, 1);
    assert_eq!(read.queue_limits[0].queue, "Nightly");
    assert_eq!(read.queue_limits[0].files, 1, "clamped up from none");
    assert_eq!(read.site_logins[0].username, "me");
    assert_eq!(read.custom_categories[0].name, "Books");
    assert_eq!(read.concurrency, Settings::default().concurrency);
    let notice = notice.expect("the entry left out went unmentioned");
    assert!(notice.contains("concurrency"), "{notice}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("settings.json.unreadable")).unwrap(),
        text,
        "the file as it was is kept"
    );
}

#[test]
fn what_a_reader_inside_left_out_is_named_and_the_file_kept() {
    /* The file reads as a whole — every odd value is inside a part read
     * leniently — so nothing failed at the top. An unknown proxy type,
     * a list entry that is not one, a bad field in an entry and a
     * category that is not one folder were each dropped without a
     * word, and the next save wrote over the only copy. */
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let text = r#"{
            "proxy": {"enabled": true, "type": "quic", "host": "p.example", "port": 3128},
            "site_logins": [7, {"host": "a.example", "username": "me"}],
            "queue_limits": [{"queue": "Nightly", "files": "two"}],
            "host_limits": [{"host": "slow.example", "connections": -1}],
            "custom_categories": [{"name": "Work/Docs", "extensions": ["pdf"]}, {"name": "Books"}]
        }"#;
    std::fs::write(&path, text).unwrap();
    let (read, notice) = Settings::load_from(&path);
    let notice = notice.expect("what was left out went unmentioned");
    for named in [
        "proxy.type",
        "site_logins[0]",
        "queue_limits[0].files",
        "host_limits[0].connections",
        "Work/Docs",
    ] {
        assert!(notice.contains(named), "{named} not named: {notice}");
    }
    assert!(!read.proxy.enabled);
    assert_eq!(read.site_logins.len(), 1);
    assert_eq!(read.queue_limits[0].queue, "Nightly");
    assert_eq!(
        read.custom_categories.len(),
        1,
        "the bad category is left out"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("settings.json.unreadable")).unwrap(),
        text,
        "the file as it was is kept before a save writes over it"
    );

    // A clean file says nothing and keeps no copy.
    let clean = tempfile::tempdir().unwrap();
    let path = clean.path().join("settings.json");
    std::fs::write(&path, serde_json::to_string(&Settings::default()).unwrap()).unwrap();
    assert_eq!(Settings::load_from(&path).1, None);
    assert!(!clean.path().join("settings.json.unreadable").exists());
}

#[test]
fn a_saved_schedule_with_unreadable_times_is_turned_off_and_said() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{"schedule": {"enabled": true, "from": "25:00", "to": "1am", "days": []}}"#,
    )
    .unwrap();
    let (read, notice) = Settings::load_from(&path);
    assert!(!read.schedule.enabled);
    assert!(notice.unwrap().contains("not HH:MM"));
}

#[test]
fn a_change_that_cannot_work_is_refused_and_an_old_one_blocks_nothing() {
    let before = Settings::default();
    let with = |change: fn(&mut Settings)| {
        let mut s = before.clone();
        change(&mut s);
        s.refusal(&before)
    };
    assert!(with(|s| {
        s.schedule.enabled = true;
        s.schedule.from = "25:00".into();
    })
    .is_some());
    assert!(with(|s| s.download_dir = "Downloads".into()).is_some());
    assert!(with(|s| s.download_dir = "~/Downloads".into()).is_none());
    assert!(with(|s| s.parts_dir = "parts".into()).is_some());
    assert!(with(|s| s.custom_categories = vec![dm_core::Custom {
        name: "Work/Docs".into(),
        ..Default::default()
    }])
    .is_some());
    assert!(with(|s| s.cookies_file = "/no/such/cookies.txt".into()).is_some());
    assert!(with(|s| s.language = "de".into()).is_none());

    // Saved before, and so not this change's doing.
    let odd = Settings {
        cookies_file: "/no/such/cookies.txt".into(),
        ..before.clone()
    };
    let mut next = odd.clone();
    next.language = "fr".into();
    assert_eq!(next.refusal(&odd), None);
}

#[test]
fn a_download_folder_from_home_is_expanded_and_a_relative_one_is_not_kept() {
    let at = |typed: &str| {
        Settings {
            download_dir: typed.into(),
            ..Default::default()
        }
        .clamped()
        .download_dir
    };
    let home = dirs::home_dir().unwrap();
    assert_eq!(at("~/Downloads"), home.join("Downloads"));
    assert_eq!(at("Downloads"), Settings::default().download_dir);
}

#[test]
fn a_folder_for_one_download_is_made_full_or_refused() {
    let home = dirs::home_dir().unwrap();
    let caller = std::env::temp_dir().join("caller");
    assert_eq!(full_folder("~/x", None), Some(home.join("x")));
    assert_eq!(full_folder("out", Some(&caller)), Some(caller.join("out")));
    assert_eq!(full_folder("out", None), None);
    assert_eq!(full_folder("~someone/x", Some(&caller)), None);
    let absolute = caller.join("abs");
    assert_eq!(
        full_folder(&absolute.display().to_string(), None),
        Some(absolute)
    );
}

#[test]
fn no_settings_file_is_not_a_problem() {
    let dir = tempfile::tempdir().unwrap();
    let (_, notice) = Settings::load_from(&dir.path().join("settings.json"));
    assert!(notice.is_none());
}

#[test]
fn values_outside_what_the_engine_accepts_are_brought_back_in() {
    let wild = Settings {
        connections: 5000,
        concurrency: 0,
        ..Default::default()
    };
    let sane = wild.clamped();
    assert_eq!(sane.connections, 32);
    assert_eq!(sane.concurrency, 1);
}

#[test]
fn a_parts_folder_is_a_full_path_or_dms_own() {
    let at = |typed: &str| {
        Settings {
            parts_dir: typed.into(),
            ..Default::default()
        }
        .clamped()
        .parts_dir
    };
    let home = dirs::home_dir().unwrap();
    assert_eq!(
        at("~/dm-parts"),
        home.join("dm-parts").display().to_string()
    );
    assert_eq!(at("parts"), "", "relative");
    assert_eq!(at("~someone/x"), "", "another user's home is not read");
    assert_eq!(at("  "), "");
    let absolute = std::env::temp_dir().join("dm-parts").display().to_string();
    assert_eq!(at(&absolute), absolute);
}

/// The click keys are ones the extension knows: anything else in a
/// hand-edited file is what they ship as, and "none" stays none.
#[test]
fn the_click_keys_are_ones_the_extension_knows() {
    let mut catch = dm_protocol::CatchRules {
        keep_key: "hyper".into(),
        take_key: "f13".into(),
        ..Default::default()
    };
    let odd = Settings {
        catch: catch.clone(),
        ..Settings::default()
    }
    .clamped();
    assert_eq!(
        (odd.catch.keep_key.as_str(), odd.catch.take_key.as_str()),
        ("alt", "shift")
    );
    catch.keep_key = "alt".into();
    catch.take_key = String::new();
    let none = Settings {
        catch,
        ..Settings::default()
    };
    assert_eq!(none.clamped().catch.take_key, "");
}

/// Settings carried to another computer leave every password behind,
/// and the one they arrive on keeps its own.
#[test]
fn a_queue_start_must_be_a_time() {
    let before = Settings::default();
    let mut wanted = before.clone();
    wanted.queue_starts.push(QueueStart {
        queue: "Night".into(),
        at: "25:00".into(),
    });
    assert!(wanted.refusal(&before).is_some());
    wanted.queue_starts[0].at = "02:30".into();
    assert!(wanted.refusal(&before).is_none());
    assert_eq!(wanted.queue_start_list(), [("Night".to_string(), 150)]);
}

#[test]
fn exported_settings_carry_no_password_and_imported_ones_keep_this_computers() {
    let kept = crate::test_secrets::secret("password");
    let proxy_kept = crate::test_secrets::secret("proxy");
    let mut here = Settings {
        site_logins: vec![SiteLogin {
            host: "example.com".into(),
            username: "me".into(),
            password: kept.clone(),
        }],
        recent_urls: vec!["https://e.com/a.zip".into()],
        last_folders: [("Archives".to_string(), "/home/me/private zips".to_string())].into(),
        connections: 5,
        ..Settings::default()
    };
    here.proxy.username = "proxyuser".into();
    here.proxy.password = proxy_kept.clone();
    here.on_complete = "their-program --flag".into();
    let file = here.exported();
    assert!(!file.contains(&kept), "a password left in the file");
    assert!(!file.contains("e.com/a.zip"), "history left in the file");
    assert!(
        !file.contains("private zips"),
        "a folder used left in the file"
    );
    let mut elsewhere = Settings {
        site_logins: here.site_logins.clone(),
        ..Settings::default()
    };
    elsewhere.proxy.username = "proxyuser".into();
    elsewhere.proxy.password = proxy_kept.clone();
    let read = elsewhere.imported(&file).unwrap();
    assert_eq!(read.connections, 5);
    assert_eq!(read.site_logins[0].password, kept);
    assert_eq!(read.proxy.password, proxy_kept);
    // A command to run is never taken from a file.
    assert_eq!(read.on_complete, elsewhere.on_complete);
    assert!(elsewhere.imported("not settings").is_err());
}

#[test]
fn a_recording_limit_is_minutes_held_to_a_day_and_none_when_zero() {
    let s = Settings {
        record_minutes: 99_999,
        ..Default::default()
    }
    .clamped();
    assert_eq!(s.record_minutes, 1440);
    let options =
        |s: &Settings| s.options(std::path::PathBuf::from("/tmp/x"), Limiter::unlimited());
    assert_eq!(
        options(&s).record_limit,
        Some(std::time::Duration::from_secs(1440 * 60))
    );
    assert_eq!(options(&Settings::default()).record_limit, None);
}

#[test]
fn attempts_timeouts_and_site_limits_are_held_to_what_the_engine_takes() {
    let wild = Settings {
        attempts: 5000,
        read_timeout: 1,
        host_limits: vec![
            dm_core::HostLimit {
                host: "  Example.COM ".into(),
                connections: 0,
            },
            dm_core::HostLimit {
                host: "   ".into(),
                connections: 4,
            },
        ],
        ..Default::default()
    }
    .clamped();
    assert_eq!(wild.attempts, 100);
    assert_eq!(wild.read_timeout, 5);
    assert_eq!(
        wild.host_limits,
        [dm_core::HostLimit {
            host: "example.com".into(),
            connections: 1
        }],
        "a limit naming no site is dropped"
    );
    // Zero is "keep trying", and is kept.
    assert_eq!(
        Settings {
            attempts: 0,
            ..Default::default()
        }
        .clamped()
        .attempts,
        0
    );
    let d = Settings::default();
    assert_eq!((d.attempts, d.read_timeout), (10, 30));
    assert_eq!(d.options(PathBuf::new(), Limiter::unlimited()).attempts, 10);
}

#[test]
fn an_empty_folder_falls_back_rather_than_saving_to_nowhere() {
    let empty = Settings {
        download_dir: PathBuf::new(),
        ..Default::default()
    };
    assert!(!empty.clamped().download_dir.as_os_str().is_empty());
}

#[test]
fn a_short_folder_list_is_filled_out_rather_than_left_ragged() {
    // The engine indexes this by category; a file edited by hand can
    // easily be short, and the kinds past its end would silently stop
    // being sorted.
    let short = Settings {
        category_folders: vec!["Videos".into()],
        ..Default::default()
    }
    .clamped();
    assert_eq!(short.category_folders.len(), Category::ALL.len());
    assert_eq!(short.category_folders[0], "Videos");
}

#[test]
fn the_speed_limit_is_read_in_the_unit_the_engine_counts() {
    let settings = Settings {
        speed_limit: 500,
        ..Default::default()
    };
    assert_eq!(settings.speed_limit_bytes(), 500 * 1024);
    assert_eq!(Settings::default().speed_limit_bytes(), 0, "no limit");
}

#[test]
fn a_quality_is_read_as_the_engine_means_it() {
    let at = |q: &str| {
        Settings {
            stream_quality: q.into(),
            ..Default::default()
        }
        .quality()
    };

    assert_eq!(at("best"), Quality::Best);
    assert_eq!(at("audio"), Quality::Audio);
    assert_eq!(at("720"), Quality::UpTo(720));
    // A typo takes the best on offer rather than the smallest, which is
    // what a value of zero would otherwise mean.
    assert_eq!(at("hd"), Quality::Best);
    assert_eq!(at(""), Quality::Best);
}

#[test]
fn settings_written_by_an_earlier_version_still_read() {
    /* The file on disk outlives the version that wrote it, and a queue
     * that refuses to start because a field is missing is a worse
     * outcome than one that fills in a default. */
    let old = r#"{"connections":4,"concurrency":2,"download_dir":"/tmp/x"}"#;
    let settings: Settings = serde_json::from_str(old).unwrap();
    assert_eq!(settings.connections, 4);
    assert_eq!(settings.speed_limit, 0);
    assert!(
        settings.categorize,
        "sorting by kind is on unless turned off"
    );
    assert!(!settings.schedule.enabled);
    assert!(!settings.proxy.enabled);
    assert_eq!(settings.quality(), Quality::Best);
}

#[test]
fn the_extension_catches_the_default_file_types_until_told_otherwise() {
    // A settings file from before the extension asked the app has no
    // `catch`; the browser must still hand over the default file types.
    let old: Settings = serde_json::from_str(r#"{"connections":4}"#).unwrap();
    assert!(old.catch.enabled);
    for kind in ["zip", "exe", "mp4", "pdf", "iso"] {
        assert!(old.catch.extensions.iter().any(|e| e == kind), "{kind}");
    }

    let chosen: Settings = serde_json::from_str(
        r#"{"catch":{"enabled":false,"extensions":["bin"],"minSizeKb":5,"skipHosts":["e.com"]}}"#,
    )
    .unwrap();
    assert!(!chosen.catch.enabled);
    assert_eq!(chosen.catch.extensions, vec!["bin".to_string()]);
    assert_eq!(chosen.catch.min_size_kb, 5);
    let saved = serde_json::to_value(&chosen).unwrap();
    assert_eq!(saved["catch"]["skipHosts"][0], "e.com");
}

#[test]
fn the_defaults_are_already_within_bounds() {
    let d = Settings::default();
    let c = d.clone().clamped();
    assert_eq!(d.connections, c.connections);
    assert_eq!(d.concurrency, c.concurrency);
}

#[test]
fn a_site_rule_gives_its_sites_downloads_a_folder_and_a_speed_unless_chosen_otherwise() {
    let dir = std::env::temp_dir().join("dm-rule-folder");
    let settings = Settings {
        site_rules: vec![
            SiteRule {
                host: "Example.COM ".into(),
                folder: dir.display().to_string(),
                speed: 200,
            },
            SiteRule {
                host: "dl.example.com".into(),
                folder: String::new(),
                speed: 50,
            },
            SiteRule {
                host: "  ".into(),
                folder: "/never".into(),
                speed: 1,
            },
            SiteRule {
                host: "relative.test".into(),
                folder: "not/absolute".into(),
                speed: 5_000_000,
            },
        ],
        ..Default::default()
    }
    .clamped();
    // Tidied: the nameless one is gone, a relative folder is none, the
    // speed is held to the setting's range, the host is plain.
    assert_eq!(settings.site_rules.len(), 3);
    assert_eq!(settings.site_rules[0].host, "example.com");
    assert_eq!(settings.site_rules[2].folder, "");
    assert_eq!(settings.site_rules[2].speed, 1024 * 1024);

    // The site and its subdomains; the most specific host wins.
    let rule = |url: &str| settings.site_rule_for(url).map(|r| r.speed);
    assert_eq!(rule("https://example.com/a.zip"), Some(200));
    assert_eq!(rule("https://cdn.example.com/a.zip"), Some(200));
    assert_eq!(rule("https://dl.example.com/a.zip"), Some(50));
    assert_eq!(rule("https://notexample.com/a.zip"), None);

    let plain = settings.with_site_rule("https://example.com/a.zip", AddOptions::default());
    assert_eq!(plain.save_dir.as_deref(), Some(dir.to_str().unwrap()));
    assert_eq!(plain.speed_limit, 200);
    // What a person chose for this one download stands.
    let chosen = settings.with_site_rule(
        "https://example.com/a.zip",
        AddOptions {
            save_dir: Some("/mine".into()),
            speed_limit: 7,
            ..Default::default()
        },
    );
    assert_eq!(chosen.save_dir.as_deref(), Some("/mine"));
    assert_eq!(chosen.speed_limit, 7);
    // And another site is left as it is.
    let other = settings.with_site_rule("https://elsewhere.org/a.zip", AddOptions::default());
    assert!(other.save_dir.is_none() && other.speed_limit == 0);
}

#[test]
fn the_new_link_offer_is_on_for_an_old_file_and_kept_when_turned_off() {
    let old: Settings = serde_json::from_str("{}").unwrap();
    assert!(old.offer_new_link);
    let text = serde_json::to_string(&Settings {
        offer_new_link: false,
        ..Settings::default()
    })
    .unwrap();
    let back: Settings = serde_json::from_str(&text).unwrap();
    assert!(!back.offer_new_link);
}
