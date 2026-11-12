// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The control port over the window, method by method: what each answers, what it refuses
//! and why, and what the window shows after; and an AI assistant driving the window through
//! its tools. What each method reads of its parameters is the shared code's
//! (`dm_desktop_base::probe`, `dm_shell::app::control_api`) and tested there. Ported from the
//! egui window's `tests/control_cover.rs`.

use dm_core::State;
use serde_json::{json, Value};

use crate::harness::world::{world, World};
use crate::harness::{settings_file, Harness};

/// The window, as a person who lets AI assistants in has it: off in a new install (gaps row
/// F7).
fn harness(world: &World) -> Harness {
    let mut settings = world.settings();
    settings.allow_mcp = true;
    world.harness_with(settings)
}

/// `urls` added for later through the port.
fn later(h: &mut Harness, urls: Vec<String>) {
    let said = h
        .call("downloads.add", json!({ "urls": urls, "later": true }))
        .unwrap();
    assert!(said["added"].as_u64().unwrap() > 0, "{said}");
    h.until("the rows to be read", |h| !h.rows().is_empty());
}

/// The id of the row whose address ends with `name`.
fn id_of(h: &mut Harness, name: &str) -> u64 {
    h.view(|view| view.backend.list())
        .iter()
        .find(|r| r.url.ends_with(name))
        .unwrap_or_else(|| panic!("no row for {name}"))
        .id
}

fn state_of(h: &mut Harness, id: u64) -> Option<State> {
    h.view(|view| view.backend.row(id).map(|r| r.state))
}

