use crate::support::*;

#[test]
fn the_command_line_pauses_resumes_and_cancels_by_number_and_leaves_the_selection_alone() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("stalled.bin"), "Start now");
    until(&mut harness, "stalled.bin to run", |app| {
        app.rows().iter().any(|r| r.state == State::Running)
    });
    let id = row(&harness, "stalled.bin").id;
    let ids = serde_json::json!({ "ids": [id, 99999] });
    let call = |h: &mut Harness<'_, DmApp>, method: &str| {
        h.state_mut().control_call(method, &ids).unwrap()
    };
    let state = |h: &Harness<'_, DmApp>| {
        h.state()
            .rows()
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.state.clone())
    };

    let said = call(&mut harness, "downloads.pause");
    assert_eq!(said["done"], serde_json::json!([id]));
    assert_eq!(said["skipped"][0]["why"], "no such download");
    until(&mut harness, "the pause", |app| {
        app.rows()
            .iter()
            .any(|r| r.id == id && r.state == State::Paused)
    });
    assert!(
        harness.state().selected.is_empty(),
        "the selection is the person's"
    );
    // Pausing a paused row is said, not done.
    assert_eq!(
        call(&mut harness, "downloads.pause")["done"],
        serde_json::json!([])
    );

    assert_eq!(
        call(&mut harness, "downloads.resume")["done"],
        serde_json::json!([id])
    );
    until(&mut harness, "the resume", |app| {
        app.rows()
            .iter()
            .any(|r| r.id == id && r.state == State::Running)
    });
    assert_eq!(
        call(&mut harness, "downloads.cancel")["done"],
        serde_json::json!([id])
    );
    until(&mut harness, "the cancel", |app| {
        app.rows()
            .iter()
            .any(|r| r.id == id && r.state == State::Cancelled)
    });
    assert_eq!(state(&harness), Some(State::Cancelled));
    assert!(harness
        .state_mut()
        .control_call("downloads.pause", &serde_json::json!({}))
        .is_err());
}

/// `--when-done` from the command line, left in the inbox, sets When done
/// for the session once the window next looks.
#[test]
fn the_command_lines_when_done_is_taken_up() {
    use dm_desktop::power::WhenDone;
    let world = world();
    let mut harness = world.harness();
    let home = dm_shell::settings::home();
    let orders = dm_shell::shell::orders(&["--when-done".into(), "exit".into()]);
    dm_shell::shell::leave_when_done(&home, &orders);
    until(&mut harness, "When done to be set", |app| {
        app.when_done == WhenDone::Quit
    });
    assert!(!dm_shell::host::inbox(&home)
        .join(dm_shell::shell::WHEN_DONE_FILE)
        .exists());
}

