// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The AI client's tools (`mcp.rs`): the MCP protocol they answer, the
//! control calls each tool stands for, and the connection to dm, tried
//! against a stand-in for dm's control port on loopback that says exactly
//! what it was asked. Moved from the egui window's tests.

use dm_desktop_base::mcp::{self, Control};
use serde_json::{json, Value};
use std::io::Write;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A stand-in for dm's control port: takes connections one after another,
/// answers each line as `reply` says, and records what it was asked.
struct FakeDm {
    port: u16,
    asked: Arc<Mutex<Vec<(String, Value)>>>,
    connections: Arc<std::sync::atomic::AtomicUsize>,
}

/// What the stand-in does with one request.
enum Reply {
    /// Answers with this line.
    Line(String),
    /// Answers with two lines at once.
    Two(String, String),
    /// Answers, and a moment later sends another line unasked.
    Late(String, String),
    /// Reads it and says nothing, the connection kept.
    Silent,
    /// Closes the connection without answering.
    Hang,
    /// Closes it without reading the request, which resets it.
    Reset,
}

fn fake_dm(reply: impl Fn(&str, &Value) -> Reply + Send + Sync + 'static) -> FakeDm {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let connections = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (log, count) = (Arc::clone(&asked), Arc::clone(&connections));
    let reply = Arc::new(reply);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let (log, reply) = (Arc::clone(&log), Arc::clone(&reply));
            std::thread::spawn(move || {
                let mut out = stream.try_clone().unwrap();
                // Unbuffered: a line read is that request and no more.
                let mut reader = stream;
                loop {
                    // The request seen whole before it is read, so a reset
                    // can leave it unread.
                    let mut peeked = vec![0u8; 64 * 1024];
                    let (first, length) = loop {
                        let seen = reader.peek(&mut peeked).unwrap_or(0);
                        if seen == 0 {
                            return;
                        }
                        if let Some(end) = peeked[..seen].iter().position(|b| *b == b'\n') {
                            let first =
                                serde_json::from_slice::<Value>(&peeked[..end]).unwrap_or_default();
                            break (first, end + 1);
                        }
                        std::thread::sleep(Duration::from_millis(2));
                    };
                    let method = first["method"].as_str().unwrap_or("").to_string();
                    let params = first["params"].clone();
                    let what = reply(&method, &params);
                    if matches!(what, Reply::Reset) {
                        return;
                    }
                    let mut line = vec![0u8; length];
                    if std::io::Read::read_exact(&mut reader, &mut line).is_err() {
                        return;
                    }
                    log.lock().unwrap().push((method, params));
                    match what {
                        Reply::Line(one) => {
                            let _ = writeln!(out, "{one}");
                        }
                        Reply::Two(one, two) => {
                            let _ = write!(out, "{one}\n{two}\n");
                        }
                        Reply::Late(one, two) => {
                            let _ = writeln!(out, "{one}");
                            std::thread::sleep(Duration::from_millis(100));
                            let _ = writeln!(out, "{two}");
                        }
                        Reply::Silent => {}
                        Reply::Hang | Reply::Reset => return,
                    }
                }
            });
        }
    });
    FakeDm {
        port,
        asked,
        connections,
    }
}

fn ok(result: Value) -> Reply {
    Reply::Line(json!({ "id": 1, "ok": true, "result": result }).to_string())
}

fn refused(why: &str) -> Reply {
    Reply::Line(json!({ "id": 1, "ok": false, "error": why }).to_string())
}

/// A stand-in that lets everyone in and echoes the method it was asked.
fn echoing_dm() -> FakeDm {
    fake_dm(|method, params| match method {
        "auth" => ok(json!({ "client": params["client"] })),
        "ui.select" if params["ids"] == json!([666]) => refused("no such rows"),
        _ => ok(json!({ "method": method })),
    })
}

fn token() -> String {
    "t".repeat(64)
}

impl FakeDm {
    fn asked(&self) -> Vec<(String, Value)> {
        self.asked.lock().unwrap().clone()
    }

