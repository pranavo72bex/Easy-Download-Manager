// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// A stand-in for EDM's control protocol that answers every line, and
/// closes after `lines` of them as an EDM quitting does.
fn fake_dm(lines: usize) -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut out = stream.try_clone().unwrap();
        let mut reader = BufReader::new(stream);
        for _ in 0..lines {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            writeln!(out, "{}", json!({ "id": 1, "ok": true, "result": port })).unwrap();
        }
    });
    port
}

#[test]
fn the_human_tools_are_control_calls() {
    assert_eq!(method_of("edm_click"), Some("ui.click"));
    assert_eq!(method_of("edm_screen"), Some("ui.texts"));
    assert_eq!(method_of("edm_type"), Some("ui.type"));
    assert_eq!(method_of("edm_press_key"), Some("ui.key"));
}

#[test]
fn tools_become_the_control_calls_they_are() {
    let one = |tool: &str, args: Value| plan(tool, &args).unwrap().unwrap();
    let run = |id: &str| ("command.run", json!({ "id": id }));
    assert_eq!(
        one("edm_pause", json!({ "ids": [3, 4] })),
        vec![("downloads.pause", json!({ "ids": [3, 4] }))]
    );
    assert_eq!(
        one("edm_resume", json!({})),
        vec![run("downloads.resume_all")]
    );
    assert_eq!(
        one("edm_cancel", json!({ "ids": [5] })),
        vec![("downloads.cancel", json!({ "ids": [5] }))]
    );
    assert_eq!(
        one("edm_set_speed_limit", json!({ "preset": "low" })),
        vec![run("downloads.speed.low")]
    );
    assert_eq!(
        one("edm_set_speed_limit", json!({ "kb": 500 })),
        vec![("downloads.speed_limit", json!({ "kb": 500 }))]
    );
    assert_eq!(
        one("edm_move", json!({ "ids": [2], "direction": "down" })),
        vec![("ui.select", json!({ "ids": [2] })), run("file.move_down")]
    );
    assert_eq!(
        one("edm_queue", json!({ "action": "stop" })),
        vec![run("downloads.stop_queue")]
    );
    assert!(plan("edm_set_speed_limit", &json!({ "preset": "x" })).is_err());
    assert!(plan("edm_move", &json!({ "ids": [1] })).is_err());
    // Without ids, cancel and move refuse rather than act on a stray
    // selection or none.
    assert!(plan("edm_cancel", &json!({})).is_err());
    assert!(plan("edm_cancel", &json!({ "ids": [] })).is_err());
    assert!(plan("edm_move", &json!({ "direction": "up" })).is_err());
    // A null `kb` is no `kb`: the preset is used.
    assert_eq!(
        one(
            "edm_set_speed_limit",
            json!({ "kb": null, "preset": "low" })
        ),
        vec![run("downloads.speed.low")]
    );
    assert!(plan("edm_queue", &json!({ "action": "x" })).is_err());
    assert_eq!(plan("edm_nonsense", &json!({})), Ok(None));
}

#[test]
fn a_edm_that_restarts_is_connected_to_again() {
    /* Connected once: after EDM restarted, on a new port, every call
     * failed until the AI client restarted `EDM mcp` as well. */
    let first = fake_dm(2);
    let at = Arc::new(Mutex::new(first));
    let found = Arc::clone(&at);
    let mut control =
        Control::with(Box::new(move || Ok((*found.lock().unwrap(), "t".into())))).unwrap();
    assert_eq!(control.call("queue.list", json!({})), Ok(json!(first)));

    // EDM quits, and starts again elsewhere.
    let second = fake_dm(3);
    *at.lock().unwrap() = second;
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert_eq!(control.call("queue.list", json!({})), Ok(json!(second)));
    assert_eq!(control.call("queue.list", json!({})), Ok(json!(second)));
}
