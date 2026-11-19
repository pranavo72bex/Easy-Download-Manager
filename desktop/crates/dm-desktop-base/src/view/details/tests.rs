// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

fn failed(url: &str, reason: &str, cookies: &str, referrer: &str) -> Entry {
    serde_json::from_value(serde_json::json!({
        "id": 7,
        "url": url,
        "state": { "kind": "failed", "reason": reason },
        "progress": { "downloaded": 0, "total": null, "speed": 0.0, "eta": null },
        "filename": "a.bin",
        "cookies": cookies,
        "referrer": referrer,
    }))
    .expect("an entry")
}

fn setting() -> Setting<'static> {
    Setting {
        version: "0.8.6",
        os: "windows",
        when: "2026-11-01 09:30 UTC+05:45",
        connections: 8,
        free: Some(5 * 1024 * 1024 * 1024),
    }
}

#[test]
fn a_row_carrying_a_login_and_a_token_gives_away_neither() {
    // Built here, not written out, so no tracked file holds a login in an address.
    let login = ["user", "hunter2"].join(":");
    let address =
        format!("http://{login}@files.example.org:8080/private/dir/a.bin?token=abc123&sig=zzz9");
    let row = failed(
        &address,
        &format!("the server answered 403 Forbidden for {address}"),
        "session=COOKIEVALUE; other=COOKIEVALUE2",
        "https://page.example.net/watch?v=1&auth=REFERERSECRET",
    );
    let text = block(&row, &setting());
    for secret in [
        "hunter2",
        "user:",
        "abc123",
        "zzz9",
        "COOKIEVALUE",
        "REFERERSECRET",
        "/private",
        "dir/a.bin",
        "token",
        "?",
    ] {
        assert!(!text.contains(secret), "{secret:?} is in:\n{text}");
    }
    assert!(text.contains("Host: files.example.org"), "{text}");
    assert!(text.contains("Status: 403"), "{text}");
    assert!(text.contains("Cookie (value not shown)"), "{text}");
    assert!(text.contains("Authorization (value not shown)"), "{text}");
    assert!(text.contains("Referer"), "{text}");
}

#[test]
fn the_block_says_the_version_the_system_the_state_and_the_disk() {
    let row = failed(
        "https://example.com/f.bin",
        "the connection was refused",
        "",
        "",
    );
    let text = block(&row, &setting());
    assert!(
        text.starts_with("dm 0.8.6 on windows\n2026-11-01 09:30 UTC+05:45\nState: failed\n"),
        "{text}"
    );
    assert!(
        text.contains("Reason: the connection was refused"),
        "{text}"
    );
    assert!(text.contains("Status: none"), "{text}");
    assert!(text.contains("Connections: 8"), "{text}");
    assert!(text.contains("Free disk space: 5"), "{text}");
    assert!(!text.contains("Cookie"), "{text}");
    assert!(!text.contains("Authorization"), "{text}");
}

#[test]
fn a_status_is_read_from_the_words_and_not_from_an_address() {
    assert_eq!(status_code("the server answered 404 Not Found"), Some(404));
    assert_eq!(
        status_code("Details: HTTP/1.1 503 Service Unavailable"),
        Some(503)
    );
    assert_eq!(status_code("could not reach 127.0.0.1:443"), None);
    assert_eq!(status_code("the connection was refused"), None);
    assert_eq!(status_code("the server answered 999"), None);
}

#[test]
fn a_folder_in_a_reason_is_not_given_away() {
    let said = scrubbed(
        "the folder C:\\Users\\Bob Smith\\Downloads cannot be written to, or /home/bob/dl or ~/x",
    );
    for part in ["Bob", "Smith", "Users", "/home", "~/x"] {
        assert!(!said.contains(part), "{part} is in: {said}");
    }
    assert!(said.starts_with("the folder"), "{said}");
    assert_eq!(scrubbed("and/or half 1/2"), "and/or half 1/2");
}

#[test]
fn an_address_in_a_reason_is_cut_to_its_host() {
    let login = ["u", "p"].join(":");
    assert_eq!(
        scrubbed(&format!(
            "no good (http://{login}@h.example/a/b?c=d), sorry"
        )),
        "no good h.example sorry"
    );
    assert_eq!(scrubbed("see ://broken"), "see (address hidden)");
    assert_eq!(scrubbed("plain words only"), "plain words only");
}

#[test]
fn each_state_has_its_word_and_an_unread_disk_is_said_unknown() {
    let mut row = failed("https://example.com/f.bin", "x", "", "");
    let unread = Setting {
        free: None,
        ..setting()
    };
    for (state, word) in [
        (State::Queued, "waiting"),
        (State::Paused, "stopped"),
        (State::Completed, "finished"),
        (State::Cancelled, "cancelled"),
    ] {
        row.state = state;
        let text = block(&row, &unread);
        assert!(text.contains(&format!("State: {word}\n")), "{text}");
        assert!(text.ends_with("Free disk space: unknown"), "{text}");
    }
}

#[test]
fn a_strong_validator_sends_if_range_and_a_weak_one_does_not() {
    let mut row = failed("https://example.com/f.bin", "x", "", "");
    row.validator = Some("\"abc\"".into());
    assert!(headers_sent(&row).contains(&"If-Range"));
    row.validator = Some("W/\"abc\"".into());
    assert!(!headers_sent(&row).contains(&"If-Range"));
    row.validator = None;
    assert!(!headers_sent(&row).contains(&"If-Range"));
}

#[test]
fn only_failed_rows_have_a_block() {
    let dir = std::env::temp_dir();
    let mut done = failed("https://example.com/f.bin", "x", "", "");
    done.state = State::Completed;
    assert!(blocks_for(std::slice::from_ref(&done), 4, &dir).is_none());
    let broken = failed(
        "https://example.com/g.bin",
        "the connection was refused",
        "",
        "",
    );
    let (text, count) = blocks_for(&[done, broken.clone(), broken], 4, &dir).unwrap();
    assert_eq!(count, 2);
    assert_eq!(text.matches("State: failed").count(), 2, "{text}");
    assert!(text.contains("Connections: 4"), "{text}");
}

#[test]
fn a_finished_or_running_row_has_no_reason_line() {
    let mut row = failed("https://example.com/f.bin", "x", "", "");
    row.state = State::Running;
    let text = block(&row, &setting());
    assert!(text.contains("State: running"), "{text}");
    assert!(!text.contains("Reason:"), "{text}");
}
