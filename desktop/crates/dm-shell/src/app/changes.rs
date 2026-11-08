// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! That the queue changed, told to every presentation that draws it.
//!
//! The queue tells one callback (`Queue::on_change`); [`connect`] makes that
//! callback this module's [`changed`], which marks [`CHANGED`] and then calls
//! every subscriber, so several presentations (a window, a headless loop, a
//! second front end) are each told. What changed is not carried: a
//! presentation reads the queue again when it next draws, so a burst of a
//! thousand changes costs it one read, and no change is ever lost to a full
//! channel.
//!
//! The rules a subscriber keeps:
//! - It is called on whatever thread the engine changed the queue on, many
//!   times a second while downloads run. It only signals (sets a flag, asks for
//!   a repaint, wakes a loop); it never blocks, locks the queue or reads it.
//! - [`take`] is called before the queue is read, never after: a change made
//!   while a presentation reads is then still owed a read afterwards.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use dm_core::Queue;

/// Set whenever the queue changes, cleared by [`take`] before a read of it.
/// The window decides when to look again from the list it last read, which
/// cannot say that something has just started: a change made after a read,
/// while nothing moved, was seen only when another change or a keystroke came
/// (a resumed download showed "Stopped" until it finished). This says it is
/// still owed.
pub static CHANGED: AtomicBool = AtomicBool::new(false);

type Subscriber = Arc<dyn Fn() + Send + Sync>;

static SUBSCRIBERS: Mutex<Vec<Subscriber>> = Mutex::new(Vec::new());

/// Makes the queue's change callback this module's: from now on every change
/// marks [`CHANGED`] and tells each subscriber. Once per queue; a second call
/// replaces the first, as `Queue::on_change` does.
pub fn connect(queue: &Queue) {
    queue.on_change(changed);
}

/// Adds `tell` to the subscribers, for the life of the process. See the
/// module's rules: it must only signal, never block.
pub fn subscribe(tell: impl Fn() + Send + Sync + 'static) {
    lock().push(Arc::new(tell));
}

/// What the queue's change callback does: [`CHANGED`] first, so a subscriber
/// woken by it finds the mark set, then each subscriber.
pub fn changed() {
    CHANGED.store(true, Ordering::Release);
    // Called outside the lock: a subscriber that subscribed another would
    // otherwise wait on itself.
    let subscribers: Vec<Subscriber> = lock().clone();
    for tell in subscribers {
        tell();
    }
}

/// Clears the mark, before the queue is read, and says whether it was set.
pub fn take() -> bool {
    CHANGED.swap(false, Ordering::AcqRel)
}

/// Whether a change is owed a read, without clearing it.
pub fn pending() -> bool {
    CHANGED.load(Ordering::Acquire)
}

fn lock() -> std::sync::MutexGuard<'static, Vec<Subscriber>> {
    // A subscriber that panicked held nothing the list depends on.
    SUBSCRIBERS.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// Every subscriber is told of each change, the mark is set before they
    /// are, and taking it clears it once.
    #[test]
    fn every_subscriber_is_told_and_the_mark_is_set_first() {
        let (a, b) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let seen_marked = Arc::new(AtomicBool::new(true));
        {
            let (a, marked) = (a.clone(), seen_marked.clone());
            subscribe(move || {
                a.fetch_add(1, Ordering::SeqCst);
                if !pending() {
                    marked.store(false, Ordering::SeqCst);
                }
            });
        }
        {
            let b = b.clone();
            subscribe(move || {
                b.fetch_add(1, Ordering::SeqCst);
            });
        }
        let _ = take();
        changed();
        changed();
        assert!(a.load(Ordering::SeqCst) >= 2 && b.load(Ordering::SeqCst) >= 2);
        assert!(
            seen_marked.load(Ordering::SeqCst),
            "a subscriber found the mark unset"
        );
        assert!(take(), "the change was owed a read");
        assert!(!take(), "and taking it cleared it");
    }
}
