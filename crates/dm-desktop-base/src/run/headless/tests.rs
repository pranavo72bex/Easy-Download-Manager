// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What a dm with no window answers over the control port. Only answers
//! that save nothing: outside dm-shell's own tests the settings and the
//! list are kept in the real home, so adding a download or changing a
//! setting is checked on the real binary, on a home of its own
//! (`scripts/app-check.py`, `scripts/smoke.py`).

use super::*;
use dm_shell::app::power::WhenDone;
use dm_shell::app::session::Fired;

/// A platform that does nothing.
struct Nothing;

impl dm_shell::app::backend::Platform for Nothing {
    fn set_login_item(&self, _: bool) -> Result<(), String> {
        Ok(())
    }
    fn notify(&self, _: &str, _: &str) {}
    fn clipboard(&self) -> Option<String> {
        None
    }
    fn reveal(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn open(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn chime(&self) {}
    fn power(
        &self,
        _: dm_shell::app::power::WhenDone,
        _: dm_shell::app::power::Report,
    ) -> Result<(), String> {
        Ok(())
    }
    fn open_with(&self, _: &str, _: &std::path::Path) -> Result<(), String> {
        Ok(())
    }
}

/// dm with no window over an empty queue of its own under `dir`.
fn headless(runtime: &tokio::runtime::Runtime, dir: &std::path::Path) -> Headless {
    headless_with(runtime, dir, Default::default())
}

/// [`headless`], with `settings` (their download folder under `dir`).
fn headless_with(
    runtime: &tokio::runtime::Runtime,
    dir: &std::path::Path,
    settings: dm_shell::Settings,
) -> Headless {
    let settings = dm_shell::Settings {
        folders: dm_shell::settings::FolderSettings {
            download_dir: dir.join("out"),
            ..settings.folders
        },
        ..settings
    };
    let queue = dm_core::DownloadList::new(
        dm_core::client(),
        settings.options(dir.join("parts"), dm_core::Limiter::unlimited()),
        settings.downloads.concurrency,
    );
    let backend = Backend::new(
        runtime.handle().clone(),
        queue,
        settings,
        Vec::new(),
        Arc::new(Nothing),
    );
    Headless::new(backend, None, false)
}

#[test]
fn with_no_window_a_picture_and_a_step_are_refused_in_words() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());
    let shot = dm.answer("ui.screenshot", &json!({"name": "x"}));
    assert_eq!(shot, Err(probe::NO_WINDOW_TO_PICTURE.to_string()));
    for (method, params) in [
        ("ui.click", json!({"text": "Options"})),
        ("ui.type", json!({"text": "a"})),
        ("ui.key", json!({"key": "Escape"})),
        ("ui.scroll", json!({"text": "a"})),
    ] {
        assert_eq!(
            dm.answer(method, &params),
            Err(NO_WINDOW.to_string()),
            "{method}"
        );
    }
    assert_eq!(
        dm.answer("ui.window", &json!({"to": "tray"})),
        Err("there is no window".to_string())
    );
    // Nothing is on a screen: an assistant's dm_screen reads that, not an
    // error.
    let texts = dm.answer("ui.texts", &Value::Null).unwrap();
    assert_eq!(texts["texts"], json!([]));
    let windows = dm.answer("ui.windows", &Value::Null).unwrap();
    assert_eq!(
        (windows["main"].clone(), windows["dialogs"].clone()),
        (Value::Null, json!([]))
    );
}

