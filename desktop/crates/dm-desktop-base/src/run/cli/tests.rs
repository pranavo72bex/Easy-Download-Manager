// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

fn words(s: &str) -> Vec<String> {
    s.split_whitespace().map(String::from).collect()
}

#[test]
fn commands_and_their_numbers_are_read_and_misuse_is_said() {
    let ok = |s: &str| parse(&words(s)).unwrap().0;
    assert_eq!(ok("list"), Ask::List);
    assert_eq!(ok("status 7"), Ask::Status(7));
    assert_eq!(ok("pause 1 2"), Ask::Act("downloads.pause", vec![1, 2]));
    assert_eq!(ok("--json cancel 3"), Ask::Act("downloads.cancel", vec![3]));
    assert!(parse(&words("--json list")).unwrap().1);
    for bad in [
        "",
        "pause",
        "status",
        "status 1 2",
        "list 4",
        "pause x",
        "fly 1",
        "list --what",
    ] {
        assert!(parse(&words(bad)).is_err(), "{bad:?}");
    }
}

#[test]
fn a_row_shows_no_cookie_referrer_or_note_even_if_dm_sent_them() {
    let row = json!({
        "id": 4, "url": "https://h/f.bin", "filename": "f.bin", "path": "/d/f.bin",
        "state": { "kind": "failed", "reason": "connection reset" },
        "progress": { "downloaded": 1536, "total": 4096, "speed": 0.0 },
        "cookies": "session=SECRET", "referrer": "https://secret/page",
        "description": "private note", "authorization": "Bearer SECRET"
    });
    let shown = brief(&row).to_string();
    for secret in ["SECRET", "secret", "private"] {
        assert!(!shown.contains(secret), "{shown}");
    }
    assert!(line(&brief(&row)).contains("failed (connection reset)"));
    assert!(line(&brief(&row)).contains("1.5 KiB"));
    assert!(
        line(&brief(&row)).contains(" 37%"),
        "{}",
        line(&brief(&row))
    );
    let running = json!({
        "id": 5, "url": "u", "filename": "g.bin", "state": { "kind": "running" },
        "progress": { "downloaded": 1 << 20, "total": 4 << 20, "speed": 2048.0 }
    });
    assert!(
        line(&brief(&running)).contains("25%  "),
        "{}",
        line(&brief(&running))
    );
    assert!(line(&brief(&running)).contains("2.0 KiB/s"));
    assert_eq!(
        parse(&words("pause 3 3 4 3")).unwrap().0,
        Ask::Act("downloads.pause", vec![3, 4])
    );
}

/// A dm that answers `queue.list` with one row and records the rest.
fn fake_dm() -> (u16, std::sync::mpsc::Receiver<String>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (send, got) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut out = stream.try_clone().unwrap();
        let mut reader = BufReader::new(stream);
        loop {
            let mut l = String::new();
            if reader.read_line(&mut l).unwrap_or(0) == 0 {
                return;
            }
            let m: Value = serde_json::from_str(&l).unwrap();
            let method = m["method"].as_str().unwrap().to_string();
            let _ = send.send(l.trim().to_string());
            let result = match method.as_str() {
                "queue.list" => json!([{ "id": 1, "url": "https://h/a", "filename": "a",
                    "state": { "kind": "running" },
                    "progress": { "downloaded": 10, "total": 20, "speed": 5.0 },
                    "path": "" }]),
                "downloads.pause" => {
                    json!({ "done": [1], "skipped": [{ "id": 9, "why": "no such download" }] })
                }
                _ => json!({}),
            };
            writeln!(out, "{}", json!({ "id": 1, "ok": true, "result": result })).unwrap();
        }
    });
    (port, got)
}

#[test]
fn list_status_and_pause_go_through_the_control_protocol_with_exit_codes() {
    let (port, got) = fake_dm();
    let mut control = Control::connect(port, "t").unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(
        run_with(&Ask::List, true, &mut control, &mut out, &mut err),
        0
    );
    let listed: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(listed[0]["state"], "running");
    out.clear();
    assert_eq!(
        run_with(&Ask::Status(2), false, &mut control, &mut out, &mut err),
        1
    );
    assert!(String::from_utf8_lossy(&err).contains("no download is numbered 2"));
    err.clear();
    let pause = Ask::Act("downloads.pause", vec![1, 9]);
    assert_eq!(run_with(&pause, false, &mut control, &mut out, &mut err), 1);
    assert!(String::from_utf8_lossy(&err).contains("9 not done: no such download"));
    let sent: Vec<String> = got.try_iter().collect();
    assert!(sent.iter().any(|l| l.contains("\"auth\"")));
    assert!(sent
        .iter()
        .any(|l| l.contains("downloads.pause") && l.contains("[1,9]")));
}
