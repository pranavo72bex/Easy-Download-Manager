// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

#[test]
fn a_queue_with_a_schedule_of_its_own_uses_it_and_the_rest_use_the_shared_one() {
    let mut settings = Settings::default();
    settings.schedule.from = "01:00".into();
    settings.queue_schedules.insert(
        "Night".into(),
        Schedule {
            from: "23:00".into(),
            ..Schedule::default()
        },
    );
    assert_eq!(settings.schedule_for("Night").from, "23:00");
    assert_eq!(settings.schedule_for("Main").from, "01:00");

    let saved = serde_json::to_string(&settings).unwrap();
    let read: Settings = serde_json::from_str(&saved).unwrap();
    assert_eq!(read.schedule_for("Night").from, "23:00");
    let old: Settings = serde_json::from_str("{}").unwrap_or_default();
    assert!(old.queue_schedules.is_empty());
}

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
        settings.downloads.connections,
        Settings::default().clamped().downloads.connections
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

/// A settings file written with the mark some editors put before UTF-8 is read as any other,
/// not set aside (gaps row 212).
#[test]
fn a_settings_file_with_a_byte_order_mark_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(
        &path,
        "\u{feff}{ \"connections\": 4, \"theme\": \"light\" }",
    )
    .unwrap();

    let (settings, notice) = Settings::load_from(&path);
    assert_eq!(notice, None);
    assert_eq!(
        (
            settings.downloads.connections,
            settings.general.theme.as_str()
        ),
        (4, "light")
    );
    assert!(path.exists());
    assert!(!dir.path().join("settings.json.unreadable").exists());
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
    changed.downloads.connections = 3;
    assert_eq!(changed.refusal(&read), None);
    changed.custom_categories.push(dm_core::Custom {
        name: "Docs: work".into(),
        ..Default::default()
    });
    assert!(changed
        .refusal(&read)
        .is_some_and(|r| r.contains("Windows")));
    let mut renamed = read.clone();
    renamed.folders.category_folders[1] = "CON".into();
    assert!(renamed.refusal(&read).is_some());
}

#[test]
fn one_odd_entry_costs_that_entry_and_not_every_setting() {
    /* A file edited by hand, or written by a later dm: a proxy type this
     * one does not know, a schedule and limits missing fields, a login
     * with no password, and a number written as a word. None of these
     * moves the whole file aside and starts from the defaults. */
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
    assert_eq!(read.downloads.connections, 4);
    assert_eq!(read.general.language, "de");
    assert!(
        !read.advanced.proxy.enabled,
        "an unknown proxy type is off, not another kind"
    );
    assert_eq!(read.advanced.proxy.host, "p.example");
    assert!(read.schedule.enabled);
    assert_eq!(read.schedule.from, "02:00");
    assert_eq!(read.schedule.to, Schedule::default().to);
    assert_eq!(read.advanced.host_limits[0].host, "slow.example");
    assert_eq!(read.advanced.host_limits[0].connections, 1);
    assert_eq!(read.downloads.queue_limits[0].queue, "Nightly");
    assert_eq!(
        read.downloads.queue_limits[0].files, 1,
        "clamped up from none"
    );
    assert_eq!(read.site_logins[0].username, "me");
    assert_eq!(read.custom_categories[0].name, "Books");
    assert_eq!(
        read.downloads.concurrency,
        Settings::default().downloads.concurrency
    );
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
    assert!(!read.advanced.proxy.enabled);
    assert_eq!(read.site_logins.len(), 1);
    assert_eq!(read.downloads.queue_limits[0].queue, "Nightly");
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
    assert!(with(|s| s.folders.download_dir = "Downloads".into()).is_some());
    assert!(with(|s| s.folders.download_dir = "~/Downloads".into()).is_none());
    assert!(with(|s| s.folders.parts_dir = "parts".into()).is_some());
    assert!(with(|s| s.custom_categories = vec![dm_core::Custom {
        name: "Work/Docs".into(),
        ..Default::default()
    }])
    .is_some());
    assert!(with(|s| s.advanced.cookies_file = "/no/such/cookies.txt".into()).is_some());
    assert!(with(|s| s.general.language = "de".into()).is_none());

    // Saved before, and so not this change's doing.
    let odd = Settings {
        advanced: AdvancedSettings {
            cookies_file: "/no/such/cookies.txt".into(),
            ..before.advanced.clone()
        },
        ..before.clone()
    };
    let mut next = odd.clone();
    next.general.language = "fr".into();
    assert_eq!(next.refusal(&odd), None);
}

#[test]
fn a_download_folder_from_home_is_expanded_and_a_relative_one_is_not_kept() {
    let at = |typed: &str| {
        Settings {
            folders: FolderSettings {
                download_dir: typed.into(),
                ..Default::default()
            },
            ..Default::default()
        }
        .clamped()
        .folders
        .download_dir
    };
    let home = dm_core::folders::home().unwrap();
    assert_eq!(at("~/Downloads"), home.join("Downloads"));
    assert_eq!(at("Downloads"), Settings::default().folders.download_dir);
}

#[test]
fn a_folder_for_one_download_is_made_full_or_refused() {
    let home = dm_core::folders::home().unwrap();
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
        downloads: DownloadSettings {
            connections: 5000,
            concurrency: 0,
            ..Default::default()
        },
        ..Default::default()
    };
    let sane = wild.clamped();
    assert_eq!(sane.downloads.connections, 32);
    assert_eq!(sane.downloads.concurrency, 1);
}

#[test]
fn a_parts_folder_is_a_full_path_or_dms_own() {
    let at = |typed: &str| {
        Settings {
            folders: FolderSettings {
                parts_dir: typed.into(),
                ..Default::default()
            },
            ..Default::default()
        }
        .clamped()
        .folders
        .parts_dir
    };
    let home = dm_core::folders::home().unwrap();
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
        browser: BrowserSettings {
            catch: catch.clone(),
        },
        ..Settings::default()
    }
    .clamped();
    assert_eq!(
        (
            odd.browser.catch.keep_key.as_str(),
            odd.browser.catch.take_key.as_str()
        ),
        ("alt", "shift")
    );
    catch.keep_key = "alt".into();
    catch.take_key = String::new();
    let none = Settings {
        browser: BrowserSettings { catch },
        ..Settings::default()
    };
    assert_eq!(none.clamped().browser.catch.take_key, "");
}