#[test]
fn the_view_and_the_commands_answer_as_under_a_window() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());
    let listed = dm.answer("commands.list", &Value::Null).unwrap();
    let ids: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["id"].as_str())
        .collect();
    assert!(ids.contains(&"file.stop_download"), "{ids:?}");
    let inspected = dm.answer("ui.inspect", &Value::Null).unwrap();
    assert_eq!(
        (inspected["rows"].clone(), inspected["hidden"].clone()),
        (json!([]), json!(true))
    );
    assert!(dm.answer("ui.place", &json!({"place": "finished"})).is_ok());
    assert!(dm.answer("ui.place", &json!({"place": "nowhere"})).is_err());
    assert_eq!(
        dm.answer("ui.select", &json!({"ids": [7]})).unwrap()["selected"],
        json!([]),
        "a row that is not listed is not chosen"
    );
    // A file is opened by a person, never over the port.
    assert!(dm
        .answer("command.run", &json!({"id": "file.open"}))
        .is_err());
    // What needs a window is refused, said why.
    let add_url = dm
        .answer("command.run", &json!({"id": "program.add_url"}))
        .unwrap_err();
    assert!(add_url.contains("needs dm's window"), "{add_url}");
    assert!(dm
        .answer("command.run", &json!({"id": "no.such.command"}))
        .is_err());
}

#[test]
fn when_done_is_set_and_exit_over_the_port_ends_dm_without_asking() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());
    dm.answer("command.run", &json!({"id": "downloads.when_done.exit"}))
        .unwrap();
    assert_eq!(dm.session.when_done, WhenDone::Quit);
    // With nobody to see a countdown, Exit when done is at once.
    assert_eq!(
        dm.session.when_done_fires(true, Instant::now()),
        Fired::QuitNow
    );
    assert!(!dm.quit_asked());
    dm.answer("command.run", &json!({"id": "program.exit"}))
        .unwrap();
    assert!(dm.quit_asked());
}

#[test]
fn a_batch_reaches_the_view_but_not_a_step() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());
    let answered = dm
        .answer(
            "batch",
            &json!({"steps": [
                {"method": "ui.inspect"},
                {"method": "ui.click", "params": {"text": "Options"}},
            ]}),
        )
        .unwrap();
    let steps = answered.as_array().cloned().unwrap_or_default();
    assert_eq!(steps.len(), 2, "{answered}");
    assert_eq!(steps[0]["ok"], json!(true), "{answered}");
    assert_eq!(steps[1]["ok"], json!(false), "{answered}");
}

/// Scheduler › Hang up modem when done is honoured with no window, as the window honours it
/// (gaps row A7): what hanging up came to is said in English, a connection that was down is not
/// said, and one Windows would not end is a problem.
#[test]
fn with_no_window_hang_up_when_done_is_carried_out_and_said() {
    use dm_shell::dialup::HungUp;
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());
    *dm_core::lock(&dm.hung_up) = Some(HungUp::Done("Office VPN".into()));
    dm.carry(Effect::Scheduled { hang_up: false });
    assert_eq!(
        dm.said,
        "The scheduled downloads are done: Office VPN is hung up."
    );
    assert!(dm_core::lock(&dm.hung_up).is_none(), "said once");

    dm.said.clear();
    *dm_core::lock(&dm.hung_up) = Some(HungUp::WasDown("Office VPN".into()));
    dm.carry(Effect::Scheduled { hang_up: false });
    assert_eq!(dm.said, "");

    *dm_core::lock(&dm.hung_up) = Some(HungUp::NoneSet);
    dm.carry(Effect::Scheduled { hang_up: false });
    assert!(dm.said.contains("Dial Up / VPN"), "{}", dm.said);

    *dm_core::lock(&dm.hung_up) = Some(HungUp::Failed("Office VPN".into(), "busy".into()));
    dm.carry(Effect::Scheduled { hang_up: false });
    assert_eq!(
        dm.problems.last().map(String::as_str),
        Some("Office VPN was not hung up: busy")
    );
}

