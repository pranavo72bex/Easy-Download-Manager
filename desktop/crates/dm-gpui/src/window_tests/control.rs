// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The control port and the MCP bridge against the window: what a script, the command line
//! or an AI assistant asks, and what it may not. Ported from the egui window's
//! `tests/window/control.rs`.

use dm_core::State;
use dm_shell::app::power::WhenDone;
use serde_json::json;

use crate::harness::world::world;
use crate::harness::{settings_file, Harness};

/// Options › General › Let AI assistants use dm over MCP, set as Options' OK sets it.
fn allow_mcp(h: &mut Harness, on: bool) {
    h.apply(|s| s.allow_mcp = on);
}

/// Steps the window until `client`'s thread is done, as frames are drawn while a client
/// waits for its answers; its result.
fn served<T>(h: &mut Harness, client: std::thread::JoinHandle<T>) -> T {
    h.until("the client to be answered", |_| client.is_finished());
    client.join().expect("the client's checks failed")
}

#[test]
fn the_command_line_pauses_resumes_and_cancels_by_number_and_leaves_the_selection_alone() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("stalled.bin to run", |h| {
        h.rows().iter().any(|r| r.state == State::Running)
    });
    let id = h.row("stalled.bin").id;
    let ids = json!({ "ids": [id, 99999] });
    let state_is =
        |h: &mut Harness, wanted: State| h.rows().iter().any(|r| r.id == id && r.state == wanted);

    let chosen = h.view(|view| view.selected.clone());
    let said = h.call("downloads.pause", ids.clone()).unwrap();
    assert_eq!(said["done"], json!([id]));
    assert_eq!(said["skipped"][0]["why"], "no such download");
    h.until("the pause", |h| state_is(h, State::Paused));
    assert_eq!(
        h.view(|view| view.selected.clone()),
        chosen,
        "the selection is the person's"
    );
    // Pausing a paused row is said, not done.
    assert_eq!(
        h.call("downloads.pause", ids.clone()).unwrap()["done"],
        json!([])
    );

    assert_eq!(
        h.call("downloads.resume", ids.clone()).unwrap()["done"],
        json!([id])
    );
    h.until("the resume", |h| state_is(h, State::Running));
    assert_eq!(
        h.call("downloads.cancel", ids.clone()).unwrap()["done"],
        json!([id])
    );
    h.until("the cancel", |h| state_is(h, State::Cancelled));
    assert!(h.call("downloads.pause", json!({})).is_err());
}

/// `--when-done` from the command line, left in the inbox, sets When done for the session
/// once the window next looks. The word left is "nothing", after Sleep is chosen: the inbox is
/// every test window's, and another window that took it first is left as it was.
#[test]
fn the_command_lines_when_done_is_taken_up() {
    let world = world();
    let mut h = world.harness();
    h.command("downloads.when_done.sleep");
    assert_eq!(h.view(|view| view.session.when_done), WhenDone::Sleep);
    let home = dm_shell::settings::home();
    let orders = dm_shell::shell::orders(&["--when-done".into(), "nothing".into()]);
    let left = dm_shell::host::inbox(&home).join(dm_shell::shell::WHEN_DONE_FILE);
    h.until("When done to be set", |h| {
        if !left.exists() {
            dm_shell::shell::leave_when_done(&home, &orders);
        }
        h.view(|view| view.session.when_done) == WhenDone::Nothing
    });
}

/// The control protocol, over a real TCP connection, against the window: what an agent or a
/// test outside the process uses to drive dm.
#[test]
fn the_control_protocol_runs_commands_and_reads_the_window_back() {
    use std::io::{BufRead, BufReader, Write};
    let world = world();
    let mut h = world.harness();
    let token = "f".repeat(64);
    let port = h.open_port(&token, false);

    let client = std::thread::spawn(move || {
        let stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut out = stream.try_clone().unwrap();
        let mut replies = BufReader::new(stream);
        let mut call = |method: &str, params: serde_json::Value| -> serde_json::Value {
            let line = json!({ "id": 1, "method": method, "params": params });
            out.write_all(format!("{line}\n").as_bytes()).unwrap();
            let mut reply = String::new();
            replies.read_line(&mut reply).unwrap();
            serde_json::from_str(&reply).unwrap()
        };
        let auth = call("auth", json!({ "token": token }));
        assert_eq!(auth["ok"], true, "{auth}");
        let commands = call("commands.list", json!({}));
        let ids: Vec<&str> = commands["result"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|c| c["id"].as_str())
            .collect();
        assert!(
            ids.contains(&"program.add_url") && ids.contains(&"downloads.start_queue"),
            "{ids:?}"
        );
        let refused = call("command.run", json!({ "id": "file.remove" }));
        assert_eq!(
            refused["ok"], false,
            "removing nothing was allowed: {refused}"
        );
        let opened = call("command.run", json!({ "id": "program.add_url" }));
        assert_eq!(opened["ok"], true, "{opened}");
        let seen = call("ui.inspect", json!({}));
        let dialogs = seen["result"]["dialogs"].as_array().unwrap().clone();
        assert!(dialogs.iter().any(|d| d == "Add URL"), "{dialogs:?}");
        let unknown = call("nonsense", json!({}));
        assert_eq!(unknown["ok"], false);
    });
    served(&mut h, client);
    assert!(
        h.dialog_open("Add URL"),
        "the command never reached the window"
    );
}