/// Settings carried to another computer leave every password behind,
/// and the one they arrive on keeps its own.
#[test]
fn a_queue_start_must_be_a_time() {
    let before = Settings::default();
    let mut wanted = before.clone();
    wanted.downloads.queue_starts.push(QueueStart {
        queue: "Night".into(),
        at: "25:00".into(),
    });
    assert!(wanted.refusal(&before).is_some());
    wanted.downloads.queue_starts[0].at = "02:30".into();
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
        downloads: DownloadSettings {
            connections: 5,
            ..Default::default()
        },
        ..Settings::default()
    };
    here.advanced.proxy.username = "proxyuser".into();
    here.advanced.proxy.password = proxy_kept.clone();
    here.downloads.on_complete = "their-program --flag".into();
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
    elsewhere.advanced.proxy.username = "proxyuser".into();
    elsewhere.advanced.proxy.password = proxy_kept.clone();
    let read = elsewhere.imported(&file).unwrap();
    assert_eq!(read.downloads.connections, 5);
    assert_eq!(read.site_logins[0].password, kept);
    assert_eq!(read.advanced.proxy.password, proxy_kept);
    // A command to run is never taken from a file.
    assert_eq!(read.downloads.on_complete, elsewhere.downloads.on_complete);
    assert!(elsewhere.imported("not settings").is_err());
}

#[test]
fn a_recording_limit_is_minutes_held_to_a_day_and_none_when_zero() {
    let s = Settings {
        downloads: DownloadSettings {
            record_minutes: 99_999,
            ..Default::default()
        },
        ..Default::default()
    }
    .clamped();
    assert_eq!(s.downloads.record_minutes, 1440);
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
        advanced: AdvancedSettings {
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
        },
        ..Default::default()
    }
    .clamped();
    assert_eq!(wild.advanced.attempts, 100);
    assert_eq!(wild.advanced.read_timeout, 5);
    assert_eq!(
        wild.advanced.host_limits,
        [dm_core::HostLimit {
            host: "example.com".into(),
            connections: 1
        }],
        "a limit naming no site is dropped"
    );
    // Zero is "keep trying", and is kept.
    assert_eq!(
        Settings {
            advanced: AdvancedSettings {
                attempts: 0,
                ..Default::default()
            },
            ..Default::default()
        }
        .clamped()
        .advanced
        .attempts,
        0
    );
    let d = Settings::default();
    assert_eq!((d.advanced.attempts, d.advanced.read_timeout), (15, 30));
    assert_eq!(d.options(PathBuf::new(), Limiter::unlimited()).attempts, 15);
}

