// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

fn row(id: Id, state: State, downloaded: u64) -> Arc<Entry> {
    Arc::new(
        serde_json::from_value(serde_json::json!({
            "id": id,
            "url": format!("https://e.com/{id}"),
            "state": serde_json::to_value(state).unwrap(),
            "progress": {"downloaded": downloaded, "total": 100, "speed": 0.0, "eta": null},
            "filename": "f",
        }))
        .unwrap(),
    )
}

#[test]
fn the_first_read_seeds_and_later_ones_tell_what_happened() {
    let mut events = Events::default();
    let sub = events.subscribe(None);
    let a = row(1, State::Queued, 0);
    events.publish(std::slice::from_ref(&a));
    assert!(sub.drain().is_empty(), "what was there is not added");
    let running = row(1, State::Running, 10);
    let b = row(2, State::Queued, 0);
    events.publish(&[running.clone(), b.clone()]);
    assert_eq!(
        sub.drain(),
        [
            DownloadEvent::StateChanged {
                id: 1,
                state: State::Running
            },
            DownloadEvent::Added {
                id: 2,
                state: State::Queued
            },
            DownloadEvent::Progress {
                id: 1,
                progress: running.progress
            },
        ]
    );
    let done = row(1, State::Completed, 100);
    let failed = row(2, State::Failed("404".into()), 0);
    events.publish(&[done.clone(), failed]);
    let got = sub.drain();
    assert!(got.contains(&DownloadEvent::Completed(1)));
    assert!(got.contains(&DownloadEvent::Failed {
        id: 2,
        error: "404".into()
    }));
    events.publish(&[done]);
    assert_eq!(sub.drain(), [DownloadEvent::Removed(2)]);
}

/// One row added and another removed in the same read: both told, though
/// the list is as long as before.
#[test]
fn a_row_added_and_another_removed_at_once_are_both_told() {
    let mut events = Events::default();
    let sub = events.subscribe(None);
    events.publish(&[row(1, State::Queued, 0)]);
    events.publish(&[row(2, State::Queued, 0)]);
    assert_eq!(
        sub.drain(),
        [
            DownloadEvent::Added {
                id: 2,
                state: State::Queued
            },
            DownloadEvent::Removed(1),
        ]
    );
}

/// Nobody listening costs nothing and remembers nothing; a subscriber
/// who comes later is told from its first read on, not of the past.
#[test]
fn with_nobody_listening_nothing_is_kept_and_a_late_subscriber_starts_fresh() {
    let mut events = Events::default();
    events.publish(&[row(1, State::Queued, 0)]);
    assert!(events.last.is_empty());
    let sub = events.subscribe(None);
    events.publish(&[row(1, State::Running, 5)]);
    assert!(
        sub.drain().is_empty(),
        "the first read after subscribing seeds"
    );
    events.publish(&[row(1, State::Paused, 5)]);
    assert_eq!(
        sub.drain(),
        [DownloadEvent::StateChanged {
            id: 1,
            state: State::Paused
        }]
    );
}

/// Every subscriber gone: what was seen is let go, and one who subscribes
/// again is told from its own first read, not of what happened between.
#[test]
fn when_the_last_subscriber_leaves_what_was_seen_is_let_go() {
    let mut events = Events::default();
    let sub = events.subscribe(None);
    events.publish(&[row(1, State::Queued, 0)]);
    assert_eq!(format!("{events:?}"), "Events { rows: 1, subscribers: 1 }");
    drop(sub);
    events.publish(&[row(1, State::Running, 5)]);
    assert!(events.last.is_empty() && !events.seeded);
    assert_eq!(format!("{events:?}"), "Events { rows: 0, subscribers: 0 }");
    let again = events.subscribe(None);
    events.publish(&[row(1, State::Running, 5), row(2, State::Queued, 0)]);
    assert!(again.drain().is_empty(), "seeded afresh");
}

#[test]
fn progress_is_coalesced_and_no_lifecycle_event_is_lost() {
    let mut events = Events::default();
    let sub = events.subscribe(None);
    events.publish(&[row(1, State::Queued, 0)]);
    // Many reads between two drains: every state change kept, one progress.
    events.publish(&[row(1, State::Running, 1)]);
    for n in 2..1000 {
        events.publish(&[row(1, State::Running, n)]);
    }
    events.publish(&[row(1, State::Paused, 999)]);
    events.publish(&[row(1, State::Running, 999)]);
    let got = sub.drain();
    let changes: Vec<&State> = got
        .iter()
        .filter_map(|e| match e {
            DownloadEvent::StateChanged { state, .. } => Some(state),
            _ => None,
        })
        .collect();
    assert_eq!(changes, [&State::Running, &State::Paused, &State::Running]);
    let progress: Vec<u64> = got
        .iter()
        .filter_map(|e| match e {
            DownloadEvent::Progress { progress, .. } => Some(progress.downloaded),
            _ => None,
        })
        .collect();
    assert_eq!(progress, [999], "one, the newest");
}

#[test]
fn subscribers_are_independent_and_a_dropped_one_stops_collecting() {
    let mut events = Events::default();
    let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let w = Arc::clone(&woken);
    let first = events.subscribe(Some(Box::new(move || {
        w.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    })));
    let second = events.subscribe(None);
    events.publish(&[]);
    events.publish(&[row(1, State::Queued, 0)]);
    assert_eq!(first.drain().len(), 1);
    assert!(
        second.pending(),
        "the first's drain took nothing of the second's"
    );
    assert_eq!(second.drain().len(), 1);
    assert_eq!(woken.load(std::sync::atomic::Ordering::Relaxed), 1);
    drop(second);
    assert_eq!(events.subscribers(), 1);
    // Nothing changed: nobody is woken.
    events.publish(&[row(1, State::Queued, 0)]);
    assert_eq!(woken.load(std::sync::atomic::Ordering::Relaxed), 1);
}

#[test]
fn an_unchanged_shared_row_is_passed_over() {
    let mut events = Events::default();
    let sub = events.subscribe(None);
    let rows: Vec<Arc<Entry>> = (0..10_000).map(|id| row(id, State::Queued, 0)).collect();
    events.publish(&rows);
    let started = std::time::Instant::now();
    for _ in 0..10 {
        events.publish(&rows);
    }
    assert!(sub.drain().is_empty());
    // Ten reads of 10,000 unchanged rows: pointer checks only.
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
}
