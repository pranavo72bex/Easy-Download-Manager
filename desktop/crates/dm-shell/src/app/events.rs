// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The application's events, typed, for any number of subscribers: what
//! happened to each download, from one read of the queue to the next.
//!
//! Where they come from: the queue signals that it changed (`changes`), a
//! presentation reads it (`Session::read`), and [`Events::publish`] compares
//! that read with the last, row by row. A row whose shared entry is the same
//! as last time is passed over without looking inside it, so a read of
//! 10,000 rows where a few move costs a few comparisons and 10,000 pointer
//! checks. Nobody polls the engine: no read, no events.
//!
//! How they are delivered: each [`Subscription`] keeps its own events until
//! it drains them, so subscribers are independent and none waits on another.
//! - Lifecycle events (added, state changed, completed, failed, removed) are
//!   kept in order and never dropped. A subscriber that never drains keeps
//!   them all; one that is dropped stops collecting at once.
//! - Progress is coalesced: one entry a row, the newest, however many reads
//!   came between two drains. A subscriber's memory for it is bounded by the
//!   rows, not by how often progress moved.
//! - Publishing takes each subscriber's lock for the time it takes to append,
//!   and calls its waker after letting go: a subscriber cannot block the
//!   engine, nor the presentation that reads the queue.
//!
//! The words, sounds and dialogs a presentation makes of an event are its own
//! (the window's toasts come from `detect`, whose `Detected` are the same
//! completions and failures, told once each).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, Weak};

use dm_core::{Entry, Id, State};

/// What happened to one download.
#[derive(Debug, Clone, PartialEq)]
pub enum DownloadEvent {
    /// A row appeared, in this state.
    Added {
        /// The row.
        id: Id,
        /// Its state when it was first seen.
        state: State,
    },
    /// A row's state changed (a completion and a failure are also told as
    /// [`DownloadEvent::Completed`] and [`DownloadEvent::Failed`]).
    StateChanged {
        /// The row.
        id: Id,
        /// Its new state.
        state: State,
    },
    /// A row finished.
    Completed(Id),
    /// A row failed, with the engine's words.
    Failed {
        /// The row.
        id: Id,
        /// Why, as the engine said it.
        error: String,
    },
    /// A row left the list.
    Removed(Id),
    /// How far a row has got: the newest since the subscriber last drained.
    Progress {
        /// The row.
        id: Id,
        /// What it has fetched, how big it is, and how fast it goes.
        progress: dm_core::Progress,
    },
}

/// One subscriber's events until it drains them.
#[derive(Default)]
struct Pending {
    lifecycle: VecDeque<DownloadEvent>,
    progress: HashMap<Id, dm_core::Progress>,
    /// The order rows' progress first arrived in since the last drain, so a
    /// drain gives progress in a stable order.
    progress_order: Vec<Id>,
}

struct Inner {
    pending: Mutex<Pending>,
    wake: Option<Box<dyn Fn() + Send + Sync>>,
}

/// One subscriber's end: drain it when convenient. Dropped, it stops
/// collecting.
pub struct Subscription {
    inner: Arc<Inner>,
}

impl Subscription {
    /// Every event since the last drain: the lifecycle events in the order
    /// they happened, then each row's newest progress.
    pub fn drain(&self) -> Vec<DownloadEvent> {
        let mut pending = lock(&self.inner.pending);
        let lifecycle = std::mem::take(&mut pending.lifecycle);
        let order = std::mem::take(&mut pending.progress_order);
        let mut progress = std::mem::take(&mut pending.progress);
        drop(pending);
        let mut out: Vec<DownloadEvent> = lifecycle.into_iter().collect();
        out.extend(order.into_iter().filter_map(|id| {
            progress
                .remove(&id)
                .map(|progress| DownloadEvent::Progress { id, progress })
        }));
        out
    }

    /// Whether anything is waiting to be drained.
    pub fn pending(&self) -> bool {
        let pending = lock(&self.inner.pending);
        !pending.lifecycle.is_empty() || !pending.progress.is_empty()
    }
}

/// What a row was at the last read: the shared entry, to pass over it
/// unchanged, and what the events compare.
struct Seen {
    entry: Arc<Entry>,
    /// The read it was last seen in: one not seen in this read has left.
    stamp: u64,
}

