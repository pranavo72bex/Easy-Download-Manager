// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

fn queued(id: Id) -> Arc<Entry> {
    Arc::new(
        serde_json::from_value(serde_json::json!({
            "id": id,
            "url": format!("https://e.com/{id}"),
            "state": serde_json::to_value(State::Queued).unwrap(),
            "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
            "filename": "f",
        }))
        .unwrap(),
    )
}

/// A schedule open all day ("00:00" to "00:00") asking for `when_done`.
fn open_all_day(when_done: &str) -> Settings {
    let mut settings = Settings::default();
    settings.schedule.enabled = true;
    settings.schedule.from = "00:00".into();
    settings.schedule.to = "00:00".into();
    settings.schedule.when_done = when_done.into();
    settings
}

/// The schedule's When done is armed as its window opens and put away
/// unused as it closes, unless the person chose another meanwhile.
#[test]
fn a_schedules_when_done_is_put_away_unused_unless_changed_since() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let state = tempfile::tempdir().unwrap();
    let backend = crate::app::backend::testing::backend(&runtime, state.path());
    let rows = [queued(1)];
    let mut open = open_all_day("sleep");
    let mut closed = open.clone();
    closed.schedule.enabled = false;

    let mut session = Session::default();
    let tick = session.schedule_tick(&backend, &open, &rows, |_| true);
    assert_eq!(tick.armed, Some(WhenDone::Sleep));
    assert_eq!(session.when_done, WhenDone::Sleep);
    assert_eq!(session.limit_in_force(), Some(open.speed_limit_bytes_now()));
    session.schedule_tick(&backend, &closed, &rows, |_| true);
    assert_eq!(session.when_done, WhenDone::Nothing, "put away unused");

    // Changed in the menu while the window was open: the person's stands.
    open.schedule.when_done = "shutdown".into();
    let mut session = Session::default();
    session.schedule_tick(&backend, &open, &rows, |_| true);
    assert_eq!(session.when_done, WhenDone::ShutDown);
    session.when_done = WhenDone::Sleep;
    session.schedule_tick(&backend, &closed, &rows, |_| true);
    assert_eq!(session.when_done, WhenDone::Sleep, "the person's choice");

    // One this computer cannot carry out is not armed.
    let mut session = Session::default();
    let tick = session.schedule_tick(&backend, &open, &rows, |_| false);
    assert_eq!(tick.armed, None);
    assert_eq!(session.when_done, WhenDone::Nothing);
}

/// The end of the window, once what was waiting has gone: its program
/// run (one that cannot start is said) and the hang-up it asks for.
#[test]
fn at_the_end_of_the_window_its_program_is_run_and_the_line_hung_up() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let state = tempfile::tempdir().unwrap();
    let backend = crate::app::backend::testing::backend(&runtime, state.path());
    let mut settings = open_all_day("nothing");
    let program = state.path().join("no such program");
    settings.schedule.run_when_done = format!("\"{}\"", program.display());
    settings.schedule.hang_up = true;

    let mut session = Session::default();
    let tick = session.schedule_tick(&backend, &settings, &[queued(1)], |_| true);
    assert!(!tick.hang_up && tick.problems.is_empty(), "work is left");
    let tick = session.schedule_tick(&backend, &settings, &[], |_| true);
    assert!(tick.hang_up);
    assert_eq!(tick.problems.len(), 1, "{:?}", tick.problems);
    assert!(
        tick.problems[0].contains("could not start"),
        "{:?}",
        tick.problems
    );
    let tick = session.schedule_tick(&backend, &settings, &[], |_| true);
    assert!(!tick.hang_up && tick.problems.is_empty(), "once a window");
}

/// What the rows were at the read before the latest.
#[test]
fn the_read_before_is_kept_for_the_dry_queue_watch() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let state = tempfile::tempdir().unwrap();
    let backend = crate::app::backend::testing::backend(&runtime, state.path());
    let settings = backend.settings();
    let mut session = Session::default();
    session.read(&backend, &settings);
    let id = backend
        .add_with(
            "http://127.0.0.1:9/a.bin",
            edm_core::AddOptions {
                later: true,
                ..edm_core::AddOptions::default()
            },
        )
        .unwrap();
    let seen = session.read(&backend, &settings);
    assert_eq!(session.before(id), None, "not listed at the read before");
    let state_then = seen.snapshot.rows[0].state.clone();
    session.read(&backend, &settings);
    assert_eq!(session.before(id), Some(&state_then));
}