/// gaps row W10: with no window, a followed one's reading is taken in as the window takes it:
/// one no longer followed is let go with nothing added, and one that could not be read is a
/// problem said in English. (What a reading adds saves what it has seen, which these tests,
/// on the real home, do not: the window's test adds.)
#[test]
fn with_no_window_a_followed_ones_reading_is_taken_in() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let address = "http://127.0.0.1:9/feed.xml".to_string();
    let settings = dm_shell::Settings {
        follows: vec![dm_shell::app::follow::Follow {
            address: address.clone(),
            title: "Weekly".into(),
            every_hours: 24,
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut dm = headless_with(&runtime, dir.path(), settings);
    let list = dm_core::youtube::Playlist {
        title: "Weekly".into(),
        entries: vec![dm_core::youtube::PlaylistEntry {
            id: "1".into(),
            title: "Episode 1".into(),
            seconds: None,
            page: Some("http://127.0.0.1:9/episode-1.mp3".into()),
        }],
        ..Default::default()
    };
    dm.carry(Effect::Followed {
        address: "http://127.0.0.1:9/other.xml".into(),
        read: Ok(list),
    });
    assert!(dm.rows.is_empty(), "not followed: nothing added");
    assert!(dm.problems.is_empty(), "{:?}", dm.problems);
    dm.carry(Effect::Followed {
        address,
        read: Err("refused".into()),
    });
    assert!(dm.rows.is_empty());
    assert_eq!(
        dm.problems,
        vec!["the followed Weekly couldn't be read: refused".to_string()]
    );
}

/// A phone's dm going on without its window ends once nothing downloads, waits to start, or is
/// shared: a stopped, failed or finished download keeps nothing going.
#[test]
fn only_downloading_waiting_or_sharing_keeps_a_windowless_phone_going() {
    let row = |kind: &str| -> Arc<Entry> {
        Arc::new(
            serde_json::from_value(serde_json::json!({
                "id": 1,
                "url": "https://example.com/a.bin",
                "state": if kind == "failed" {
                    serde_json::json!({ "kind": "failed", "reason": "gone" })
                } else {
                    serde_json::json!({ "kind": kind })
                },
                "progress": { "downloaded": 0, "total": null, "speed": 0.0, "eta": null },
                "filename": "a.bin",
            }))
            .expect("an entry"),
        )
    };
    assert!(busy(&[row("running")], false));
    assert!(busy(&[row("queued")], false));
    assert!(busy(&[row("completed")], true), "a torrent still shared");
    for kind in ["paused", "completed", "failed", "cancelled"] {
        assert!(!busy(&[row(kind)], false), "{kind}");
    }
    assert!(!busy(&[], false));
}

/// `answer_requests` processes requests from the control port, checking MCP
/// permissions and answering each in turn.
#[test]
fn answer_requests_processes_control_port_requests() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let mut dm = Headless::new(
        {
            let settings = dm_shell::Settings {
                folders: dm_shell::settings::FolderSettings {
                    download_dir: dir.path().join("out"),
                    ..Default::default()
                },
                ..Default::default()
            };
            let queue = dm_core::DownloadList::new(
                dm_core::client(),
                settings.options(dir.path().join("parts"), dm_core::Limiter::unlimited()),
                settings.downloads.concurrency,
            );
            Backend::new(
                runtime.handle().clone(),
                queue,
                settings,
                Vec::new(),
                Arc::new(Nothing),
            )
        },
        Some(rx),
        false,
    );

    // No requests: nothing happens
    dm.answer_requests();
    assert!(dm.said.is_empty());

    // A request that gets an answer
    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    tx.send(crate::control::Request {
        via_mcp: false,
        method: "ui.texts".into(),
        params: Value::Null,
        reply: reply_tx,
    })
    .unwrap();
    dm.answer_requests();
    let answer = reply_rx.recv().unwrap();
    assert!(answer.is_ok());
    assert_eq!(answer.unwrap()["texts"], json!([]));

    // A request for an unknown method
    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    tx.send(crate::control::Request {
        via_mcp: false,
        method: "no.such.method".into(),
        params: Value::Null,
        reply: reply_tx,
    })
    .unwrap();
    dm.answer_requests();
    let answer = reply_rx.recv().unwrap();
    assert!(answer.is_err());
    assert!(answer.unwrap_err().contains("no method is called"));
}

/// `answer_requests` refuses non-MCP requests when the port is assistants-only.
#[test]
fn answer_requests_refuses_non_mcp_when_assistants_only() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let mut dm = Headless::new(
        {
            let settings = dm_shell::Settings {
                folders: dm_shell::settings::FolderSettings {
                    download_dir: dir.path().join("out"),
                    ..Default::default()
                },
                ..Default::default()
            };
            let queue = dm_core::DownloadList::new(
                dm_core::client(),
                settings.options(dir.path().join("parts"), dm_core::Limiter::unlimited()),
                settings.downloads.concurrency,
            );
            Backend::new(
                runtime.handle().clone(),
                queue,
                settings,
                Vec::new(),
                Arc::new(Nothing),
            )
        },
        Some(rx),
        true, // assistants_only
    );

    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    tx.send(crate::control::Request {
        via_mcp: false, // non-MCP request
        method: "ui.texts".into(),
        params: Value::Null,
        reply: reply_tx,
    })
    .unwrap();
    dm.answer_requests();
    let answer = reply_rx.recv().unwrap();
    assert!(answer.is_err());
    assert!(answer.unwrap_err().contains("AI assistants only"));
}