/// The MCP bridge, as an AI client would use it: tools listed, a download added and listed
/// back, a wrong tool refused — through a real control connection to the window.
#[test]
fn the_mcp_bridge_gives_an_agent_dm_as_tools() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    allow_mcp(&mut h, true);
    let token = "e".repeat(64);
    let port = h.open_port(&token, false);
    let url = world.url("file.zip");

    let client = std::thread::spawn(move || {
        let mut control = dm_desktop_base::mcp::Control::connect(port, &token).unwrap();
        let script = [
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#.to_string(),
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#.to_string(),
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#.to_string(),
            format!(r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"dm_add_downloads","arguments":{{"urls":["{url}"],"later":true}}}}}}"#),
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"dm_list_downloads","arguments":{}}}"#.to_string(),
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"dm_nonsense"}}"#.to_string(),
            "not json".to_string(),
        ]
        .join("\n");
        let mut out = Vec::new();
        dm_desktop_base::mcp::serve(std::io::Cursor::new(script), &mut out, &mut control).unwrap();
        String::from_utf8(out).unwrap()
    });
    let out = served(&mut h, client);
    let replies: Vec<serde_json::Value> = out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        replies.len(),
        6,
        "a notification was answered, or a request was not: {out}"
    );
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "dm");
    let tools = replies[1]["result"]["tools"].as_array().unwrap();
    assert!(tools.iter().any(|t| t["name"] == "dm_add_downloads"));
    assert_eq!(
        replies[2]["result"]["isError"],
        serde_json::Value::Null,
        "{}",
        replies[2]
    );
    assert!(replies[3]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("file.zip"));
    assert_eq!(replies[4]["error"]["code"], -32602);
    assert_eq!(replies[5]["error"]["code"], -32700);
    h.until("the agent's download in the window", |h| {
        h.rows().len() == 1
    });
}

/// An address already listed is not added again over the control protocol (or MCP) unless
/// `again` says so, and the reply says which were skipped.
#[test]
fn the_control_protocol_does_not_add_an_address_twice_unless_asked() {
    let world = world();
    let mut h = world.harness();
    let url = world.url("twice.bin");
    let add = |h: &mut Harness, again: bool| {
        h.call(
            "downloads.add",
            json!({ "urls": [url], "later": true, "again": again }),
        )
        .unwrap()
    };
    let first = add(&mut h, false);
    assert_eq!(first["added"], 1, "{first}");
    h.until("the row", |h| h.rows().len() == 1);
    let second = add(&mut h, false);
    assert_eq!(second["added"], 0, "{second}");
    assert_eq!(second["already_listed"], json!([url]), "{second}");
    let forced = add(&mut h, true);
    assert_eq!(forced["added"], 1, "{forced}");
    h.until("both rows", |h| h.rows().len() == 2);
}

/// An empty list of links was answered "added 0" as a success, and any `queue:` name — one
/// never made, or 100 KB of one — was taken as a place.
#[test]
fn the_control_protocol_refuses_nothing_to_add_and_a_queue_that_is_not_there() {
    let world = world();
    let mut h = world.harness();
    for empty in [
        json!({ "urls": [] }),
        json!({ "urls": [""] }),
        json!({ "urls": "" }),
    ] {
        assert!(
            h.call("downloads.add", empty.clone()).is_err(),
            "{empty} was added"
        );
    }
    for nowhere in [
        "queue:nowhere".to_string(),
        format!("queue:{}", "q".repeat(100_000)),
    ] {
        assert!(h.call("ui.place", json!({ "place": nowhere })).is_err());
    }
    assert!(h
        .call("ui.place", json!({ "place": "queue:Main queue" }))
        .is_ok());
}

