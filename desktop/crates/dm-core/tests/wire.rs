//! The shape a row takes in `queue.json` and over the control protocol's
//! `queue.list`.
//!
//! The check scripts and anything driving dm from outside read these by name
//! — `state.kind`, `state.reason`, `progress.eta.secs`. Nothing checks those
//! spellings at compile time, so the format is pinned here, where changing it
//! fails a test instead of quietly breaking whatever reads it.

use std::time::Duration;

use dm_core::{Entry, Progress, State};

fn entry(state: State) -> Entry {
    Entry {
        id: 7,
        url: "https://example.com/f.bin".into(),
        state,
        progress: Progress {
            downloaded: 512,
            total: Some(2048),
            speed: 1024.5,
            eta: Some(Duration::from_secs(90)),
        },
        filename: "f.bin".into(),
        finished_at: None,
        category: None,
        checksum: None,
        verified: false,
        mirrors: 0,
        path: String::new(),
        referrer: String::new(),
        paused_by_schedule: false,
        description: String::new(),
        queue: String::new(),
        save_dir: None,
        save_name: None,
        validator: None,
        quality: None,
        parts_dir: String::new(),
        began_in: String::new(),
        cookies: String::new(),
        cookies_once: false,
        replaces: None,
        speed_limit: 0,
        stalled: String::new(),
        sound: None,
        span: None,
        shape: None,
        mp3: false,
        gif: false,
        delete_after: None,
    }
}

#[test]
fn a_row_carries_the_names_the_window_reads() {
    let json = serde_json::to_value(entry(State::Running)).unwrap();

    assert_eq!(json["id"], 7);
    assert_eq!(json["filename"], "f.bin");
    assert_eq!(json["url"], "https://example.com/f.bin");
    assert_eq!(json["progress"]["downloaded"], 512);
    assert_eq!(json["progress"]["total"], 2048);
}

#[test]
fn a_state_is_a_tagged_object_not_a_bare_string() {
    // Readers switch on `state.kind`; a bare string would make every such
    // comparison fail silently.
    let json = serde_json::to_value(entry(State::Running)).unwrap();
    assert_eq!(json["state"]["kind"], "running");
}

#[test]
fn a_failure_carries_its_reason_where_the_window_looks() {
    let json =
        serde_json::to_value(entry(State::Failed("the server answered 404".into()))).unwrap();

    assert_eq!(json["state"]["kind"], "failed");
    assert_eq!(json["state"]["reason"], "the server answered 404");
}

#[test]
fn every_state_spells_its_kind_in_lower_case() {
    // Readers compare against lower-case literals.
    for (state, expected) in [
        (State::Queued, "queued"),
        (State::Running, "running"),
        (State::Paused, "paused"),
        (State::Completed, "completed"),
        (State::Cancelled, "cancelled"),
    ] {
        let json = serde_json::to_value(entry(state)).unwrap();
        assert_eq!(json["state"]["kind"], expected);
    }
}

#[test]
fn an_eta_arrives_as_seconds_the_window_can_read() {
    // serde renders a Duration as { secs, nanos }; readers take `.secs`.
    let json = serde_json::to_value(entry(State::Running)).unwrap();
    assert_eq!(json["progress"]["eta"]["secs"], 90);
}

#[test]
fn an_unknown_total_or_eta_is_null_rather_than_zero() {
    /* The difference matters: zero would claim to know a total of nothing
     * where the total is not known at all. */
    let mut row = entry(State::Running);
    row.progress.total = None;
    row.progress.eta = None;

    let json = serde_json::to_value(row).unwrap();
    assert!(json["progress"]["total"].is_null());
    assert!(json["progress"]["eta"].is_null());
}