    fn methods(&self) -> Vec<String> {
        self.asked().into_iter().map(|(m, _)| m).collect()
    }

    fn connections(&self) -> usize {
        self.connections.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn clear(&self) {
        self.asked.lock().unwrap().clear();
    }
}

/// The MCP replies to these lines, in order.
fn serve(control: &mut Control, lines: &[Value]) -> Vec<Value> {
    let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let mut out = Vec::new();
    mcp::serve(std::io::Cursor::new(input), &mut out, control).unwrap();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn tool(id: u64, name: &str, arguments: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": name, "arguments": arguments } })
}

#[test]
fn the_mcp_server_answers_its_protocol_and_lists_every_tool() {
    let dm = echoing_dm();
    let mut control = Control::connect(dm.port, &token()).unwrap();
    assert_eq!(dm.asked()[0].1["client"], "mcp", "said to be an assistant");
    let input = format!(
        "{}\n\n   \n{}\nnot json\n{}\n{}\n{}\n",
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        json!({ "jsonrpc": "2.0", "id": "two", "method": "ping" }),
        json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }),
        json!({ "jsonrpc": "2.0", "id": 4, "method": "resources/list" }),
    );
    let mut out = Vec::new();
    mcp::serve(std::io::Cursor::new(input), &mut out, &mut control).unwrap();
    let replies: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(replies.len(), 5, "{replies:?}");
    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "dm");
    assert!(replies[0]["result"]["capabilities"]["tools"].is_object());
    assert_eq!(replies[1]["error"]["code"], -32700);
    assert_eq!(replies[1]["id"], Value::Null);
    assert_eq!(
        replies[2],
        json!({ "jsonrpc": "2.0", "id": "two", "result": {} })
    );
    let tools = replies[3]["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for name in [
        "dm_list_commands",
        "dm_run_command",
        "dm_add_downloads",
        "dm_list_downloads",
        "dm_select",
        "dm_pause",
        "dm_resume",
        "dm_cancel",
        "dm_set_speed_limit",
        "dm_move",
        "dm_queue",
        "dm_screen",
        "dm_click",
        "dm_type",
        "dm_press_key",
        "dm_inspect",
        "dm_screenshot",
    ] {
        assert!(names.contains(&name), "{name} missing: {names:?}");
    }
    assert_eq!(names.len(), 17);
    for tool in tools {
        assert!(tool["description"].as_str().is_some_and(|d| !d.is_empty()));
        assert_eq!(tool["inputSchema"]["type"], "object");
    }
    assert_eq!(replies[4]["error"]["code"], -32601);
    assert!(replies[4]["error"]["message"]
        .as_str()
        .unwrap()
        .contains("resources/list"));
    // The client's own protocol version is said back.
    let said = serve(
        &mut control,
        &[json!({ "jsonrpc": "2.0", "id": 9, "method": "initialize",
                  "params": { "protocolVersion": "2024-11-05" } })],
    );
    assert_eq!(said[0]["result"]["protocolVersion"], "2024-11-05");
    // Only the auth reached dm.
    assert_eq!(dm.methods(), ["auth"]);
}