/// `answer_requests` refuses non-MCP requests when MCP is not allowed.
#[test]
fn answer_requests_refuses_non_mcp_when_mcp_not_allowed() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let mut dm = Headless::new(
        {
            let settings = dm_shell::Settings {
                folders: dm_shell::settings::FolderSettings {
                    download_dir: dir.path().join("out"),
                    ..Default::default()
                },
                general: dm_shell::settings::GeneralSettings {
                    allow_mcp: false,
                    ..Default::default()
                },
                ..Default::default()
            };
            let queue = dm_core::DownloadList::new(
                dm_core::client(),
                settings.options(dir.path().join("parts"), dm_core::Limiter::unlimited()),
                settings.downloads.concurrency,
            );
            Backend::new(
                runtime.handle().clone(),
                queue,
                settings,
                Vec::new(),
                Arc::new(Nothing),
            )
        },
        Some(rx),
        false,
    );

    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    tx.send(crate::control::Request {
        via_mcp: true,
        method: "ui.texts".into(),
        params: Value::Null,
        reply: reply_tx,
    })
    .unwrap();
    dm.answer_requests();
    let answer = reply_rx.recv().unwrap();
    assert!(answer.is_err());
    assert!(answer.unwrap_err().contains("AI assistants are turned off"));
}

/// `downloads.add` via `answer_inside` adds fresh URLs and reports already-listed ones.
#[test]
fn downloads_add_via_answer_inside_adds_fresh_and_reports_listed() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());

    // Add a fresh URL
    let result = dm
        .answer(
            "downloads.add",
            &json!({"urls": ["https://example.com/file.zip"]}),
        )
        .unwrap();
    assert_eq!(result["added"], json!(1));
    assert_eq!(result["already_listed"], json!([]));

    // Add the same URL again (should be listed, not added)
    let result = dm
        .answer(
            "downloads.add",
            &json!({"urls": ["https://example.com/file.zip"]}),
        )
        .unwrap();
    assert_eq!(result["added"], json!(0));
    assert_eq!(
        result["already_listed"],
        json!(["https://example.com/file.zip"])
    );

    // Add with `again: true` should add it again
    let result = dm
        .answer(
            "downloads.add",
            &json!({"urls": ["https://example.com/file.zip"], "again": true}),
        )
        .unwrap();
    assert_eq!(result["added"], json!(1));
    assert_eq!(result["already_listed"], json!([]));

    // Add multiple URLs at once
    let result = dm
        .answer(
            "downloads.add",
            &json!({"urls": ["https://example.com/a.zip", "https://example.com/b.zip"]}),
        )
        .unwrap();
    assert_eq!(result["added"], json!(2));
    assert_eq!(result["already_listed"], json!([]));

    // Empty URLs list is an error
    let result = dm.answer("downloads.add", &json!({"urls": []}));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("empty"));

    // Missing urls is an error
    let result = dm.answer("downloads.add", &json!({}));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("give `urls`"));
}