/// The publisher: what the last read held, and who is subscribed.
#[derive(Default)]
pub struct Events {
    last: HashMap<Id, Seen>,
    subscribers: Vec<Weak<Inner>>,
    seeded: bool,
    stamp: u64,
}

impl std::fmt::Debug for Events {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Events")
            .field("rows", &self.last.len())
            .field("subscribers", &self.subscribers.len())
            .finish()
    }
}

impl Events {
    /// A new subscriber, told of everything from the next read on. `wake`, if
    /// given, is called after events are added for it (it must only signal:
    /// set a flag, ask for a repaint); the subscriber drains when it likes.
    pub fn subscribe(&mut self, wake: Option<Box<dyn Fn() + Send + Sync>>) -> Subscription {
        let inner = Arc::new(Inner {
            pending: Mutex::new(Pending::default()),
            wake,
        });
        self.subscribers.push(Arc::downgrade(&inner));
        Subscription { inner }
    }

    /// How many subscribers are still listening.
    pub fn subscribers(&mut self) -> usize {
        self.subscribers.retain(|s| s.strong_count() > 0);
        self.subscribers.len()
    }

    /// Compares `rows` (a read of the queue) with the last, and tells every
    /// subscriber what happened. The first read seeds it: what was already
    /// there is not told as added.
    pub fn publish(&mut self, rows: &[Arc<Entry>]) {
        self.subscribers.retain(|s| s.strong_count() > 0);
        // Nobody listening: no work at all (the window today). The first read
        // after someone subscribes seeds again.
        if self.subscribers.is_empty() {
            if self.seeded {
                self.last = HashMap::new();
                self.seeded = false;
            }
            return;
        }
        let seeded = self.seeded;
        self.seeded = true;
        self.stamp = self.stamp.wrapping_add(1);
        let stamp = self.stamp;
        let mut lifecycle = Vec::new();
        let mut progress = Vec::new();
        // How many of the last read's rows this read has: fewer, and some left.
        let (was, mut matched) = (self.last.len(), 0usize);
        // Updated in place, each row stamped with this read: a map of 10,000
        // rebuilt at every read cost more than the comparisons.
        for row in rows {
            let id = row.id;
            match self.last.get_mut(&id) {
                Some(seen) => {
                    seen.stamp = stamp;
                    matched += 1;
                    // The same shared entry: nothing in it changed.
                    if Arc::ptr_eq(&seen.entry, row) {
                        continue;
                    }
                    let before = &seen.entry;
                    if before.state != row.state {
                        lifecycle.push(DownloadEvent::StateChanged {
                            id,
                            state: row.state.clone(),
                        });
                        match &row.state {
                            State::Completed => lifecycle.push(DownloadEvent::Completed(id)),
                            State::Failed(why) => lifecycle.push(DownloadEvent::Failed {
                                id,
                                error: why.clone(),
                            }),
                            _ => {}
                        }
                    }
                    if before.progress.downloaded != row.progress.downloaded
                        || before.progress.total != row.progress.total
                        || before.progress.speed.to_bits() != row.progress.speed.to_bits()
                    {
                        progress.push((id, row.progress));
                    }
                    seen.entry = Arc::clone(row);
                }
                None => {
                    if seeded {
                        lifecycle.push(DownloadEvent::Added {
                            id,
                            state: row.state.clone(),
                        });
                    }
                    self.last.insert(
                        id,
                        Seen {
                            entry: Arc::clone(row),
                            stamp,
                        },
                    );
                }
            }
        }
        if matched < was {
            let mut gone: Vec<Id> = Vec::new();
            self.last.retain(|id, seen| {
                let here = seen.stamp == stamp;
                if !here {
                    gone.push(*id);
                }
                here
            });
            if seeded {
                gone.sort_unstable();
                lifecycle.extend(gone.into_iter().map(DownloadEvent::Removed));
            }
        }
        if lifecycle.is_empty() && progress.is_empty() {
            return;
        }
        for subscriber in self.subscribers.iter().filter_map(Weak::upgrade) {
            {
                let mut pending = lock(&subscriber.pending);
                pending.lifecycle.extend(lifecycle.iter().cloned());
                for (id, p) in &progress {
                    if pending.progress.insert(*id, *p).is_none() {
                        pending.progress_order.push(*id);
                    }
                }
            }
            if let Some(wake) = &subscriber.wake {
                wake();
            }
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // What a panicking subscriber held is plain data, still usable.
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
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
}
