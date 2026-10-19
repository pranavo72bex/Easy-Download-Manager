//! The shape the window actually reads.
//!
//! The UI is plain JavaScript reaching into these structures by name —
//! `state.kind`, `state.reason`, `progress.eta.secs`. Nothing checks those
//! spellings at compile time, and getting one wrong renders an empty window
//! with no error anywhere. So the wire format is pinned here, where changing
//! it fails a test instead of quietly emptying the screen.

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
        replaces: None,
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
    // The UI switches on `state.kind`; a bare string would make every
    // comparison fail silently and every row render as "undefined".
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
    // The UI compares against lower-case literals.
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
    // serde renders a Duration as { secs, nanos }; the UI reads `.secs`.
    let json = serde_json::to_value(entry(State::Running)).unwrap();
    assert_eq!(json["progress"]["eta"]["secs"], 90);
}

#[test]
fn an_unknown_total_or_eta_is_null_rather_than_zero() {
    /* The difference matters: the UI hides a progress bar when the total is
     * unknown, and zero would draw an empty bar claiming to know. */
    let mut row = entry(State::Running);
    row.progress.total = None;
    row.progress.eta = None;

    let json = serde_json::to_value(row).unwrap();
    assert!(json["progress"]["total"].is_null());
    assert!(json["progress"]["eta"].is_null());
}