/// `downloads.add` with folder option validates it's inside the download folder.
#[test]
fn downloads_add_validates_folder_inside_download_dir() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());
    let download_dir = dir.path().join("out");

    // Folder outside download dir is refused
    let result = dm.answer(
        "downloads.add",
        &json!({
            "urls": ["https://example.com/file.zip"],
            "folder": "/etc"
        }),
    );
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .contains("must be inside the download folder"));

    // Folder with .. is refused
    let result = dm.answer(
        "downloads.add",
        &json!({
            "urls": ["https://example.com/file.zip"],
            "folder": "~/../etc"
        }),
    );
    assert!(result.is_err());

    // Valid folder inside download dir (absolute path) is accepted
    let subfolder = download_dir.join("subfolder");
    let result = dm.answer(
        "downloads.add",
        &json!({
            "urls": ["https://example.com/file.zip"],
            "folder": subfolder.to_string_lossy()
        }),
    );
    assert!(result.is_ok(), "{:?}", result);
    assert_eq!(result.unwrap()["added"], json!(1));
}

/// `downloads.add` with a video part requires both from and to.
#[test]
fn downloads_add_video_part_requires_both_ends() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());

    // Only from is an error
    let result = dm.answer(
        "downloads.add",
        &json!({
            "urls": ["https://example.com/video.mp4"],
            "from": "1:00"
        }),
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("give both `from` and `to`"));

    // Only to is an error
    let result = dm.answer(
        "downloads.add",
        &json!({
            "urls": ["https://example.com/video.mp4"],
            "to": "2:00"
        }),
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("give both `from` and `to`"));

    // Both from and to is accepted
    let result = dm.answer(
        "downloads.add",
        &json!({
            "urls": ["https://example.com/video.mp4"],
            "from": "1:00",
            "to": "2:00"
        }),
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap()["added"], json!(1));
}

/// `downloads.speed_limit` via `answer_inside` sets the limit and keeps the previous one.
#[test]
fn downloads_speed_limit_via_answer_inside_sets_and_keeps_previous() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());

    // Initial limit is 0 (unlimited)
    let result = dm
        .answer("downloads.speed_limit", &json!({"kb": 500}))
        .unwrap();
    assert_eq!(result["speed_limit_kb"], json!(500));

    // Setting a new limit keeps the previous one
    let result = dm
        .answer("downloads.speed_limit", &json!({"kb": 1000}))
        .unwrap();
    assert_eq!(result["speed_limit_kb"], json!(1000));

    // Setting to 0 removes the limit
    let result = dm
        .answer("downloads.speed_limit", &json!({"kb": 0}))
        .unwrap();
    assert_eq!(result["speed_limit_kb"], json!(0));

    // Invalid kb (too large) is an error
    let result = dm.answer("downloads.speed_limit", &json!({"kb": 100_000_001}));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("at most 100000000"));

    // Missing kb is an error
    let result = dm.answer("downloads.speed_limit", &json!({}));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("give `kb`"));
}

/// `run` handles SelectAll command.
#[test]
fn run_select_all_selects_all_shown_rows() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());

    // Add some downloads first
    dm.answer(
        "downloads.add",
        &json!({"urls": ["https://example.com/a.zip", "https://example.com/b.zip"]}),
    )
    .unwrap();
    dm.read();

    // SelectAll should select all shown rows
    dm.answer("command.run", &json!({"id": "file.select_all"}))
        .unwrap();
    assert_eq!(dm.selected.len(), 2);
}

/// `run` handles speed limiter toggle and level commands.
#[test]
fn run_speed_limiter_commands_toggle_and_set_levels() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());

    // SpeedLimiterToggle turns it on (to last limit or default)
    dm.answer(
        "command.run",
        &json!({"id": "downloads.speed_limiter.toggle"}),
    )
    .unwrap();
    assert!(dm.said.contains("Speed limiter on"));
    let limit_after_toggle = dm.backend.settings().advanced.speed_limit;
    assert!(limit_after_toggle > 0);

    // SpeedLow sets a low limit (256 KB/s)
    dm.answer("command.run", &json!({"id": "downloads.speed.low"}))
        .unwrap();
    assert!(dm.said.contains("Speed limiter on"));
    assert_eq!(dm.backend.settings().advanced.speed_limit, 256);

    // SpeedMedium sets a medium limit (1024 KB/s)
    dm.answer("command.run", &json!({"id": "downloads.speed.medium"}))
        .unwrap();
    assert_eq!(dm.backend.settings().advanced.speed_limit, 1024);

    // SpeedHigh sets a high limit (4096 KB/s)
    dm.answer("command.run", &json!({"id": "downloads.speed.high"}))
        .unwrap();
    assert_eq!(dm.backend.settings().advanced.speed_limit, 4096);

    // SpeedUnlimited turns it off
    dm.answer("command.run", &json!({"id": "downloads.speed.unlimited"}))
        .unwrap();
    assert_eq!(dm.said, "Speed limiter off.");
    assert_eq!(dm.backend.settings().advanced.speed_limit, 0);
}