/// The control protocol, over a real TCP connection, against the window:
/// what an agent or a test outside the process uses to drive dm.
#[test]
fn the_control_protocol_runs_commands_and_reads_the_window_back() {
    use std::io::{BufRead, BufReader, Write};
    let world = world();
    let mut harness = world.harness();
    let (to_window, requests) = std::sync::mpsc::channel();
    harness.state_mut().attach_control(requests);
    let token = "f".repeat(64);
    let port = dm_desktop::control::listen(0, token.clone(), to_window, || {}).unwrap();
    let url = world.url("file.zip");

    let client = std::thread::spawn(move || {
        let stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut out = stream.try_clone().unwrap();
        let mut replies = BufReader::new(stream);
        let mut call = |method: &str, params: serde_json::Value| -> serde_json::Value {
            let line = serde_json::json!({ "id": 1, "method": method, "params": params });
            out.write_all(format!("{line}\n").as_bytes()).unwrap();
            let mut reply = String::new();
            replies.read_line(&mut reply).unwrap();
            serde_json::from_str(&reply).unwrap()
        };
        let auth = call("auth", serde_json::json!({ "token": token }));
        assert_eq!(auth["ok"], true, "{auth}");
        let commands = call("commands.list", serde_json::json!({}));
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
        let refused = call("command.run", serde_json::json!({ "id": "file.remove" }));
        assert_eq!(
            refused["ok"], false,
            "removing nothing was allowed: {refused}"
        );
        let opened = call(
            "command.run",
            serde_json::json!({ "id": "program.add_url" }),
        );
        assert_eq!(opened["ok"], true, "{opened}");
        let seen = call("ui.inspect", serde_json::json!({}));
        let dialogs = seen["result"]["dialogs"].as_array().unwrap().clone();
        assert!(dialogs.iter().any(|d| d == "Add URL"), "{dialogs:?}");
        let unknown = call("nonsense", serde_json::json!({}));
        assert_eq!(unknown["ok"], false);
        url
    });

    // The window answers on its own thread, as frames are drawn.
    while !client.is_finished() {
        harness.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    client.join().expect("the client's checks failed");
    assert!(
        harness.state().dialogs.add_url.is_some(),
        "the command never reached the window"
    );
}

/// The MCP bridge, as an AI client would use it: tools listed, a download
/// added and listed back, a wrong tool refused — through a real control
/// connection to the window.
#[test]
fn the_mcp_bridge_gives_an_agent_dm_as_tools() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    allow_mcp(&mut harness, true);
    let (to_window, requests) = std::sync::mpsc::channel();
    harness.state_mut().attach_control(requests);
    let token = "e".repeat(64);
    let port = dm_desktop::control::listen(0, token.clone(), to_window, || {}).unwrap();
    let url = world.url("file.zip");

    let client = std::thread::spawn(move || {
        let mut control = dm_desktop::mcp::Control::connect(port, &token).unwrap();
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
        dm_desktop::mcp::serve(std::io::Cursor::new(script), &mut out, &mut control).unwrap();
        String::from_utf8(out).unwrap()
    });
    while !client.is_finished() {
        harness.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    let out = client.join().unwrap();
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
    assert_eq!(
        harness.state().rows().len(),
        1,
        "the agent's download is not in the window"
    );
}

/// An address already listed is not added again over the control protocol
/// (or MCP) unless `again` says so, and the reply says which were skipped.
#[test]
fn the_control_protocol_does_not_add_an_address_twice_unless_asked() {
    let world = world();
    let mut harness = world.harness();
    let app = harness.state_mut();
    let url = world.url("twice.bin");
    let first = app
        .control_call(
            "downloads.add",
            &serde_json::json!({ "urls": [url], "later": true }),
        )
        .unwrap();
    assert_eq!(first["added"], 1, "{first}");
    app.refresh();
    let second = app
        .control_call(
            "downloads.add",
            &serde_json::json!({ "urls": [url], "later": true }),
        )
        .unwrap();
    assert_eq!(second["added"], 0, "{second}");
    assert_eq!(
        second["already_listed"],
        serde_json::json!([url]),
        "{second}"
    );
    let forced = app
        .control_call(
            "downloads.add",
            &serde_json::json!({ "urls": [url], "later": true, "again": true }),
        )
        .unwrap();
    assert_eq!(forced["added"], 1, "{forced}");
    app.refresh();
    assert_eq!(app.rows().len(), 2);
}

/// An empty list of links was answered "added 0" as a success, and any
/// `queue:` name — one never made, or 100 KB of one — was taken as a place.
#[test]
fn the_control_protocol_refuses_nothing_to_add_and_a_queue_that_is_not_there() {
    let world = world();
    let mut harness = world.harness();
    let app = harness.state_mut();
    for empty in [
        serde_json::json!({ "urls": [] }),
        serde_json::json!({ "urls": [""] }),
        serde_json::json!({ "urls": "" }),
    ] {
        assert!(
            app.control_call("downloads.add", &empty).is_err(),
            "{empty} was added"
        );
    }
    for nowhere in [
        "queue:nowhere".to_string(),
        format!("queue:{}", "q".repeat(100_000)),
    ] {
        assert!(app
            .control_call("ui.place", &serde_json::json!({ "place": nowhere }))
            .is_err());
    }
    assert!(app
        .control_call(
            "ui.place",
            &serde_json::json!({ "place": "queue:Main queue" })
        )
        .is_ok());
}

/// Opening a file runs it if it is a program: never at a script's word.
#[test]
fn opening_a_file_is_refused_over_the_control_protocol() {
    let world = world();
    let mut harness = world.harness();
    let app = harness.state_mut();
    for id in ["file.open", "file.open_with"] {
        let refused = app
            .control_call("command.run", &serde_json::json!({ "id": id }))
            .expect_err(id);
        assert!(refused.contains("by a person"), "{refused}");
    }
}

#[test]
fn a_batch_runs_its_steps_in_order() {
    let world = world();
    let mut harness = world.harness();
    let steps = serde_json::json!({ "steps": [
        { "method": "downloads.add", "params": { "urls": [world.url("a.bin"), world.url("b.bin")], "later": true } },
        { "method": "ui.place", "params": { "place": "queue:Main queue" } },
        { "method": "nonsense" },
        { "method": "ui.inspect" },
    ]});
    let results = harness.state_mut().control_call("batch", &steps).unwrap();
    let results = results.as_array().unwrap();
    assert_eq!(results.len(), 4);
    assert_eq!(results[0]["result"]["added"], 2);
    assert_eq!(results[2]["ok"], false);
    assert_eq!(results[3]["result"]["rows"].as_array().unwrap().len(), 2);
    let too_many =
        serde_json::json!({ "steps": vec![serde_json::json!({"method": "ui.inspect"}); 257] });
    assert!(harness
        .state_mut()
        .control_call("batch", &too_many)
        .is_err());
}

/// The pictures in the README, rendered offscreen from a queue the engine
/// really served — the sizes, rates and times are its, not a mock-up's.
///
///     DM_SHOTS=docs cargo test -p dm-desktop --test window shots
///
/// Writes screenshot.png, screenshot-dark.png and settings.png there.
#[test]
fn shots() {
    let Some(dir) = std::env::var_os("DM_SHOTS").map(std::path::PathBuf::from) else {
        return;
    };
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "a finished row", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    add_url(&mut harness, &world.url("stalled.bin"), "Start now");
    add_url(&mut harness, &world.url("gone.iso"), "Start now");
    add_url(&mut harness, &world.url("later.mkv"), "Add for later");
    until(&mut harness, "a failed row", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    select(&mut harness, "stalled.bin");
    harness.run_steps(4);

    // dm's own looks, chosen as Options › Look chooses them: Light
    // and Dark. A few frames let the embedded font come into force.
    let render = |harness: &mut Harness<'_, DmApp>, look: &str, name: &str| {
        harness.state_mut().applied.theme = look.into();
        // No test pointer over the picture, and a download folder a reader
        // would recognise rather than the test's temporary one.
        harness.event(egui::Event::PointerGone);
        harness.state_mut().applied.download_dir = "~/Downloads".into();
        harness.run_steps(6);
        let image = harness.render().expect("the window could not be rendered");
        image.save(dir.join(name)).unwrap();
    };
    render(&mut harness, "light", "screenshot.png");
    render(&mut harness, "dark", "screenshot-dark.png");
    let id = harness.state().targets()[0].id;
    harness.state_mut().dialogs.progress.insert(id);
    render(&mut harness, "light", "progress.png");
    render(&mut harness, "dark", "progress-dark.png");
    harness.state_mut().dialogs.progress.clear();
    let settings = harness.state().applied.clone();
    harness.state_mut().dialogs.open_options(&settings);
    render(&mut harness, "light", "settings.png");
    render(&mut harness, "dark", "settings-dark.png");
}

/// What a client asks of the window as a person would waits for frames, so
/// it may not pile up without end, nor be queued for a window that draws
/// none.
#[test]
fn steps_waiting_for_frames_are_capped_and_refused_while_hidden() {
    let world = world();
    let mut harness = world.harness();
    let key = serde_json::json!({ "key": "A" });
    // No frame runs between these, so each one waits (a key is two steps).
    let mut taken = 0;
    let refused = loop {
        let answer = harness.state_mut().human_call("ui.key", &key, None);
        match answer {
            Some(Ok(_)) => taken += 1,
            other => break other,
        }
        assert!(taken < 100, "nothing was ever refused");
    };
    assert!(taken >= 10, "refused too soon: {taken}");
    assert!(
        matches!(&refused, Some(Err(why)) if why.contains("too many steps")),
        "{refused:?}"
    );
    harness.run_steps(4);
    harness.state_mut().hidden = true;
    let hidden =
        harness
            .state_mut()
            .human_call("ui.click", &serde_json::json!({ "text": "Add URL" }), None);
    assert!(
        matches!(&hidden, Some(Err(why)) if why.contains("hidden")),
        "{hidden:?}"
    );
    // And a method that is not a person's is none of its business.
    assert!(harness
        .state_mut()
        .human_call("queue.list", &key, None)
        .is_none());
}

fn allow_mcp(harness: &mut Harness<'_, DmApp>, on: bool) {
    let mut wanted = harness.state().applied.clone();
    wanted.allow_mcp = on;
    harness.state_mut().apply_settings(wanted).unwrap();
}

/// Options › General › Let AI assistants use dm over MCP: on unless unticked,
/// asked afresh with every request, and said plainly when it refuses.
#[test]
fn ai_assistants_are_answered_only_while_the_setting_is_on() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    assert!(harness.state().applied.allow_mcp, "on unless turned off");
    let (to_window, requests) = std::sync::mpsc::channel();
    harness.state_mut().attach_control(requests);
    let token = "f".repeat(64);
    let port = dm_desktop::control::listen(0, token.clone(), to_window, || {}).unwrap();

    let (ask, asked) = std::sync::mpsc::channel::<()>();
    let (answer, answers) = std::sync::mpsc::channel::<Result<serde_json::Value, String>>();
    let client = std::thread::spawn(move || {
        let mut mcp = dm_desktop::mcp::Control::connect(port, &token).unwrap();
        // A client that is not MCP: the command line's kind.
        let mut plain = dm_desktop::mcp::Control::connect_as(port, &token, None).unwrap();
        while asked.recv().is_ok() {
            let via_mcp = mcp.call("queue.list", serde_json::json!({}));
            let via_plain = plain.call("queue.list", serde_json::json!({}));
            answer.send(via_mcp).unwrap();
            answer.send(via_plain).unwrap();
        }
    });
    let round = |harness: &mut Harness<'_, DmApp>| {
        ask.send(()).unwrap();
        let mut got = Vec::new();
        while got.len() < 2 {
            harness.step();
            while let Ok(one) = answers.try_recv() {
                got.push(one);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        got
    };
    let on = round(&mut harness);
    assert!(on[0].is_ok(), "refused while on, the default: {on:?}");
    assert!(on[1].is_ok(), "{on:?}");
    allow_mcp(&mut harness, false);
    let off = round(&mut harness);
    assert!(
        matches!(&off[0], Err(why) if why.contains("turned off")),
        "turning it off did not take hold on the open connection: {off:?}"
    );
    assert!(
        off[1].is_ok(),
        "the command line's kind of client is not an AI assistant's: {off:?}"
    );
    allow_mcp(&mut harness, true);
    let on_again = round(&mut harness);
    assert!(
        on_again[0].is_ok(),
        "refused after turning it back on: {on_again:?}"
    );
    drop(ask);
    client.join().unwrap();
}

/// A port opened only because AI assistants are allowed answers assistants
/// and nothing else: the command line is not handed over by that default.
#[test]
fn a_port_opened_for_assistants_alone_refuses_other_clients() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().restrict_control_to_assistants(true);
    let (to_window, requests) = std::sync::mpsc::channel();
    harness.state_mut().attach_control(requests);
    let token = "9".repeat(64);
    let port = dm_desktop::control::listen(0, token.clone(), to_window, || {}).unwrap();
    let client = std::thread::spawn(move || {
        let mut mcp = dm_desktop::mcp::Control::connect(port, &token).unwrap();
        let mut plain = dm_desktop::mcp::Control::connect_as(port, &token, None).unwrap();
        (
            mcp.call("queue.list", serde_json::json!({})),
            plain.call("queue.list", serde_json::json!({})),
        )
    });
    while !client.is_finished() {
        harness.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    let (assistant, other) = client.join().unwrap();
    assert!(assistant.is_ok(), "{assistant:?}");
    assert!(
        matches!(&other, Err(why) if why.contains("AI assistants only")),
        "{other:?}"
    );
}

/// A pause followed at once by a resume, before the window has read the list
/// again: the resume is answered from the queue, not from the window's copy,
/// which still says the download runs. It was refused as "not paused", and the
/// download stayed stopped.
#[test]
fn a_resume_straight_after_a_pause_is_not_refused_by_a_stale_list() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("stalled.bin"), "Start now");
    until(&mut harness, "stalled.bin to run", |app| {
        app.rows().iter().any(|r| r.state == State::Running)
    });
    let id = row(&harness, "stalled.bin").id;
    let ids = serde_json::json!({ "ids": [id] });
    let said = harness
        .state_mut()
        .control_call("downloads.pause", &ids)
        .unwrap();
    assert_eq!(said["done"], serde_json::json!([id]));
    // No frame between: the window's list is as it was.
    let said = harness
        .state_mut()
        .control_call("downloads.resume", &ids)
        .unwrap();
    assert_eq!(
        said["done"],
        serde_json::json!([id]),
        "the resume was refused: {said}"
    );
    until(&mut harness, "it to run again", |app| {
        app.rows()
            .iter()
            .any(|r| r.id == id && r.state == State::Running)
    });
}