#[test]
fn each_tool_is_the_control_calls_it_stands_for() {
    let dm = echoing_dm();
    let mut control = Control::connect(dm.port, &token()).unwrap();
    // A tool, its arguments, and the control calls it is.
    type Case = (&'static str, Value, Vec<(&'static str, Value)>);
    let cases: Vec<Case> = vec![
        (
            "dm_list_commands",
            json!({}),
            vec![("commands.list", json!({}))],
        ),
        (
            "dm_run_command",
            json!({ "id": "downloads.start_queue" }),
            vec![("command.run", json!({ "id": "downloads.start_queue" }))],
        ),
        (
            "dm_add_downloads",
            json!({ "urls": ["http://x.invalid/a"], "later": true }),
            vec![(
                "downloads.add",
                json!({ "urls": ["http://x.invalid/a"], "later": true }),
            )],
        ),
        (
            "dm_list_downloads",
            json!({}),
            vec![("queue.list", json!({}))],
        ),
        (
            "dm_select",
            json!({ "ids": [1] }),
            vec![("ui.select", json!({ "ids": [1] }))],
        ),
        ("dm_inspect", json!({}), vec![("ui.inspect", json!({}))]),
        (
            "dm_screenshot",
            json!({ "name": "n" }),
            vec![("ui.screenshot", json!({ "name": "n" }))],
        ),
        ("dm_screen", json!({}), vec![("ui.texts", json!({}))]),
        (
            "dm_type",
            json!({ "text": "x" }),
            vec![("ui.type", json!({ "text": "x" }))],
        ),
        (
            "dm_press_key",
            json!({ "key": "Escape" }),
            vec![("ui.key", json!({ "key": "Escape" }))],
        ),
        (
            "dm_pause",
            json!({ "ids": null }),
            vec![("command.run", json!({ "id": "downloads.stop_all" }))],
        ),
        (
            "dm_resume",
            json!({ "ids": [2] }),
            vec![("downloads.resume", json!({ "ids": [2] }))],
        ),
        (
            "dm_cancel",
            json!({ "ids": [3] }),
            vec![("downloads.cancel", json!({ "ids": [3] }))],
        ),
        (
            "dm_move",
            json!({ "ids": [4], "direction": "up" }),
            vec![
                ("ui.select", json!({ "ids": [4] })),
                ("command.run", json!({ "id": "file.move_up" })),
            ],
        ),
        (
            "dm_queue",
            json!({ "action": "start" }),
            vec![("command.run", json!({ "id": "downloads.start_queue" }))],
        ),
        (
            "dm_queue",
            json!({ "action": "stop" }),
            vec![("command.run", json!({ "id": "downloads.stop_queue" }))],
        ),
        (
            "dm_move",
            json!({ "ids": [5, 6], "direction": "down" }),
            vec![
                ("ui.select", json!({ "ids": [5, 6] })),
                ("command.run", json!({ "id": "file.move_down" })),
            ],
        ),
        (
            "dm_pause",
            json!({ "ids": [7] }),
            vec![("downloads.pause", json!({ "ids": [7] }))],
        ),
        (
            "dm_resume",
            json!({}),
            vec![("command.run", json!({ "id": "downloads.resume_all" }))],
        ),
        (
            "dm_set_speed_limit",
            json!({ "preset": "unlimited" }),
            vec![("command.run", json!({ "id": "downloads.speed.unlimited" }))],
        ),
        (
            "dm_set_speed_limit",
            json!({ "kb": 0, "preset": "low" }),
            vec![("downloads.speed_limit", json!({ "kb": 0 }))],
        ),
    ];
    for (id, (name, arguments, wanted)) in cases.into_iter().enumerate() {
        dm.clear();
        let said = serve(&mut control, &[tool(id as u64, name, arguments.clone())]);
        let result = &said[0]["result"];
        assert_eq!(said[0]["id"], id as u64);
        assert_eq!(result["isError"], Value::Null, "{name}: {result}");
        let text: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(result["content"][0]["type"], "text");
        // The last call's answer is the tool's.
        assert_eq!(text["method"], wanted.last().unwrap().0, "{name}");
        let asked: Vec<(String, Value)> = wanted
            .into_iter()
            .map(|(m, p)| (m.to_string(), p))
            .collect();
        assert_eq!(dm.asked(), asked, "{name} {arguments}");
    }
    // No arguments at all is none given.
    dm.clear();
    let said = serve(
        &mut control,
        &[json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                  "params": { "name": "dm_list_downloads" } })],
    );
    assert_eq!(said[0]["result"]["isError"], Value::Null);
    assert_eq!(dm.asked(), [("queue.list".to_string(), json!({}))]);
}