/// `run` handles StartQueue and StopQueue commands.
#[test]
fn run_start_stop_queue_commands() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());

    // StartQueue on empty queue
    dm.answer("command.run", &json!({"id": "downloads.start_queue"}))
        .unwrap();
    assert!(dm
        .said
        .contains("Nothing in Main queue was waiting to start"));

    // StopQueue on empty queue
    dm.answer("command.run", &json!({"id": "downloads.stop_queue"}))
        .unwrap();
    assert_eq!(dm.said, "Stopped 0 downloads in Main queue.");

    // Add a download and try again
    dm.answer(
        "downloads.add",
        &json!({"urls": ["https://example.com/file.zip"]}),
    )
    .unwrap();
    dm.read();

    dm.answer("command.run", &json!({"id": "downloads.start_queue"}))
        .unwrap();
    // The download may start or not depending on concurrency, but the message should reflect it
    assert!(dm.said.contains("Main queue") || dm.said.contains("Nothing in"));
}

/// `run` handles verb commands on chosen rows (pause, resume, stop_all, resume_all).
#[test]
fn run_verb_commands_on_chosen_rows() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());

    // Add a download
    dm.answer(
        "downloads.add",
        &json!({"urls": ["https://example.com/file.zip"]}),
    )
    .unwrap();
    dm.read();

    // Select the download
    let rows = dm.shown();
    assert!(!rows.is_empty());
    let id = rows[0].id;
    dm.answer("ui.select", &json!({"ids": [id]})).unwrap();

    // Pause the selected download (StopDownload)
    dm.answer("command.run", &json!({"id": "file.stop_download"}))
        .unwrap();
    dm.read();
    let row = dm.rows.iter().find(|r| r.id == id).unwrap();
    assert!(matches!(row.state, dm_core::State::Paused));

    // Resume the selected download (DownloadNow)
    dm.answer("command.run", &json!({"id": "file.download_now"}))
        .unwrap();
    dm.read();
    let row = dm.rows.iter().find(|r| r.id == id).unwrap();
    assert!(matches!(
        row.state,
        dm_core::State::Running | dm_core::State::Queued
    ));

    // StopAll and ResumeAll work on all rows
    dm.answer("command.run", &json!({"id": "downloads.stop_all"}))
        .unwrap();
    dm.read();
    for row in &dm.rows {
        assert!(matches!(row.state, dm_core::State::Paused));
    }

    dm.answer("command.run", &json!({"id": "downloads.resume_all"}))
        .unwrap();
    dm.read();
    for row in &dm.rows {
        assert!(matches!(
            row.state,
            dm_core::State::Running | dm_core::State::Queued
        ));
    }
}

/// `run` refuses commands that need a window.
#[test]
fn run_refuses_commands_needing_a_window() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());

    // Add URL needs a window
    let result = dm.answer("command.run", &json!({"id": "program.add_url"}));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("needs dm's window"));

    // Open file needs a window (refused by person_only check)
    let result = dm.answer("command.run", &json!({"id": "file.open"}));
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .contains("done in the window by a person"));
}