#[test]
fn an_empty_folder_falls_back_rather_than_saving_to_nowhere() {
    let empty = Settings {
        folders: FolderSettings {
            download_dir: PathBuf::new(),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(!empty.clamped().folders.download_dir.as_os_str().is_empty());
}

#[test]
fn a_short_folder_list_is_filled_out_rather_than_left_ragged() {
    // The engine indexes this by category; a file edited by hand can
    // easily be short, and the kinds past its end would silently stop
    // being sorted.
    let short = Settings {
        folders: FolderSettings {
            category_folders: vec!["Videos".into()],
            ..Default::default()
        },
        ..Default::default()
    }
    .clamped();
    assert_eq!(short.folders.category_folders.len(), Category::ALL.len());
    assert_eq!(short.folders.category_folders[0], "Videos");
}

#[test]
fn the_speed_limit_is_read_in_the_unit_the_engine_counts() {
    let settings = Settings {
        advanced: AdvancedSettings {
            speed_limit: 500,
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(settings.speed_limit_bytes(), 500 * dm_core::KB);
    assert_eq!(Settings::default().speed_limit_bytes(), 0, "no limit");
}

#[test]
fn a_quality_is_read_as_the_engine_means_it() {
    let at = |q: &str| {
        Settings {
            downloads: DownloadSettings {
                stream_quality: q.into(),
                ..Default::default()
            },
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
    assert_eq!(settings.downloads.connections, 4);
    assert_eq!(settings.advanced.speed_limit, 0);
    assert!(
        settings.folders.categorize,
        "sorting by kind is on unless turned off"
    );
    assert!(!settings.schedule.enabled);
    // A file that never named a proxy follows the computer's, as a new one does.
    assert!(
        settings.advanced.proxy.enabled
            && settings.advanced.proxy.kind == dm_core::ProxyKind::System
    );
    assert_eq!(settings.quality(), Quality::Best);
}

#[test]
fn the_extension_catches_every_file_type_until_told_otherwise() {
    // A settings file from before the extension asked the app has no
    // `catch`; the browser must still hand over every download.
    let old: Settings = serde_json::from_str(r#"{"connections":4}"#).unwrap();
    assert!(old.browser.catch.enabled);
    assert!(
        old.browser.catch.extensions.is_empty(),
        "no list is every type"
    );

    let chosen: Settings = serde_json::from_str(
        r#"{"catch":{"enabled":false,"extensions":["bin"],"minSizeKb":5,"skipHosts":["e.com"]}}"#,
    )
    .unwrap();
    assert!(!chosen.browser.catch.enabled);
    assert_eq!(chosen.browser.catch.extensions, vec!["bin".to_string()]);
    assert_eq!(chosen.browser.catch.min_size_kb, 5);
    let saved = serde_json::to_value(&chosen).unwrap();
    assert_eq!(saved["catch"]["skipHosts"][0], "e.com");
}

#[test]
fn the_defaults_are_already_within_bounds() {
    let d = Settings::default();
    let c = d.clone().clamped();
    assert_eq!(d.downloads.connections, c.downloads.connections);
    assert_eq!(d.downloads.concurrency, c.downloads.concurrency);
}

#[test]
fn a_site_rule_gives_its_sites_downloads_a_folder_and_a_speed_unless_chosen_otherwise() {
    let dir = std::env::temp_dir().join("dm-rule-folder");
    let settings = Settings {
        advanced: AdvancedSettings {
            site_rules: vec![
                SiteRule {
                    host: "Example.COM ".into(),
                    folder: dir.display().to_string(),
                    speed: 200,
                    ..Default::default()
                },
                SiteRule {
                    host: "dl.example.com".into(),
                    folder: String::new(),
                    speed: 50,
                    ..Default::default()
                },
                SiteRule {
                    host: "  ".into(),
                    folder: "/never".into(),
                    speed: 1,
                    ..Default::default()
                },
                SiteRule {
                    host: "relative.test".into(),
                    folder: "not/absolute".into(),
                    speed: 5_000_000,
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
        ..Default::default()
    }
    .clamped();
    // Tidied: the nameless one is gone, a relative folder is none, the
    // speed is held to the setting's range, the host is plain.
    assert_eq!(settings.advanced.site_rules.len(), 3);
    assert_eq!(settings.advanced.site_rules[0].host, "example.com");
    assert_eq!(settings.advanced.site_rules[2].folder, "");
    assert_eq!(settings.advanced.site_rules[2].speed, 1024 * 1024);

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

/// gaps row F1: a site's rule names the `User-Agent` and headers its downloads go with; a
/// header the browser (or the person) already gave stands over the rule's.
#[test]
fn a_site_rules_agent_and_headers_join_a_downloads_own() {
    let settings = Settings {
        advanced: AdvancedSettings {
            site_rules: vec![SiteRule {
                host: "example.com".into(),
                agent: " Rule/1 ".into(),
                headers: "X-Api-Key: k1 | Authorization: Basic rule | user-agent: ignored".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
        ..Settings::default()
    };
    let pair = |n: &str, v: &str| (n.to_string(), v.to_string());
    let with = settings.with_site_rule(
        "https://dl.example.com/a.zip",
        AddOptions {
            headers: vec![pair("Authorization", "Bearer browser")],
            ..Default::default()
        },
    );
    assert_eq!(
        with.headers,
        vec![
            pair("Authorization", "Bearer browser"),
            pair("X-Api-Key", "k1"),
            pair("User-Agent", "Rule/1"),
        ]
    );
    assert_eq!(
        settings.site_headers("https://elsewhere.org/a.zip", &[]),
        Vec::new()
    );
    // Kept and read back as written.
    let text = serde_json::to_string(&settings).unwrap();
    let back: Settings = serde_json::from_str(&text).unwrap();
    assert_eq!(back.advanced.site_rules, settings.advanced.site_rules);
}

#[test]
fn the_new_link_offer_is_on_for_an_old_file_and_kept_when_turned_off() {
    let old: Settings = serde_json::from_str("{}").unwrap();
    assert!(old.general.offer_new_link);
    let text = serde_json::to_string(&Settings {
        general: GeneralSettings {
            offer_new_link: false,
            ..Default::default()
        },
        ..Settings::default()
    })
    .unwrap();
    let back: Settings = serde_json::from_str(&text).unwrap();
    assert!(!back.general.offer_new_link);
}

fn saved_with_catch(extensions: &str, version: &str) -> Settings {
    let text = format!(r#"{{"catch": {{"extensions": {extensions}{version}}}}}"#);
    serde_json::from_str::<Settings>(&text).unwrap().clamped()
}

/// A catch list saved before documents joined the defaults has them now, so
/// a .docx is not left to the browser for good; nothing the person had is
/// lost.
#[test]
fn a_catch_list_saved_before_new_default_types_gets_them_once() {
    let settings = saved_with_catch(r#"["zip", "xyz"]"#, "");
    for kind in ["zip", "xyz", "docx", "xlsx", "dmg"] {
        assert!(
            settings.browser.catch.extensions.iter().any(|k| k == kind),
            "{kind}"
        );
    }
    assert_eq!(
        settings.browser.catch.defaults_version,
        dm_protocol::CATCH_DEFAULTS_VERSION
    );
}

/// Once brought up to date, a type taken out stays out; and a list that
/// means every type (empty) is not turned into a short one.
#[test]
fn a_catch_list_brought_up_to_date_is_left_as_the_person_made_it() {
    let version = format!(
        r#", "defaultsVersion": {}"#,
        dm_protocol::CATCH_DEFAULTS_VERSION
    );
    let settings = saved_with_catch(r#"["zip"]"#, &version);
    assert_eq!(settings.browser.catch.extensions, ["zip"]);
    let every = saved_with_catch("[]", "");
    assert!(every.browser.catch.extensions.is_empty());
}

/// Reference A's pop-ups, every one on (edition 2): a file from an older
/// edition, whose `false` was dm's old default and not a choice, takes them
/// once; a choice made since (the edition is saved with it) stands.
#[test]
fn pop_ups_from_an_older_edition_take_reference_as_defaults_once() {
    let old = serde_json::from_str::<Settings>(
        r#"{"popups_version": 1, "show_progress": false, "show_complete": false,
            "ask_caught_downloads": false, "ask_panel_downloads": false}"#,
    )
    .unwrap()
    .clamped();
    assert!(old.general.show_progress && old.general.show_complete);
    assert!(old.general.ask_caught_downloads && old.general.ask_panel_downloads);
    assert_eq!(old.popups_version, POPUPS_VERSION);
    // Turned off afterwards and saved: kept through the next read.
    let mut chosen = old.clone();
    chosen.general.show_progress = false;
    chosen.general.ask_caught_downloads = false;
    let text = serde_json::to_string(&chosen).unwrap();
    let again = serde_json::from_str::<Settings>(&text).unwrap().clamped();
    assert!(!again.general.show_progress && again.general.show_complete);
    assert!(!again.general.ask_caught_downloads && again.general.ask_panel_downloads);
    // A fresh install has them all on.
    let fresh = Settings::default();
    assert!(fresh.general.show_progress && fresh.general.show_complete);
    assert!(fresh.general.ask_caught_downloads && fresh.general.ask_panel_downloads);
}

/// A store that refuses everything, for the fallback.
struct Refusing;

impl dm_core::secretstore::SecretStore for Refusing {
    fn get(&self, _: &str) -> Result<Option<Vec<u8>>, String> {
        Err("locked".into())
    }
    fn put(&self, _: &str, _: &[u8]) -> Result<(), String> {
        Err("locked".into())
    }
    fn delete(&self, _: &str) -> Result<(), String> {
        Err("locked".into())
    }
}

/// Made up for the run, never written in the code (a scanner read the
/// literals these tests had as real passwords).
fn site_password() -> String {
    crate::test_secrets::secret("site_password")
}

fn proxy_password() -> String {
    crate::test_secrets::secret("proxy_password")
}

fn with_passwords() -> Settings {
    let mut settings = Settings {
        site_logins: vec![SiteLogin {
            host: "example.com".into(),
            username: "ann".into(),
            password: site_password(),
        }],
        ..Settings::default()
    };
    settings.advanced.proxy.username = "bob".into();
    settings.advanced.proxy.password = proxy_password();
    settings
}

#[test]
fn passwords_go_to_the_store_and_the_file_keeps_none() {
    let store = dm_core::secretstore::MemoryStore::default();
    let mut file = with_passwords();
    assert_eq!(file.move_passwords_to(&store, "ns"), None);
    assert!(file.site_logins[0].password.is_empty() && file.advanced.proxy.password.is_empty());
    let text = serde_json::to_string(&file).unwrap();
    assert!(!text.contains(&site_password()) && !text.contains(&proxy_password()));

    // Read back: an empty field is looked up in the store.
    let mut read: Settings = serde_json::from_str(&text).unwrap();
    assert_eq!(read.take_passwords_from(&store, "ns"), (false, None));
    assert_eq!(read.site_logins[0].password, site_password());
    assert_eq!(read.advanced.proxy.password, proxy_password());

    // Another home's names are not this one's.
    let mut other: Settings = serde_json::from_str(&text).unwrap();
    other.take_passwords_from(&store, "elsewhere");
    assert!(other.site_logins[0].password.is_empty());
}

#[test]
fn a_file_with_clear_passwords_is_moved_into_the_store_on_its_first_read() {
    let store = dm_core::secretstore::MemoryStore::default();
    let mut read = with_passwords();
    let (moved, note) = read.take_passwords_from(&store, "ns");
    assert!(moved && note.is_none());
    assert_eq!(
        read.site_logins[0].password,
        site_password(),
        "in memory as before"
    );
    // What is written next carries none, and a later read finds them in the store.
    let mut next = read.clone();
    next.move_passwords_to(&store, "ns");
    let mut later: Settings = serde_json::from_str(&serde_json::to_string(&next).unwrap()).unwrap();
    later.take_passwords_from(&store, "ns");
    assert_eq!(later.advanced.proxy.password, proxy_password());
}

/// An empty password is forgotten in the store when the person cleared it, and kept when this
/// run never read it back: a settings file read in part, or a store that would not answer at
/// start (security review).
#[test]
fn a_password_this_run_did_not_read_back_is_not_forgotten_by_the_next_save() {
    let store = dm_core::secretstore::MemoryStore::default();
    let mut first = with_passwords();
    first.move_passwords_to(&store, "ns");

    let mut fallback = Settings {
        keep_stored_passwords: true,
        ..Settings::default()
    };
    fallback.move_passwords_to(&store, "ns");
    let mut read = with_passwords();
    read.advanced.proxy.password.clear();
    read.take_passwords_from(&store, "ns");
    assert_eq!(
        read.advanced.proxy.password,
        proxy_password(),
        "kept through the fallback"
    );

    // Cleared by the person, in a run that read it: forgotten.
    let mut cleared = with_passwords();
    cleared.advanced.proxy.password.clear();
    cleared.move_passwords_to(&store, "ns");
    let mut again = with_passwords();
    again.advanced.proxy.password.clear();
    again.take_passwords_from(&store, "ns");
    assert!(
        again.advanced.proxy.password.is_empty(),
        "the person's clearing stands"
    );

    // A file that cannot be read is a fallback; one that is read whole is not.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, "{ not json").unwrap();
    assert!(Settings::load_from(&path).0.keep_stored_passwords);
    std::fs::write(&path, serde_json::to_string(&Settings::default()).unwrap()).unwrap();
    assert!(!Settings::load_from(&path).0.keep_stored_passwords);
}

#[test]
fn a_store_that_refuses_leaves_the_passwords_in_the_file_and_says_so() {
    let mut file = with_passwords();
    let note = file.move_passwords_to(&Refusing, "ns").expect("a notice");
    assert!(note.contains("locked") && note.contains("settings file"));
    assert_eq!(file.site_logins[0].password, site_password());
    assert_eq!(file.advanced.proxy.password, proxy_password());

    let mut read = with_passwords();
    let (moved, note) = read.take_passwords_from(&Refusing, "ns");
    assert!(!moved && note.is_some());
    assert_eq!(
        read.advanced.proxy.password,
        proxy_password(),
        "nothing is lost"
    );
}

#[test]
fn a_proxy_password_stays_only_with_the_proxy_it_was_given_for() {
    let mut mine = Settings::default();
    mine.advanced.proxy.username = "corp".into();
    mine.advanced.proxy.host = "proxy.corp.example".into();
    mine.advanced.proxy.port = 3128;
    mine.advanced.proxy.password = "kept".into();
    let file = |host: &str, port: u16| {
        let mut other = Settings::default();
        other.advanced.proxy.username = "corp".into();
        other.advanced.proxy.host = host.into();
        other.advanced.proxy.port = port;
        serde_json::to_string(&other).unwrap()
    };
    let same = mine.imported(&file("proxy.corp.example", 3128)).unwrap();
    assert_eq!(same.advanced.proxy.password, "kept");
    let elsewhere = mine.imported(&file("evil.example", 3128)).unwrap();
    assert_eq!(elsewhere.advanced.proxy.password, "");
    let other_port = mine.imported(&file("proxy.corp.example", 8080)).unwrap();
    assert_eq!(other_port.advanced.proxy.password, "");
}

#[test]
fn a_settings_file_never_sets_the_program_run_at_the_end_of_the_schedule() {
    let mut mine = Settings::default();
    mine.schedule.run_when_done = "backup --quiet".into();
    let mut theirs = Settings::default();
    theirs.schedule.run_when_done = "evil".into();
    let read = mine
        .imported(&serde_json::to_string(&theirs).unwrap())
        .unwrap();
    assert_eq!(read.schedule.run_when_done, "backup --quiet");
}

#[test]
fn a_settings_file_never_sets_the_proxy_script_dm_runs() {
    let mut mine = Settings::default();
    mine.advanced.proxy.script = "https://proxy.corp.example/proxy.pac".into();
    let mut theirs = Settings::default();
    theirs.advanced.proxy.enabled = true;
    theirs.advanced.proxy.script = "http://evil.example/p.pac".into();
    let read = mine
        .imported(&serde_json::to_string(&theirs).unwrap())
        .unwrap();
    assert_eq!(
        read.advanced.proxy.script,
        "https://proxy.corp.example/proxy.pac"
    );
}

#[test]
fn a_settings_file_never_sets_the_program_run_at_the_end_of_a_queues_own_hours() {
    let mut mine = Settings::default();
    let kept = Schedule {
        run_when_done: "backup --quiet".into(),
        ..Schedule::default()
    };
    mine.queue_schedules.insert("Nightly".into(), kept);
    let mut theirs = Settings::default();
    for name in ["Nightly", "Main queue", "Added by the file"] {
        let schedule = Schedule {
            enabled: true,
            run_when_done: "sh -c 'evil'".into(),
            ..Schedule::default()
        };
        theirs.queue_schedules.insert(name.into(), schedule);
    }
    let read = mine
        .imported(&serde_json::to_string(&theirs).unwrap())
        .unwrap();
    assert_eq!(
        read.queue_schedules["Nightly"].run_when_done,
        "backup --quiet"
    );
    assert_eq!(read.queue_schedules["Main queue"].run_when_done, "");
    assert_eq!(read.queue_schedules["Added by the file"].run_when_done, "");
}

#[test]
fn a_sound_file_chosen_for_an_event_is_kept_and_an_old_file_has_none() {
    let mut settings = Settings::default();
    settings
        .sounds
        .insert("queue_done".into(), "/sounds/done.wav".into());
    let saved = serde_json::to_string(&settings).unwrap();
    let read: Settings = serde_json::from_str(&saved).unwrap();
    assert_eq!(
        read.sounds.get("queue_done").map(String::as_str),
        Some("/sounds/done.wav")
    );
    let old: Settings = serde_json::from_str("{}").unwrap_or_default();
    assert!(old.sounds.is_empty(), "no entry keeps the system's sound");
}

/// The browser's host reads settings as the window does: one odd field
/// costs that field, not every choice, and the file is left as it was
/// (the host has no window to say it moved it).
#[test]
fn read_only_keeps_the_rest_of_a_file_with_one_odd_field_and_leaves_it_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let text = format!(
        r#"{{"connections": "eight", "ask_caught_downloads": false, "popups_version": {POPUPS_VERSION}}}"#
    );
    std::fs::write(&path, &text).unwrap();
    let read = Settings::read_only(&path);
    assert!(
        !read.general.ask_caught_downloads,
        "the person's choice was lost"
    );
    assert_eq!(
        read.downloads.connections,
        Settings::default().downloads.connections
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        text,
        "the file was touched"
    );
    assert!(!path.with_extension("json.unreadable").exists());
    // No file at all: the defaults.
    let none = Settings::read_only(&dir.path().join("absent.json"));
    assert_eq!(
        none.downloads.connections,
        Settings::default().downloads.connections
    );
}

/// The slow hours' limit is in force in its hours, the lower of the two,
/// and the speed limit outside them; past midnight too; off, never.
#[test]
fn the_slow_hours_lower_the_limit_in_their_hours_only() {
    let mut s = Settings {
        slow_hours: SlowHours {
            enabled: true,
            from: "22:00".into(),
            to: "06:00".into(),
            limit: 100,
        },
        advanced: AdvancedSettings {
            speed_limit: 1000,
            ..Default::default()
        },
        ..Settings::default()
    };
    assert_eq!(
        s.speed_limit_bytes_at(1, 23 * 60),
        100 * dm_core::KB,
        "in its hours"
    );
    assert_eq!(
        s.speed_limit_bytes_at(2, 3 * 60),
        100 * dm_core::KB,
        "past midnight"
    );
    assert_eq!(
        s.speed_limit_bytes_at(2, 12 * 60),
        1000 * dm_core::KB,
        "outside"
    );
    // No speed limit the rest of the day: the slow one alone in its hours.
    s.advanced.speed_limit = 0;
    assert_eq!(s.speed_limit_bytes_at(1, 23 * 60), 100 * dm_core::KB);
    assert_eq!(s.speed_limit_bytes_at(1, 12 * 60), 0);
    // A speed limit lower than the slow one stands.
    s.advanced.speed_limit = 50;
    assert_eq!(s.speed_limit_bytes_at(1, 23 * 60), 50 * dm_core::KB);
    // Off, or with a time that is not one, never.
    s.slow_hours.enabled = false;
    assert_eq!(s.speed_limit_bytes_at(1, 23 * 60), 50 * dm_core::KB);
    s.advanced.speed_limit = 0;
    s.slow_hours.enabled = true;
    s.slow_hours.from = "25:00".into();
    assert_eq!(s.speed_limit_bytes_at(1, 23 * 60), 0);
    // An older settings file, without them, reads as off.
    let read: Settings = serde_json::from_str("{}").unwrap();
    assert!(!read.slow_hours.enabled);
}

#[test]
fn a_finished_torrent_is_kept_shared_only_when_turned_on() {
    let parts = std::path::PathBuf::from("/tmp/parts");
    let off = Settings::default();
    assert!(
        !off.torrent_share,
        "sharing sends from the person's connection: off"
    );
    assert!(off
        .options(parts.clone(), dm_core::Limiter::unlimited())
        .torrent
        .share
        .is_none());
    let on = Settings {
        torrent_share: true,
        torrent_share_ratio: 1000.0,
        torrent_share_minutes: 0,
        torrent_upload_limit: 50,
        ..Settings::default()
    };
    let share = on
        .options(parts, dm_core::Limiter::unlimited())
        .torrent
        .share
        .expect("turned on, a finished torrent is shared");
    assert_eq!(share.ratio, 100.0, "held to a ratio that ends");
    assert_eq!(share.minutes, 1, "held to at least a minute");
    assert_eq!(share.upload_kbps, 50);
    // A file from before it existed reads as off, with the usual ratio and time.
    let old: Settings = serde_json::from_str(r#"{"connections":4}"#).unwrap();
    assert!(!old.torrent_share);
    assert_eq!(old.torrent_share_ratio, 1.0);
    assert_eq!(old.torrent_share_minutes, 120);
}

fn dialup_password() -> String {
    crate::test_secrets::secret("dialup_password")
}

/// The dial-up connection's password is written nowhere unless "Save password" is on, as
/// reference A's: kept for this run only (gaps row 93, G84). On, it goes where the proxy's goes.
#[test]
fn a_dial_up_password_is_kept_only_when_save_password_is_on() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let store = dm_core::secretstore::MemoryStore::default();
    let mut settings = Settings::default();
    settings.advanced.dialup.enabled = true;
    settings.advanced.dialup.connection = "Office VPN".into();
    settings.advanced.dialup.password = dialup_password();

    // Off: neither the file nor the store has it, with a store or without one.
    for store in [None, Some(&store as &dyn dm_core::secretstore::SecretStore)] {
        settings.save_with(&path, store).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains(&dialup_password()),
            "written with Save password off"
        );
        assert!(text.contains("Office VPN"), "the connection itself is kept");
    }
    assert_eq!(
        settings.advanced.dialup.password,
        dialup_password(),
        "kept for this run"
    );
    let (mut read, _) = Settings::load_file(&path);
    read.take_passwords_from(&store, &dm_core::secretstore::namespace(&path));
    assert!(
        read.advanced.dialup.password.is_empty(),
        "the store kept it"
    );

    // On: into the store, the file keeping none, and read back from there.
    settings.advanced.dialup.save_password = true;
    settings.save_with(&path, Some(&store)).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        !text.contains(&dialup_password()),
        "in the file beside the store"
    );
    let (mut read, _) = Settings::load_file(&path);
    read.take_passwords_from(&store, &dm_core::secretstore::namespace(&path));
    assert_eq!(read.advanced.dialup.password, dialup_password());

    // Off again: what the store kept is forgotten at the next save.
    read.advanced.dialup.save_password = false;
    read.save_with(&path, Some(&store)).unwrap();
    let (mut again, _) = Settings::load_file(&path);
    again.take_passwords_from(&store, &dm_core::secretstore::namespace(&path));
    assert!(
        again.advanced.dialup.password.is_empty(),
        "forgotten once Save password was turned off"
    );
}

#[test]
fn a_close_or_double_click_this_dm_does_not_know_takes_the_default() {
    let odd = Settings {
        general: GeneralSettings {
            on_close: "minimise".into(),
            double_click: "explode".into(),
            ..Default::default()
        },
        ..Settings::default()
    }
    .clamped();
    assert_eq!(odd.general.on_close, "ask");
    assert_eq!(odd.general.double_click, "properties");
    let known = Settings {
        general: GeneralSettings {
            on_close: "tray".into(),
            double_click: "folder".into(),
            ..Default::default()
        },
        ..Settings::default()
    }
    .clamped();
    assert_eq!(
        (
            known.general.on_close.as_str(),
            known.general.double_click.as_str()
        ),
        ("tray", "folder")
    );
}

#[test]
fn a_cookies_file_that_is_a_folder_and_a_kinds_folder_are_judged() {
    let dir = tempfile::tempdir().unwrap();
    let before = Settings::default();
    let folder = Settings {
        advanced: AdvancedSettings {
            cookies_file: dir.path().display().to_string(),
            ..before.advanced.clone()
        },
        ..before.clone()
    };
    let why = folder.refusal(&before).unwrap();
    assert!(why.contains("could not be read"), "{why}");

    let file = dir.path().join("cookies.txt");
    std::fs::write(&file, "").unwrap();
    let readable = Settings {
        advanced: AdvancedSettings {
            cookies_file: file.display().to_string(),
            ..before.advanced.clone()
        },
        ..before.clone()
    };
    assert_eq!(readable.refusal(&before), None);

    let mut kinds = before.clone();
    kinds.folders.category_folders = vec!["Fine".into(), "a/b".into()];
    let why = kinds.refusal(&before).unwrap();
    assert!(why.contains("a/b"), "{why}");
    kinds.folders.category_folders = vec!["Fine".into(), String::new()];
    assert_eq!(kinds.refusal(&before), None);
}

#[test]
fn a_settings_file_that_cannot_be_moved_aside_is_said_and_left() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    // A folder where the copy would go, with something in it, so neither
    // a rename nor a copy can take its place.
    let aside = dir.path().join("settings.json.unreadable");
    std::fs::create_dir(&aside).unwrap();
    std::fs::write(aside.join("held"), "").unwrap();

    std::fs::write(&path, "5").unwrap();
    let (read, notice) = Settings::load_from(&path);
    assert_eq!(
        read.downloads.connections,
        Settings::default().clamped().downloads.connections
    );
    let notice = notice.unwrap();
    assert!(notice.contains("or moved aside"), "{notice}");
    assert!(path.exists(), "the file is not lost");

    std::fs::write(&path, r#"{"connections": 4, "concurrency": "three"}"#).unwrap();
    let (read, notice) = Settings::load_from(&path);
    assert_eq!(read.downloads.connections, 4);
    let notice = notice.unwrap();
    assert!(notice.contains("could not be kept aside"), "{notice}");
}

#[test]
fn a_proxy_or_a_list_read_in_parts_names_what_it_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{"proxy": {"enabled": false, "type": "quic", "host": "p.example", "port": "x", "script": 5},
            "site_logins": 5}"#,
    )
    .unwrap();
    let (read, notice) = Settings::load_from(&path);
    let notice = notice.unwrap();
    for named in ["proxy.type", "proxy.port", "proxy.script", "site_logins"] {
        assert!(notice.contains(named), "{named} not named: {notice}");
    }
    assert!(
        !notice.contains("so the proxy is off"),
        "it was off already: {notice}"
    );
    assert!(!read.advanced.proxy.enabled);
    assert_eq!(read.advanced.proxy.host, "p.example");
    assert_eq!(read.advanced.proxy.port, dm_core::Proxy::default().port);
    assert!(read.advanced.proxy.script.is_empty());
    assert!(read.site_logins.is_empty());

    std::fs::write(&path, r#"{"proxy": 5, "connections": 3}"#).unwrap();
    let (read, notice) = Settings::load_from(&path);
    assert!(notice.unwrap().contains("proxy"));
    assert_eq!(
        read.advanced.proxy,
        dm_core::Proxy::default(),
        "off, not a guess"
    );
    assert!(!read.advanced.proxy.enabled);
    assert_eq!(read.downloads.connections, 3);
}

/// The tests' saves never reach the real dm's home: a test that changed the
/// settings through the backend wrote over the person's own.
#[test]
fn a_test_of_this_crate_keeps_its_settings_out_of_the_real_home() {
    if dm_core::env::get_os("DM_HOME").is_none() {
        assert!(Settings::path().starts_with(std::env::temp_dir()));
        assert_ne!(
            home(),
            dm_core::folders::local_data()
                .unwrap_or_default()
                .join("dm-engine")
        );
    }
}

/// The window's scale (issue #32, the maintainer's slider):
/// four stops, 100% for a new install and for an older file, kept through a
/// save and a load, any other value read as the nearest stop, an older
/// file's names read as 80, 100 and 120, and View's steps held at 60 and
/// 120.
#[test]
fn the_text_size_is_one_of_four_stops_kept_and_stepped() {
    assert_eq!(TEXT_SIZE.stops, [0.6, 0.8, 1.0, 1.2]);
    assert_eq!(Settings::default().text_size, 1.0);
    let old: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(old.text_size, 1.0, "a file from before it is 100%");

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let large = Settings {
        text_size: 1.2,
        ..Settings::default()
    };
    large.save_to(&path).unwrap();
    let (read, _) = Settings::load_from(&path);
    assert_eq!(read.text_size, 1.2, "kept across launches");

    for (written, held) in [
        ("0.6", 0.6),
        ("0.85", 0.8),
        ("1.1", 1.0),
        ("9", 1.2),
        ("0.01", 0.6),
        ("-2", 0.6),
        ("80", 0.8),
        ("\"Small\"", 0.8),
        ("\"medium\"", 1.0),
        ("\"Large\"", 1.2),
        ("\"60%\"", 0.6),
        ("\"big\"", 1.0),
        ("null", 1.0),
        ("[1]", 1.0),
    ] {
        std::fs::write(
            &path,
            format!(r#"{{"text_size": {written}, "connections": 3}}"#),
        )
        .unwrap();
        let (read, _) = Settings::load_from(&path);
        assert_eq!(read.text_size, held, "{written}");
        assert_eq!(
            read.downloads.connections, 3,
            "{written}: the rest of the file stands"
        );
    }
    let odd = Settings {
        text_size: f32::NAN,
        ..Settings::default()
    };
    assert_eq!(odd.clamped().text_size, 1.0);

    assert_eq!(TEXT_SIZE.stepped(1.0, true), 1.2);
    assert_eq!(TEXT_SIZE.stepped(1.2, true), 1.2, "held at 120");
    assert_eq!(TEXT_SIZE.stepped(1.0, false), 0.8);
    assert_eq!(TEXT_SIZE.stepped(0.8, false), 0.6);
    assert_eq!(TEXT_SIZE.stepped(0.6, false), 0.6, "held at 60");
    assert_eq!(TEXT_SIZE.stepped(0.85, true), 1.0, "from the nearest stop");
    assert_eq!(TEXT_SIZE.percent(0.6), 60);
    assert_eq!(TEXT_SIZE.percent(1.2), 120);
    assert_eq!(TEXT_SIZE.index(1.0), 2);
}

/// The side panel's width (gaps row U12): 200 for a new install and for a file from before it,
/// so the window looks as it did; kept through a save and a load; a width dragged or typed past
/// 150 or 400 held there, a fraction rounded to a whole point, and one that is no number read
/// as the default with the rest of the file standing.
#[test]
fn the_side_panels_width_is_bounded_and_kept() {
    assert_eq!(SIDEBAR_WIDTH.default, 200.0);
    assert_eq!(Settings::default().sidebar_width, 200.0);
    let old: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(old.sidebar_width, 200.0, "a file from before it is 200");

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let wide = Settings {
        sidebar_width: 312.0,
        ..Settings::default()
    };
    wide.save_to(&path).unwrap();
    let (read, notice) = Settings::load_from(&path);
    assert_eq!(read.sidebar_width, 312.0, "kept across launches");
    assert_eq!(notice, None);

    for (written, held) in [
        ("150", 150.0),
        ("400", 400.0),
        ("10", 150.0),
        ("-5", 150.0),
        ("4000", 400.0),
        ("250.6", 251.0),
        ("\"wide\"", 200.0),
        ("null", 200.0),
    ] {
        std::fs::write(
            &path,
            format!(r#"{{"sidebar_width": {written}, "connections": 3}}"#),
        )
        .unwrap();
        let (read, _) = Settings::load_from(&path);
        assert_eq!(read.sidebar_width, held, "{written}");
        assert_eq!(
            read.downloads.connections, 3,
            "{written}: the rest of the file stands"
        );
    }

    for odd in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let odd = Settings {
            sidebar_width: odd,
            ..Settings::default()
        };
        assert_eq!(odd.clamped().sidebar_width, 200.0);
    }
    assert_eq!(SIDEBAR_WIDTH.held(149.4), 150.0);
    assert_eq!(SIDEBAR_WIDTH.held(400.4), 400.0);
    assert_eq!(SIDEBAR_WIDTH.held(222.2), 222.0);
}

/// The home folder is written as `~` where dm fills a folder in, and read back whole (gaps row
/// U17); a folder elsewhere, or one only beginning with the home's letters, is left as it is.
#[cfg(not(windows))]
#[test]
fn a_folder_in_home_is_written_with_a_tilde_and_read_back() {
    let home = dm_core::folders::home().unwrap();
    let inside = home.join("Downloads").join("Films");
    assert_eq!(
        home_short(&inside.display().to_string()),
        "~/Downloads/Films"
    );
    assert_eq!(full_path("~/Downloads/Films"), Some(inside));
    assert_eq!(home_short(&home.display().to_string()), "~");
    let beside = format!("{}2/x", home.display());
    assert_eq!(home_short(&beside), beside);
    assert_eq!(home_short("/tmp/x"), "/tmp/x");
}

/// The settings file carries its format number (gaps row A8); one from before the numbers, and
/// one from a later dm with a higher number and a field this one does not know, read the same.
#[test]
fn the_settings_file_carries_its_format_and_reads_the_same_without_it() {
    let dir = tempfile::tempdir().unwrap();
    let chosen = Settings {
        downloads: DownloadSettings {
            connections: 5,
            ..Default::default()
        },
        advanced: AdvancedSettings {
            speed_limit: 300,
            ..Default::default()
        },
        ..Settings::default()
    };
    let stamped = dir.path().join("settings.json");
    chosen.save_with(&stamped, None).unwrap();
    let text = std::fs::read(&stamped).unwrap();
    assert_eq!(dm_core::saved::format_of(&text), SETTINGS_FORMAT);
    let (read, notice) = Settings::load_from(&stamped);
    assert_eq!(notice, None);
    let read = serde_json::to_value(&read).unwrap();

    let mut fields = serde_json::to_value(&chosen).unwrap();
    let old = dir.path().join("old.json");
    std::fs::write(&old, serde_json::to_string_pretty(&fields).unwrap()).unwrap();
    assert_eq!(dm_core::saved::format_of(&std::fs::read(&old).unwrap()), 0);
    let (from_old, notice) = Settings::load_from(&old);
    assert_eq!(notice, None);
    assert_eq!(serde_json::to_value(&from_old).unwrap(), read);

    fields["format"] = 99.into();
    fields["a_later_field"] = true.into();
    let newer = dir.path().join("newer.json");
    std::fs::write(&newer, fields.to_string()).unwrap();
    let (from_newer, notice) = Settings::load_from(&newer);
    assert_eq!(notice, None, "nothing of it was left out");
    assert_eq!(serde_json::to_value(&from_newer).unwrap(), read);

    let carried = chosen.exported();
    assert_eq!(
        dm_core::saved::format_of(carried.as_bytes()),
        SETTINGS_FORMAT
    );
    assert_eq!(chosen.imported(&carried).unwrap().downloads.connections, 5);
}

/// Every key a `settings.json` has, as dm wrote it before each page of Options had a struct
/// (gaps row A8): the settings no page edits, then General's, Browser's, Folders',
/// Downloads' and Advanced's. The nested values are added by the test.
const A_FULL_FILE: &str = r#"{
    "last_speed_limit": 300,
    "slow_hours": {"enabled": true, "from": "08:00", "to": "18:00", "limit": 128},
    "identify_as_dm": true,
    "duplicate_choice": "again",
    "recent_urls": ["https://example.com/a.zip"],
    "last_folders": {"Videos": "/srv/videos"},
    "limit_mb": 2048,
    "limit_hours": 24,
    "custom_categories": [{"name": "Books", "extensions": ["epub"], "sites": []}],
    "start_copied": true,
    "start_while_asking": true,
    "peer_blocklist": "https://example.com/list.gz",
    "torrent_share": true,
    "torrent_share_ratio": 2.5,
    "torrent_share_minutes": 60,
    "torrent_upload_limit": 100,
    "update_checked": 1700000000,
    "popups_version": 2,
    "text_size": 1.0,
    "hinted_big": true,
    "hinted_details": true,
    "toolbar_style": "icons",
    "toolbar_order": ["add_url"],
    "column_order": ["name", "size"],
    "columns_hidden": ["rate"],
    "column_widths": {"name": 240.5},
    "sidebar_width": 250.0,
    "shut_groups": ["Queues"],
    "sort_column": "size",
    "sort_ascending": false,
    "show_categories": false,
    "show_toolbar": false,
    "toolbar_hidden": ["delete"],
    "site_logins": [{"host": "files.example", "username": "me", "password": "secret"}],
    "second_path": "en5",
    "sounds": {"complete": "/srv/done.wav"},
    "site_searches": [],
    "follows": [],
    "drop_target": true,

    "watch_clipboard": true,
    "start_on_login": false,
    "allow_command_line": true,
    "allow_mcp": true,
    "check_updates": true,
    "language": "de",
    "show_progress": false,
    "show_complete": false,
    "sound_on_complete": true,
    "ask_panel_downloads": false,
    "ask_caught_downloads": false,
    "theme": "dark",
    "double_click": "folder",
    "on_close": "tray",
    "server_dates": true,
    "sound_on_failure": true,
    "mica": true,
    "offer_new_link": false,

    "download_dir": "/srv/files",
    "categorize": false,
    "category_folders": ["Clips", "Music", "", "Docs"],
    "parts_dir": "/srv/parts",
    "unpack_kinds": ["archives", "other"],
    "unpack_trash": true,

    "connections": 16,
    "concurrency": 5,
    "stream_quality": "720",
    "watch_folder": "/srv/drop",
    "record_minutes": 90,
    "on_complete": "scan --quick",
    "recheck_hours": 6,
    "start_queue_on_launch": true,
    "subtitles": true,
    "queue_limits": [{"queue": "Nightly", "files": 2}],
    "queue_starts": [{"queue": "Nightly", "at": "02:30"}],
    "remux_ts": false,
    "sound_language": "es",
    "forget_after_days": 30,
    "on_metered": "wait",
    "battery_below": 20,
    "wifi_only": true,
    "not_roaming": true,
    "data_saver_waits": true,

    "speed_limit": 512,
    "retry_wait": 9,
    "cookies_file": "/srv/cookies.txt",
    "attempts": 7,
    "read_timeout": 45,
    "only_through": "en0",
    "host_limits": [{"host": "slow.example", "connections": 2}],
    "site_rules": [
        {"host": "example.com", "folder": "/srv/example", "speed": 64, "agent": "", "headers": ""}
    ]
}"#;

/// [`A_FULL_FILE`] with its nested settings, each as its own type writes it.
fn a_full_file() -> serde_json::Map<String, serde_json::Value> {
    let mut file: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(A_FULL_FILE).unwrap();
    let hours = Schedule {
        enabled: true,
        from: "01:00".into(),
        to: "05:00".into(),
        ..Schedule::default()
    };
    let nested = [
        ("schedule", serde_json::to_value(&hours)),
        (
            "queue_schedules",
            serde_json::to_value(
                [("Nightly", &hours)]
                    .into_iter()
                    .collect::<std::collections::BTreeMap<_, _>>(),
            ),
        ),
        (
            "catch",
            serde_json::to_value(dm_protocol::CatchRules {
                enabled: false,
                ..Default::default()
            }),
        ),
        (
            "proxy",
            serde_json::to_value(dm_core::Proxy {
                host: "proxy.example".into(),
                port: 3128,
                ..Default::default()
            }),
        ),
        (
            "dialup",
            serde_json::to_value(crate::dialup::DialUp {
                enabled: true,
                connection: "Office VPN".into(),
                ..Default::default()
            }),
        ),
    ];
    for (key, value) in nested {
        file.insert(key.to_string(), value.unwrap());
    }
    file
}

#[test]
fn a_full_settings_file_reads_into_the_pages_and_is_written_back_the_same() {
    let file = a_full_file();
    let text = serde_json::Value::Object(file.clone()).to_string();
    let (read, dropped) = Settings::parse(&text).unwrap();
    assert!(dropped.is_empty(), "left out: {dropped:?}");
    // Each page's struct took its own keys.
    assert_eq!(read.general.theme, "dark");
    assert!(!read.browser.catch.enabled);
    assert_eq!(read.folders.parts_dir, "/srv/parts");
    assert_eq!(read.downloads.connections, 16);
    assert_eq!(read.downloads.sound_language, "es");
    assert_eq!(
        (
            read.downloads.on_metered.as_str(),
            read.downloads.battery_below
        ),
        ("wait", 20)
    );
    // A phone's Options lines (gaps row N10) and the one network (W12).
    let phone = &read.downloads;
    assert!(phone.wifi_only && phone.not_roaming && phone.data_saver_waits);
    assert_eq!(read.advanced.only_through, "en0");
    assert_eq!(read.advanced.proxy.host, "proxy.example");
    assert_eq!(read.limit_hours, 24);
    // Written back: the same keys with the same values, and no other.
    assert_eq!(
        serde_json::to_value(&read).unwrap(),
        serde_json::Value::Object(file.clone())
    );
    let keys = |value: serde_json::Value| -> Vec<String> {
        // Sorted: serde_json keeps the order written when another crate of
        // the workspace turns its `preserve_order` on, and order is not the question.
        let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    };
    assert_eq!(
        keys(serde_json::to_value(Settings::default()).unwrap()),
        keys(serde_json::Value::Object(file.clone())),
        "a key of the file is no setting, or a setting has no key"
    );
    // And saved as dm saves it: the same object, with its format number beside it.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    read.save_with(&path, None).unwrap();
    let mut saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved["format"], SETTINGS_FORMAT);
    saved.as_object_mut().unwrap().remove("format");
    assert_eq!(saved, serde_json::Value::Object(file));
}

#[test]
fn a_file_without_a_pages_keys_reads_with_that_pages_defaults() {
    let read: Settings = serde_json::from_str(
        r#"{"theme": "dark", "connections": 4, "recent_urls": ["https://example.com/a"]}"#,
    )
    .unwrap();
    let fresh = Settings::default();
    assert_eq!(read.general.theme, "dark");
    assert_eq!(read.downloads.connections, 4);
    assert_eq!(read.recent_urls, ["https://example.com/a"]);
    // A page none of whose keys the file has is as a new install has it.
    let json = |settings: &Settings| {
        [
            serde_json::to_value(&settings.browser).unwrap(),
            serde_json::to_value(&settings.folders).unwrap(),
            serde_json::to_value(&settings.advanced).unwrap(),
        ]
    };
    assert_eq!(json(&read), json(&fresh));
    // The rest of a page the file has some of: as before the pages had structs.
    assert_eq!(read.downloads.concurrency, fresh.downloads.concurrency);
    assert_eq!(read.downloads.stream_quality, "best");
    assert!(read.downloads.remux_ts);
    assert_eq!(read.general.double_click, fresh.general.double_click);
    assert!(read.general.offer_new_link);
}

#[test]
fn a_bad_value_on_a_page_is_named_by_its_key_and_costs_only_itself() {
    let (read, dropped) = Settings::parse(
        r#"{"theme": "dark", "connections": "many", "attempts": -3, "categorize": "yes"}"#,
    )
    .unwrap();
    for key in ["connections", "attempts", "categorize"] {
        assert!(
            dropped.iter().any(|d| d == key),
            "{key} not named: {dropped:?}"
        );
    }
    assert_eq!(dropped.len(), 3, "{dropped:?}");
    let fresh = Settings::default();
    assert_eq!(read.general.theme, "dark");
    assert_eq!(read.downloads.connections, fresh.downloads.connections);
    assert_eq!(read.advanced.attempts, fresh.advanced.attempts);
    assert_eq!(read.folders.categorize, fresh.folders.categorize);
}