#[test]
fn a_tool_that_fails_is_a_result_the_model_reads_and_stops_where_it_failed() {
    let dm = echoing_dm();
    let mut control = Control::connect(dm.port, &token()).unwrap();
    dm.clear();
    let said = serve(
        &mut control,
        &[
            tool(1, "dm_move", json!({ "ids": [666], "direction": "down" })),
            tool(2, "dm_cancel", json!({})),
            tool(3, "dm_move", json!({ "ids": [1], "direction": "sideways" })),
            tool(4, "dm_queue", json!({})),
            tool(5, "dm_set_speed_limit", json!({})),
            tool(6, "dm_teleport", json!({})),
            json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call" }),
            tool(8, "dm_move", json!({ "ids": [], "direction": "up" })),
            tool(9, "dm_cancel", json!({ "ids": null })),
        ],
    );
    let text = |n: usize| said[n]["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(said[0]["result"]["isError"], true);
    assert_eq!(text(0), "no such rows");
    // The select failed: the move was not run on whatever was selected.
    assert_eq!(dm.methods(), ["ui.select"]);
    assert_eq!(text(1), "give `ids`: the downloads to cancel");
    assert_eq!(text(2), "direction is up or down");
    assert_eq!(text(3), "action is start or stop");
    assert!(text(4).contains("preset is low, medium, high or unlimited"));
    for one in &said[1..5] {
        assert_eq!(one["result"]["isError"], true, "{one}");
    }
    assert_eq!(said[5]["error"]["code"], -32602);
    assert_eq!(said[5]["error"]["message"], "no tool is called dm_teleport");
    assert_eq!(said[6]["error"]["message"], "no tool is called ");
    assert_eq!(text(7), "give `ids`: the downloads to move");
    assert_eq!(text(8), "give `ids`: the downloads to cancel");
    assert_eq!(dm.methods(), ["ui.select"], "nothing more reached dm");
}

#[test]
fn a_connection_is_refused_with_why_when_dm_is_not_there_or_says_no() {
    // A port nothing listens on.
    let closed = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let why = Control::connect(closed, &token()).err().unwrap();
    assert!(why.contains("dm is not running"), "{why}");
    assert!(why.contains(&closed.to_string()), "{why}");

    let wrong = fake_dm(|_, _| refused("wrong token"));
    let why = Control::connect_as(wrong.port, "nope", None).err().unwrap();
    assert_eq!(why, "wrong token");
    let silent = fake_dm(|_, _| Reply::Line(json!({ "ok": false }).to_string()));
    assert_eq!(
        Control::connect(silent.port, "x").err().unwrap(),
        "dm refused"
    );

    let lost = Control::with(Box::new(|| Err("nowhere to look".into())));
    assert_eq!(lost.err().unwrap(), "nowhere to look");

    // `dm mcp` with nothing to talk to ends at once, saying so.
    let home = tempfile::tempdir().unwrap();
    assert_eq!(mcp::main(&[], home.path()), 2);
    let tokens = home.path().join("token");
    std::fs::write(&tokens, token()).unwrap();
    let args: Vec<String> = [
        "mcp",
        "--port",
        &closed.to_string(),
        "--token-file",
        tokens.to_str().unwrap(),
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(mcp::main(&args, home.path()), 2);
}

#[test]
fn the_port_and_token_are_found_in_the_options_or_in_dms_home() {
    let home = tempfile::tempdir().unwrap();
    let nothing = mcp::finder(home.path(), None, None);
    assert!(nothing().unwrap_err().contains("no running dm"));
    std::fs::write(home.path().join("control.port"), " 4321\n").unwrap();
    assert!(nothing().is_err(), "a port without a token");
    std::fs::write(home.path().join("control.token"), "  sesame \n").unwrap();
    assert_eq!(nothing().unwrap(), (4321, "sesame".to_string()));
    // The options before the files, read again each time.
    let other = home.path().join("other.token");
    std::fs::write(&other, "other").unwrap();
    let given = mcp::finder(
        home.path(),
        Some("1234".into()),
        Some(other.display().to_string()),
    );
    assert_eq!(given().unwrap(), (1234, "other".to_string()));
    std::fs::write(&other, "changed").unwrap();
    assert_eq!(given().unwrap().1, "changed");
    let garbled = mcp::finder(home.path(), Some("port".into()), None);
    assert!(garbled().is_err());
    std::fs::write(home.path().join("control.port"), "99999").unwrap();
    assert!(nothing().is_err(), "no such port");

    // Found that way, and connected as an assistant.
    let dm = echoing_dm();
    std::fs::write(home.path().join("control.port"), dm.port.to_string()).unwrap();
    let mut control = Control::as_assistant(mcp::finder(home.path(), None, None)).unwrap();
    assert_eq!(
        dm.asked()[0].1,
        json!({ "token": "sesame", "client": "mcp" })
    );
    assert_eq!(
        control.call("queue.list", json!({})).unwrap()["method"],
        "queue.list"
    );
}

#[test]
fn a_connection_lost_mid_call_is_made_again_for_the_next() {
    let dm = fake_dm(|method, params| match (method, params["n"].as_u64()) {
        ("auth", _) => ok(json!("in")),
        (_, Some(1)) => Reply::Hang,
        (_, Some(2)) => Reply::Reset,
        (_, Some(3)) => Reply::Line("not json".into()),
        (_, Some(4)) => Reply::Two(
            json!({ "ok": true, "result": "four" }).to_string(),
            json!({ "ok": true, "result": "early" }).to_string(),
        ),
        (_, Some(5)) => Reply::Line(json!({ "ok": true }).to_string()),
        (_, Some(6)) => refused("no"),
        (_, Some(7)) => Reply::Late(
            json!({ "ok": true, "result": "seven" }).to_string(),
            json!({ "ok": true, "result": "unasked" }).to_string(),
        ),
        // Answered already, by the second line of the answer before.
        ("w", _) => Reply::Silent,
        _ => ok(json!(method)),
    });
    let mut control = Control::with(Box::new({
        let port = dm.port;
        move || Ok((port, token()))
    }))
    .unwrap();
    assert_eq!(dm.asked()[0].1["client"], Value::Null, "not an assistant");
    assert_eq!(control.call("x", json!({})).unwrap(), json!("x"));

    // Closed without an answer: said, and the next call connects again.
    let lost = control.call("x", json!({ "n": 1 })).unwrap_err();
    assert_eq!(lost, "dm closed the connection");
    assert_eq!(control.call("y", json!({})).unwrap(), json!("y"));
    assert_eq!(dm.connections(), 2);

    // Reset with the request unread.
    assert!(control.call("x", json!({ "n": 2 })).is_err());
    assert_eq!(control.call("z", json!({})).unwrap(), json!("z"));
    assert_eq!(dm.connections(), 3);

    // An answer that is not JSON.
    assert_eq!(
        control.call("x", json!({ "n": 3 })).unwrap_err(),
        "dm closed the connection"
    );

    // Two answers to one request: the second, already read, is taken as
    // the next call's, on the same connection.
    let before = dm.connections();
    assert_eq!(control.call("x", json!({ "n": 4 })).unwrap(), json!("four"));
    assert_eq!(control.call("w", json!({})).unwrap(), json!("early"));
    assert_eq!(dm.connections(), before);

    // An answer with nothing in it, and a refusal that keeps the connection.
    assert_eq!(control.call("x", json!({ "n": 5 })).unwrap(), Value::Null);
    assert_eq!(control.call("x", json!({ "n": 6 })).unwrap_err(), "no");
    assert_eq!(control.call("v", json!({})).unwrap(), json!("v"));
    assert_eq!(dm.connections(), before);

    // A line sent unasked after an answer, still unread: the connection is
    // there, and the line is taken as the next answer.
    assert_eq!(
        control.call("x", json!({ "n": 7 })).unwrap(),
        json!("seven")
    );
    // Not waited for: the next call's answer is read once it is there.
    assert_eq!(control.call("w", json!({})).unwrap(), json!("unasked"));
    assert_eq!(dm.connections(), before);

    // A person's step waits for the frames that draw it.
    let at = Instant::now();
    assert!(control.call("ui.click", json!({ "text": "a" })).is_ok());
    assert!(at.elapsed() >= Duration::from_millis(300));
}