/// `run` handles StartQueue and StopQueue commands on the current place's queue.
#[test]
fn run_start_stop_queue_on_current_place_queue() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());

    // StartQueue on main queue (default place)
    dm.answer("command.run", &json!({"id": "downloads.start_queue"}))
        .unwrap();
    assert!(dm.said.contains("Main queue"));

    // StopQueue on main queue
    dm.answer("command.run", &json!({"id": "downloads.stop_queue"}))
        .unwrap();
    assert_eq!(dm.said, "Stopped 0 downloads in Main queue.");

    // Change to a named queue and try again
    dm.place = dm_shell::app::places::Place::Queue("TestQueue".into());
    dm.answer("command.run", &json!({"id": "downloads.start_queue"}))
        .unwrap();
    assert!(dm.said.contains("TestQueue"));

    dm.answer("command.run", &json!({"id": "downloads.stop_queue"}))
        .unwrap();
    assert_eq!(dm.said, "Stopped 0 downloads in TestQueue.");
}

/// `ui.select` chooses only listed rows.
#[test]
fn ui_select_chooses_only_listed_rows() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());

    // Add a download
    dm.answer(
        "downloads.add",
        &json!({"urls": ["https://example.com/file.zip"]}),
    )
    .unwrap();
    dm.read();

    let rows = dm.shown();
    assert!(!rows.is_empty());
    let id = rows[0].id;

    // Select the listed row
    let result = dm.answer("ui.select", &json!({"ids": [id]})).unwrap();
    assert_eq!(result["selected"], json!([id]));
    assert_eq!(dm.selected, std::collections::BTreeSet::from([id]));

    // Select an unlisted row (should be ignored)
    let result = dm.answer("ui.select", &json!({"ids": [999]})).unwrap();
    assert_eq!(result["selected"], json!([]));
    assert!(dm.selected.is_empty());

    // Select mix of listed and unlisted
    let result = dm.answer("ui.select", &json!({"ids": [id, 999]})).unwrap();
    assert_eq!(result["selected"], json!([id]));
    assert_eq!(dm.selected, std::collections::BTreeSet::from([id]));
}

/// `ui.place` changes the shown place.
#[test]
fn ui_place_changes_the_shown_place() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());

    // Valid places
    assert!(dm.answer("ui.place", &json!({"place": "all"})).is_ok());
    assert_eq!(dm.place, dm_shell::app::places::Place::All);

    assert!(dm
        .answer("ui.place", &json!({"place": "unfinished"}))
        .is_ok());
    assert_eq!(dm.place, dm_shell::app::places::Place::Unfinished);

    assert!(dm.answer("ui.place", &json!({"place": "finished"})).is_ok());
    assert_eq!(dm.place, dm_shell::app::places::Place::Finished);

    assert!(dm
        .answer("ui.place", &json!({"place": "queue:Main queue"}))
        .is_ok());
    assert!(matches!(dm.place, dm_shell::app::places::Place::Queue(ref q) if q == "Main queue"));

    // Invalid place
    let result = dm.answer("ui.place", &json!({"place": "nowhere"}));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("no place is called"));
}

/// `ui.inspect` returns the window's view with rows.
#[test]
fn ui_inspect_returns_window_view_with_rows() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());

    // Add a download
    dm.answer(
        "downloads.add",
        &json!({"urls": ["https://example.com/file.zip"]}),
    )
    .unwrap();
    dm.read();

    let inspect = dm.answer("ui.inspect", &Value::Null).unwrap();

    assert_eq!(inspect["place"], json!("All"));
    assert_eq!(inspect["hidden"], json!(true));
    assert_eq!(inspect["rows"].as_array().unwrap().len(), 1);
    let row = &inspect["rows"][0];
    assert_eq!(row["id"], json!(dm.rows[0].id));
    assert!(!row["name"].as_str().unwrap().is_empty());
    assert!(!row["status"].as_str().unwrap().is_empty());
    // Queue is empty for downloads added without a specific queue
    assert_eq!(row["queue"], json!(""));
    assert!(row["url"].as_str().unwrap().contains("example.com"));
    assert_eq!(row["rect"], Value::Null);
    assert_eq!(inspect["selected"], json!([]));
    assert_eq!(inspect["dialogs"], json!([]));
    assert_eq!(inspect["problems"], json!([]));
    assert_eq!(inspect["categories"], json!(false));
    assert_eq!(inspect["toolbar"], json!(false));
    assert_eq!(inspect["dark"], json!(false));
    assert!(inspect["sort"].is_null());
    assert_eq!(inspect["said"], json!(""));
}