#[test]
fn a_script_reads_the_screen_clicks_types_and_presses_keys_as_a_person() {
    let _file = settings_file();
    let world = world();
    let mut h = harness(&world);
    let seen = h.call("ui.texts", json!({})).unwrap();
    let add = seen["texts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["text"] == "Add URL")
        .unwrap_or_else(|| panic!("no Add URL on screen: {seen}"))
        .clone();
    assert_eq!(add["window"], "main");
    assert_eq!(add["rect"].as_array().unwrap().len(), 4);

    // Found whatever the case it is asked in.
    let clicked = h.call("ui.click", json!({ "text": "add url" })).unwrap();
    assert_eq!(clicked["clicked"], "Add URL");
    assert_eq!(clicked["at"].as_array().unwrap().len(), 2);
    h.until("Add URL to open", |h| h.dialog_open("Add URL"));

    // Typed where the keyboard is: Add URL's address field.
    let typed = h
        .call("ui.type", json!({ "text": "héllo-typed" }))
        .unwrap();
    assert_eq!(typed["typed"], 11);
    h.until("the words typed", |h| h.shows_part("héllo-typed"));

    let pressed = h.call("ui.key", json!({ "key": "Escape" })).unwrap();
    assert_eq!(pressed["pressed"], "Escape");
    h.until("Escape to close Add URL", |h| !h.dialog_open("Add URL"));

    // Words that are not there are refused, naming some that are; a refused click is no step.
    let missing = h
        .call("ui.click", json!({ "text": "no such words" }))
        .unwrap_err();
    assert!(missing.contains("nothing on screen says"), "{missing}");
    let missing = h
        .call("ui.click", json!({ "text": "Add URL", "nth": 7 }))
        .unwrap_err();
    assert!(missing.contains("nothing on screen says"), "{missing}");
    assert!(h.dialogs().is_empty());
}

/// What runs a downloaded file is a person's to do: the row menu's Open with, asked for by a
/// part of its words, is refused; and a double-click that opens a finished file, where Options
/// says so, runs nothing when a script makes it.
#[test]
fn what_runs_a_file_is_refused_to_a_script_in_the_row_menu_and_on_a_double_click() {
    let _file = settings_file();
    let world = world();
    let mut settings = world.settings();
    settings.double_click = "open".into();
    let mut h = world.harness_with(settings);
    h.call("downloads.add", json!({ "urls": [world.url("file.zip")] }))
        .unwrap();
    h.until("file.zip to finish", |h| {
        h.rows().iter().any(|r| r.state == State::Completed)
    });
    h.until_shown("file.zip");
    let clicked = h
        .call("ui.click", json!({ "text": "file.zip", "button": "right" }))
        .unwrap();
    assert_eq!(clicked["clicked"], "file.zip");
    h.until("the row menu", |h| {
        h.texts().iter().any(|w| w.starts_with("Open with"))
    });
    let refused = h
        .call("ui.click", json!({ "text": "pen wit" }))
        .unwrap_err();
    assert!(refused.contains("is done by a person"), "{refused}");
    h.close_dialogs();

    h.call("ui.click", json!({ "text": "file.zip", "double": true }))
        .unwrap();
    h.until("the refusal to be said", |h| {
        h.said().contains("done by a person")
    });
    assert!(
        world.fake.opened.lock().unwrap().is_empty(),
        "the file was run: {:?}",
        world.fake.opened.lock().unwrap()
    );
}

#[test]
fn a_scroll_turns_the_list_under_the_words_it_is_held_over() {
    let _file = settings_file();
    let world = world();
    let mut h = harness(&world);
    let urls: Vec<String> = (0..80)
        .map(|i| world.url(&format!("row{i:02}.bin")))
        .collect();
    later(&mut h, urls);
    h.until_shown("row00.bin");
    let missing = h
        .call("ui.scroll", json!({ "text": "row79.bin" }))
        .unwrap_err();
    assert!(missing.contains("nothing on screen says"), "{missing}");
    let scrolled = h
        .call("ui.scroll", json!({ "text": "row03.bin", "lines": 12 }))
        .unwrap();
    assert_eq!(scrolled, json!({ "scrolled": "row03.bin", "lines": 12.0 }));
    h.until("the list to turn", |h| !h.shows("row00.bin"));
}

#[test]
fn keys_with_modifiers_reach_the_window_and_unknown_keys_are_refused() {
    let _file = settings_file();
    let world = world();
    let mut h = harness(&world);
    later(&mut h, vec![world.url("k1.bin"), world.url("k2.bin")]);
    h.until("both rows", |h| h.rows().len() == 2);
    assert!(h
        .call("ui.key", json!({ "key": "Hyper" }))
        .unwrap_err()
        .contains("no key is called Hyper"));
    // Select all, as the platform writes it.
    let command = if cfg!(target_os = "macos") {
        "CMD"
    } else {
        "Ctrl"
    };
    let pressed = h
        .call("ui.key", json!({ "key": "A", "modifiers": [command] }))
        .unwrap();
    assert_eq!(pressed["pressed"], "A");
    h.until("every row to be selected", |h| {
        h.view(|view| view.selected.len() == 2)
    });
}

/// A dialog is a window of its own: listed by its title, its words a dialog's.
#[test]
fn dialogs_in_windows_of_their_own_are_listed_and_read() {
    let _file = settings_file();
    let world = world();
    let mut h = harness(&world);
    let windows = h.call("ui.windows", json!({})).unwrap();
    assert_eq!(windows["dialogs"], json!([]));
    h.command("program.add_url");
    h.until("Add URL", |h| h.dialog_open("Add URL"));
    let seen = h.call("ui.texts", json!({})).unwrap();
    assert!(
        seen["texts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["window"] == "dialog"),
        "{seen}"
    );
    // Moving it is asked of the system's window, which GPUI's test platform has none of: what
    // `ui.move_window` reads of its parameters is `dm_desktop_base::probe`'s, tested there.
    h.click("Cancel");
    h.until("Add URL to close", |h| !h.dialog_open("Add URL"));
}

#[test]
fn commands_are_listed_and_run_and_what_cannot_run_is_said() {
    let _file = settings_file();
    let world = world();
    let mut h = harness(&world);
    let listed = h.call("commands.list", json!({})).unwrap();
    let listed = listed.as_array().unwrap();
    let remove = listed.iter().find(|c| c["id"] == "file.remove").unwrap();
    assert_eq!(remove["enabled"], false, "nothing is selected");
    let unknown = h
        .call("command.run", json!({ "id": "file.nonsense" }))
        .unwrap_err();
    assert!(unknown.contains("no command is called file.nonsense"));
    let refused = h
        .call("command.run", json!({ "id": "file.preview" }))
        .unwrap_err();
    assert!(refused.contains("by a person"), "{refused}");
    let idle = h
        .call("command.run", json!({ "id": "file.remove" }))
        .unwrap_err();
    assert!(idle.contains("has nothing to act on"), "{idle}");

    let ran = h
        .call("command.run", json!({ "id": "program.add_url" }))
        .unwrap();
    assert_eq!(ran["ran"], "program.add_url");
    h.until("Add URL", |h| h.dialog_open("Add URL"));
    h.close_dialogs();

    // Exit is not asked about over the port: dm quits.
    assert!(!h.view(|view| view.quit_asked));
    let quit = h
        .call("command.run", json!({ "id": "program.exit" }))
        .unwrap();
    assert_eq!(quit["ran"], "program.exit");
    h.until("dm to quit", |h| h.view(|view| view.quit_asked));
    assert!(h
        .call("nonsense", json!({}))
        .unwrap_err()
        .contains("no method is called nonsense"));
}

#[test]
fn the_selection_and_the_place_shown_are_set_and_read_back() {
    let _file = settings_file();
    let world = world();
    let mut h = harness(&world);
    later(&mut h, vec![world.url("one.mp4"), world.url("two.zip")]);
    h.until("both rows", |h| h.rows().len() == 2);
    let one = id_of(&mut h, "one.mp4");
    let picked = h
        .call("ui.select", json!({ "ids": [one, 424242, "x"] }))
        .unwrap();
    assert_eq!(picked, json!({ "selected": [one] }));
    assert!(h.view(|view| view.selected.contains(&one)));

    for place in ["all", "unfinished", "finished", "kind:videos", "queue:Main queue"] {
        let said = h.call("ui.place", json!({ "place": place })).unwrap();
        assert_eq!(said["place"], place);
    }
    assert!(h.view(|view| matches!(
        view.place,
        dm_shell::app::places::Place::Queue(ref q) if q == "Main queue"
    )));
    for (place, why) in [
        ("queue:Nowhere", "no queue is called Nowhere"),
        ("custom:Mine", "no category is called Mine"),
        ("somewhere", "no place is called somewhere"),
    ] {
        let refused = h.call("ui.place", json!({ "place": place })).unwrap_err();
        assert!(refused.contains(why), "{place}: {refused}");
    }
    h.settings_to(|s| {
        s.custom_categories.push(dm_core::Custom {
            name: "Mine".into(),
            extensions: vec!["zip".into()],
            sites: Vec::new(),
        })
    });
    h.call("ui.place", json!({ "place": "custom:Mine" }))
        .unwrap();
    assert!(h.view(|view| matches!(
        view.place,
        dm_shell::app::places::Place::Custom(ref c) if c == "Mine"
    )));

    h.call("ui.place", json!({ "place": "all" })).unwrap();
    let seen = h.call("ui.inspect", json!({})).unwrap();
    assert_eq!(seen["place"], "All");
    assert!(seen["dialogs"].as_array().unwrap().is_empty(), "{seen}");
    let rows = seen["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{seen}");
    let first = rows.iter().find(|r| r["id"] == one).unwrap();
    assert_eq!(first["name"], "one.mp4");
    assert_eq!(first["queue"], "Main queue");
}

#[test]
fn the_queue_is_listed_without_any_sign_in() {
    let _file = settings_file();
    let world = world();
    let mut h = harness(&world);
    let cookies = ["session", "=", "abc"].concat();
    let url = world.url("signed.bin");
    h.view(|view| {
        view.backend
            .add_with(
                &url,
                dm_core::AddOptions {
                    later: true,
                    queue: "Main queue".into(),
                    cookies: cookies.clone(),
                    headers: vec![("X-Test".into(), "value".into())],
                    ..Default::default()
                },
            )
            .unwrap()
    });
    later(&mut h, vec![world.url("plain.bin")]);
    let listed = h.call("queue.list", json!({})).unwrap();
    let listed = listed.as_array().unwrap();
    assert_eq!(listed.len(), 2);
    for row in listed {
        assert!(row.get("cookies").is_none() && row.get("headers").is_none());
        assert!(!row.to_string().contains(&cookies), "{row}");
        let signed = row["url"].as_str().unwrap().ends_with("signed.bin");
        assert_eq!(row["signed_in"], signed, "{row}");
    }
}

#[test]
fn downloads_are_paused_resumed_and_cancelled_by_id_and_what_cannot_be_is_said() {
    let _file = settings_file();
    let world = world();
    let mut h = harness(&world);
    for url in ["file.zip", "gone.iso", "stalled.bin"] {
        let said = h
            .call("downloads.add", json!({ "urls": [world.url(url)] }))
            .unwrap();
        assert_eq!(said["added"], 1, "{said}");
    }
    let (done, gone, running) = (
        id_of(&mut h, "file.zip"),
        id_of(&mut h, "gone.iso"),
        id_of(&mut h, "stalled.bin"),
    );
    h.until("one done, one failed, one running", |h| {
        state_of(h, done) == Some(State::Completed)
            && matches!(state_of(h, gone), Some(State::Failed(_)))
            && state_of(h, running) == Some(State::Running)
    });
    let why = |said: &Value, id: u64| {
        said["skipped"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id)
            .map(|s| s["why"].as_str().unwrap().to_string())
    };

    let said = h
        .call(
            "downloads.resume",
            json!({ "ids": [done, gone, running, 99999] }),
        )
        .unwrap();
    assert_eq!(said["done"], json!([gone]), "a failed one is tried again");
    assert_eq!(why(&said, running).as_deref(), Some("it is not paused or failed"));
    assert_eq!(why(&said, 99999).as_deref(), Some("no such download"));

    let said = h
        .call("downloads.pause", json!({ "ids": [done, running] }))
        .unwrap();
    assert_eq!(said["done"], json!([running]));
    assert_eq!(why(&said, done).as_deref(), Some("it is not running"));
    h.until("it to pause", |h| state_of(h, running) == Some(State::Paused));

    let said = h
        .call("downloads.resume", json!({ "ids": [running] }))
        .unwrap();
    assert_eq!(said["done"], json!([running]));
    h.until("it to run again", |h| {
        state_of(h, running) == Some(State::Running)
    });
    let said = h
        .call("downloads.cancel", json!({ "ids": [done, running] }))
        .unwrap();
    assert_eq!(said["done"], json!([running]));
    assert_eq!(why(&said, done).as_deref(), Some("it is already settled"));
    h.until("the cancel", |h| state_of(h, running) == Some(State::Cancelled));
    // And the window shows it.
    h.until("the row to say so", |h| {
        h.rows()
            .iter()
            .any(|r| r.id == running && r.state == State::Cancelled)
    });
}

#[test]
fn a_download_the_schedule_holds_is_not_resumed_over_the_port() {
    let _file = settings_file();
    let world = world();
    let mut settings = world.settings();
    settings.allow_mcp = true;
    // Closed until a day far off.
    settings.schedule = dm_core::Schedule {
        enabled: true,
        once_at: Some("2099-01-01 00:00".into()),
        to: "01:00".into(),
        ..dm_core::Schedule::default()
    };
    let mut h = world.harness_with(settings);
    h.call("downloads.add", json!({ "urls": [world.url("stalled.bin")] }))
        .unwrap();
    let id = id_of(&mut h, "stalled.bin");
    h.until("the schedule to hold it", |h| {
        h.view(|view| {
            view.backend
                .row(id)
                .is_some_and(|r| r.state == State::Paused && r.paused_by_schedule)
        })
    });
    let said = h
        .call("downloads.resume", json!({ "ids": [id] }))
        .unwrap();
    assert_eq!(said["done"], json!([]));
    assert_eq!(
        said["skipped"][0]["why"],
        "the schedule or the download limit holds it"
    );
    assert_eq!(state_of(&mut h, id), Some(State::Paused));
}

/// The speed limit set exactly, the one before kept; downloads added with their
/// descriptions, folders, names and parts; a batch of steps answered each.
#[test]
fn the_port_sets_the_limit_adds_where_downloads_may_go_and_answers_a_batch() {
    let _file = settings_file();
    let world = world();
    let mut h = harness(&world);
    let set = h
        .call("downloads.speed_limit", json!({ "kb": 500 }))
        .unwrap();
    assert_eq!(set, json!({ "speed_limit_kb": 500 }));
    assert_eq!(h.view(|view| view.backend.settings().speed_limit), 500);
    let off = h.call("downloads.speed_limit", json!({ "kb": 0 })).unwrap();
    assert_eq!(off, json!({ "speed_limit_kb": 0 }));
    let settings = h.view(|view| view.backend.settings());
    assert_eq!(settings.speed_limit, 0);
    assert_eq!(settings.last_speed_limit, 500, "the one before");

    let said = h
        .call(
            "downloads.add",
            json!({ "url": world.url("single.bin"), "later": true, "description": "the one" }),
        )
        .unwrap();
    assert_eq!(said, json!({ "added": 1, "already_listed": [] }));
    h.until("the row", |h| h.rows().len() == 1);
    let row = h.rows()[0].clone();
    assert_eq!(row.description, "the one");
    assert_eq!(row.queue, "Main queue");
    // Listed already, and named once however often it is asked for, unless asked again.
    let twice = world.url("single.bin");
    let said = h
        .call(
            "downloads.add",
            json!({ "urls": [twice, twice, twice], "later": true }),
        )
        .unwrap();
    assert_eq!(said, json!({ "added": 0, "already_listed": [twice] }));
    let said = h
        .call(
            "downloads.add",
            json!({ "urls": [twice], "later": true, "again": true }),
        )
        .unwrap();
    assert_eq!(said["added"], 1);

    let outside = world.dir.path().join("elsewhere");
    let refused = h
        .call(
            "downloads.add",
            json!({ "urls": [world.url("f.bin")], "later": true,
                    "folder": outside.display().to_string() }),
        )
        .unwrap_err();
    assert!(refused.contains("inside the download folder"), "{refused}");
    let inside = world.dir.path().join("out").join("sub");
    let said = h
        .call(
            "downloads.add",
            json!({ "urls": [world.url("f.bin")], "later": true,
                    "folder": inside.display().to_string() }),
        )
        .unwrap();
    assert_eq!(said["added"], 1, "{said}");
    let f = id_of(&mut h, "f.bin");
    let row = h.view(|view| view.backend.row(f)).unwrap();
    assert!(
        row.save_dir.as_deref().is_some_and(|d| d.ends_with("sub")),
        "{:?}",
        row.save_dir
    );
    let said = h
        .call(
            "downloads.add",
            json!({ "urls": [world.url("clip.mp4")], "later": true, "from": "0:10", "to": "75" }),
        )
        .unwrap();
    assert_eq!(said["added"], 1);
    let clip = id_of(&mut h, "clip.mp4");
    assert!(
        h.view(|view| view.backend.row(clip)).unwrap().span.is_some(),
        "the part was not kept"
    );

    let said = h
        .call(
            "batch",
            json!({ "steps": [
                { "method": "batch", "params": { "steps": [] } },
                { "method": "queue.list" },
                { "method": "ui.select" },
            ]}),
        )
        .unwrap();
    let said = said.as_array().unwrap();
    assert_eq!(said[0]["error"], "batch cannot be batched");
    assert_eq!(said[1]["ok"], true);
    assert_eq!(said[1]["result"].as_array().map(Vec::len), Some(4));
    assert_eq!(said[2]["result"], json!({ "selected": [] }));
}

/// AI assistants are answered only while Options lets them in; a port opened for assistants
/// answers nobody else.
#[test]
fn assistants_are_answered_only_while_options_lets_them_and_a_port_for_them_alone_is_theirs() {
    let _file = settings_file();
    let world = world();
    let mut h = harness(&world);
    assert_eq!(h.call_as_assistant("queue.list", json!({})).unwrap(), json!([]));
    h.settings_to(|s| s.allow_mcp = false);
    let refused = h.call_as_assistant("queue.list", json!({})).unwrap_err();
    assert!(refused.contains("turned off"), "{refused}");
    assert!(h.call("queue.list", json!({})).is_ok());
    h.settings_to(|s| s.allow_mcp = true);

    h.assistants_only(true);
    let refused = h.call("queue.list", json!({})).unwrap_err();
    assert!(refused.contains("AI assistants only"), "{refused}");
    let unknown = h.call_as_assistant("nonsense", json!({})).unwrap_err();
    assert!(unknown.contains("no method is called nonsense"), "{unknown}");
}

/// The JSON-RPC line `dm mcp` is sent for the tool `name`.
fn tool(id: u64, name: &str, arguments: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": name, "arguments": arguments } })
}

/// An assistant, through `dm mcp` and a real control port over the window: adds a download,
/// reads the screen, clicks, is refused what runs a file, and sees the dialog it opened.
#[test]
fn an_assistant_drives_the_window_through_its_tools() {
    let _file = settings_file();
    let world = world();
    let mut h = harness(&world);
    let secret = "t".repeat(64);
    let port =
        dm_desktop_base::control::listen(0, secret.clone(), h.requests(), || {}).unwrap();
    let url = world.url("tooled.bin");
    let client = std::thread::spawn(move || {
        let mut control = dm_desktop_base::mcp::Control::connect(port, &secret).unwrap();
        let lines = [
            tool(1, "dm_add_downloads", json!({ "urls": [url], "later": true })),
            tool(2, "dm_screen", json!({})),
            tool(3, "dm_click", json!({ "text": "Add URL" })),
            tool(4, "dm_click", json!({ "text": "Open" })),
            tool(5, "dm_inspect", json!({})),
        ];
        let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
        let mut out = Vec::new();
        dm_desktop_base::mcp::serve(std::io::Cursor::new(input), &mut out, &mut control)
            .unwrap();
        String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .collect::<Vec<_>>()
    });
    h.until_within(
        "the assistant's five tools",
        std::time::Duration::from_secs(60),
        &mut |_| client.is_finished(),
    );
    let said = client.join().unwrap();
    let text = |n: usize| said[n]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text(0).contains("\"added\":1"), "{}", said[0]);
    assert!(text(1).contains("Add URL"), "{}", said[1]);
    assert!(text(2).contains("\"clicked\":\"Add URL\""), "{}", said[2]);
    assert_eq!(said[3]["result"]["isError"], true);
    assert!(text(3).contains("runs a downloaded file"), "{}", said[3]);
    assert!(text(4).contains("Add URL"), "the dialog is open: {}", said[4]);
    h.until("Add URL", |h| h.dialog_open("Add URL"));
    assert_eq!(h.rows().len(), 1);
}