/// `ui.window` puts the window where a person would: back from the tray through the window's
/// own asks, and nowhere it does not name. The tray is refused here: the tests' window has
/// none (`start_tray` is off under test), as a desktop with no tray refuses it; and GPUI's test
/// platform cannot minimize a window, so "minimized" is not asked.
#[test]
fn the_window_is_put_in_the_tray_and_shown_again_over_the_control_protocol() {
    let world = world();
    let mut h = world.harness();
    let to = |h: &mut Harness, to: &str| h.call("ui.window", json!({ "to": to }));
    let tray = to(&mut h, "tray");
    assert!(
        matches!(&tray, Err(why) if why.contains("no tray")),
        "{tray:?}"
    );
    // Parked in the tray, as the close button leaves it, and shown again.
    h.view(|view| view.hidden = true);
    assert!(to(&mut h, "shown").is_ok());
    assert!(!h.view(|view| view.hidden), "not shown again");
    assert!(to(&mut h, "away").is_err());
}

/// Opening a file runs it if it is a program: never at a script's word.
#[test]
fn opening_a_file_is_refused_over_the_control_protocol() {
    let world = world();
    let mut h = world.harness();
    for id in ["file.open", "file.open_with"] {
        let refused = h.call("command.run", json!({ "id": id })).expect_err(id);
        assert!(refused.contains("by a person"), "{refused}");
    }
}

#[test]
fn a_batch_runs_its_steps_in_order() {
    let world = world();
    let mut h = world.harness();
    let steps = json!({ "steps": [
        { "method": "downloads.add", "params": { "urls": [world.url("a.bin"), world.url("b.bin")], "later": true } },
        { "method": "ui.place", "params": { "place": "queue:Main queue" } },
        { "method": "nonsense" },
        { "method": "ui.inspect" },
    ]});
    let results = h.call("batch", steps).unwrap();
    let results = results.as_array().unwrap();
    assert_eq!(results.len(), 4);
    assert_eq!(results[0]["result"]["added"], 2);
    assert_eq!(results[2]["ok"], false);
    assert_eq!(results[3]["result"]["rows"].as_array().unwrap().len(), 2);
    let too_many = json!({ "steps": vec![json!({"method": "ui.inspect"}); 257] });
    assert!(h.call("batch", too_many).is_err());
}

/// What a client asks of the window as a person would is refused while the window is hidden
/// in the tray, where no one sees what it would act on; a method that is not a person's still
/// goes. The egui window also capped the steps waiting for its frames: the GPUI window does
/// each step as it is asked, so none waits.
#[test]
fn steps_waiting_for_frames_are_capped_and_refused_while_hidden() {
    let world = world();
    let mut h = world.harness();
    assert!(h.call("ui.key", json!({ "key": "A" })).is_ok());
    h.view(|view| view.hidden = true);
    for (method, params) in [
        ("ui.click", json!({ "text": "Add URL" })),
        ("ui.key", json!({ "key": "A" })),
        ("ui.type", json!({ "text": "x" })),
    ] {
        let hidden = h.call(method, params);
        assert!(
            matches!(&hidden, Err(why) if why.contains("hidden")),
            "{method}: {hidden:?}"
        );
    }
    assert!(!h.dialog_open("Add URL"), "the click went through");
    // And a method that is not a person's is none of its business.
    assert!(h.call("queue.list", json!({})).is_ok());
}