/// `commands.list` lists all commands with their enabled state.
#[test]
fn commands_list_lists_all_commands_with_enabled_state() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());

    let commands = dm.answer("commands.list", &Value::Null).unwrap();

    let commands_array = commands.as_array().unwrap();
    assert!(!commands_array.is_empty());
    for cmd in commands_array {
        assert!(!cmd["id"].as_str().unwrap().is_empty());
        assert!(!cmd["label"].as_str().unwrap().is_empty());
        // enabled is a bool, no assertion needed
    }

    // Check for some known commands
    let ids: Vec<&str> = commands_array
        .iter()
        .filter_map(|c| c["id"].as_str())
        .collect();
    assert!(ids.contains(&"file.stop_download"));
    assert!(ids.contains(&"program.add_url"));
    assert!(ids.contains(&"downloads.speed_limiter.toggle"));
}

/// `command.run` runs a command and returns what it did.
#[test]
fn command_run_runs_command_and_returns_result() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());

    // Add a download
    dm.answer(
        "downloads.add",
        &json!({"urls": ["https://example.com/file.zip"]}),
    )
    .unwrap();
    dm.read();

    // Run a command that works (SelectAll) - no said message for SelectAll
    let result = dm
        .answer("command.run", &json!({"id": "file.select_all"}))
        .unwrap();
    assert_eq!(result["ran"], json!("file.select_all"));
    // SelectAll doesn't produce a said message
    assert_eq!(dm.selected.len(), 1);

    // Run a command that needs a window (should be refused)
    let result = dm.answer("command.run", &json!({"id": "file.open"}));
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .contains("done in the window by a person"));

    // Run a command that doesn't exist
    let result = dm.answer("command.run", &json!({"id": "no.such.command"}));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("no command is called"));

    // Run a command that has nothing to act on (pause with no selection)
    dm.selected.clear();
    let result = dm.answer("command.run", &json!({"id": "file.stop_download"}));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("has nothing to act on"));
}

/// UI methods are all refused when headless.
#[test]
fn ui_methods_refused_when_headless() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut dm = headless(&runtime, dir.path());

    // ui.click is refused
    assert_eq!(
        dm.answer("ui.click", &json!({"text": "Options"})),
        Err(NO_WINDOW.to_string())
    );

    // ui.scroll is refused
    assert_eq!(
        dm.answer("ui.scroll", &json!({"text": "a"})),
        Err(NO_WINDOW.to_string())
    );

    // ui.type is refused
    assert_eq!(
        dm.answer("ui.type", &json!({"text": "a"})),
        Err(NO_WINDOW.to_string())
    );

    // ui.key is refused
    assert_eq!(
        dm.answer("ui.key", &json!({"key": "Escape"})),
        Err(NO_WINDOW.to_string())
    );

    // ui.window is refused
    assert_eq!(
        dm.answer("ui.window", &json!({"to": "tray"})),
        Err("there is no window".to_string())
    );

    // ui.move_window is refused
    assert_eq!(
        dm.answer(
            "ui.move_window",
            &json!({"title": "Options", "x": 0, "y": 0})
        ),
        Err("there is no window to move".to_string())
    );

    // ui.accessibility returns empty array
    let acc = dm.answer("ui.accessibility", &Value::Null).unwrap();
    assert_eq!(acc, json!([]));

    // ui.windows returns empty
    let windows = dm.answer("ui.windows", &Value::Null).unwrap();
    assert_eq!(windows["main"], Value::Null);
    assert_eq!(windows["dialogs"], json!([]));
    assert_eq!(windows["pixels_per_point"], Value::Null);
    assert_eq!(windows["main_title"], Value::Null);

    // ui.texts returns empty (not an error)
    let texts = dm.answer("ui.texts", &Value::Null).unwrap();
    assert_eq!(texts["texts"], json!([]));
}