/// Options › General › Let AI assistants use dm over MCP: off in a new install (gaps row F7),
/// asked afresh with every request, and said plainly when it refuses.
#[test]
fn ai_assistants_are_answered_only_while_the_setting_is_on() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    assert!(
        !dm_shell::Settings::default().allow_mcp,
        "off in a new install"
    );
    allow_mcp(&mut h, true);
    let token = "f".repeat(64);
    let port = h.open_port(&token, false);

    let (ask, asked) = std::sync::mpsc::channel::<()>();
    let (answer, answers) = std::sync::mpsc::channel::<Result<serde_json::Value, String>>();
    let client = std::thread::spawn(move || {
        let mut mcp = dm_desktop_base::mcp::Control::connect(port, &token).unwrap();
        // A client that is not MCP: the command line's kind.
        let mut plain = dm_desktop_base::mcp::Control::connect_as(port, &token, None).unwrap();
        while asked.recv().is_ok() {
            // The list, answered on the port's own thread, and a method the window answers.
            for method in ["queue.list", "ui.inspect"] {
                answer.send(mcp.call(method, json!({}))).unwrap();
                answer.send(plain.call(method, json!({}))).unwrap();
            }
        }
    });
    let round = |h: &mut Harness| {
        ask.send(()).unwrap();
        let mut got = Vec::new();
        h.until("the answers", |_| {
            got.extend(answers.try_iter());
            got.len() == 4
        });
        got
    };
    let on = round(&mut h);
    assert!(on.iter().all(Result::is_ok), "refused while on: {on:?}");
    allow_mcp(&mut h, false);
    let off = round(&mut h);
    for at in [0, 2] {
        assert!(
            matches!(&off[at], Err(why) if why.contains("turned off")),
            "turning it off did not take hold on the open connection: {off:?}"
        );
        assert!(
            off[at + 1].is_ok(),
            "the command line's kind of client is not an AI assistant's: {off:?}"
        );
    }
    allow_mcp(&mut h, true);
    let on_again = round(&mut h);
    assert!(
        on_again.iter().all(Result::is_ok),
        "refused after turning it back on: {on_again:?}"
    );
    drop(ask);
    served(&mut h, client);
}

/// A port opened only because AI assistants are allowed answers assistants and nothing else:
/// the command line is not handed over by that default.
#[test]
fn a_port_opened_for_assistants_alone_refuses_other_clients() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    // Assistants let in: off in a new install (gaps row F7).
    allow_mcp(&mut h, true);
    let token = "9".repeat(64);
    let port = h.open_port(&token, true);
    let client = std::thread::spawn(move || {
        let mut mcp = dm_desktop_base::mcp::Control::connect(port, &token).unwrap();
        let mut plain = dm_desktop_base::mcp::Control::connect_as(port, &token, None).unwrap();
        ["queue.list", "ui.inspect"]
            .map(|method| (mcp.call(method, json!({})), plain.call(method, json!({}))))
    });
    for (assistant, other) in served(&mut h, client) {
        assert!(assistant.is_ok(), "{assistant:?}");
        assert!(
            matches!(&other, Err(why) if why.contains("AI assistants only")),
            "{other:?}"
        );
    }
}

/// A pause followed at once by a resume, before the window has read the list again: the resume
/// is answered from the queue, not from the window's copy, which still says the download runs.
/// It was refused as "not paused", and the download stayed stopped.
#[test]
fn a_resume_straight_after_a_pause_is_not_refused_by_a_stale_list() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("stalled.bin to run", |h| {
        h.rows().iter().any(|r| r.state == State::Running)
    });
    let id = h.row("stalled.bin").id;
    let ids = json!({ "ids": [id] });
    // Both in one batch: no read of the list between them.
    let said = h
        .call(
            "batch",
            json!({ "steps": [
                { "method": "downloads.pause", "params": ids },
                { "method": "downloads.resume", "params": ids },
            ]}),
        )
        .unwrap();
    assert_eq!(said[0]["result"]["done"], json!([id]), "{said}");
    assert_eq!(
        said[1]["result"]["done"],
        json!([id]),
        "the resume was refused: {said}"
    );
    h.until("it to run again", |h| {
        h.rows()
            .iter()
            .any(|r| r.id == id && r.state == State::Running)
    });
}

/// A program run at the end of a queue's own hours is a person's to set, as the shared
/// schedule's is: settings put in force a moment after a script drove the window keep the
/// program the queue had.
#[test]
fn a_script_driving_the_window_cannot_set_the_program_a_queues_hours_run() {
    // Its settings are applied, and so saved.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.key("escape", &[]);
    assert!(h.view(|view| view.scripted.by_script()));
    let hours = dm_core::Schedule {
        enabled: true,
        run_when_done: "sh -c 'evil'".into(),
        ..dm_core::Schedule::default()
    };
    h.apply(|wanted| {
        wanted.queue_schedules.insert("Main queue".into(), hours);
        wanted.proxy.script = "http://evil.example/p.pac".into();
    });
    let saved = h.view(|view| view.backend.settings());
    let kept = saved
        .queue_schedules
        .get("Main queue")
        .map(|s| s.run_when_done.as_str());
    assert_eq!(kept, Some(""), "{kept:?}");
    assert_eq!(saved.proxy.script, "", "a script set the proxy script");
}
