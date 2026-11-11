// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Work whose cost grows with a file's length and is the computer's, not the network's: a
//! finished file's digest and the join of its parts (gaps row 185).
//!
//! It runs on tokio's blocking pool, a few pieces of it at a time: as many at once as half the
//! computer's cores, at least one, however many downloads and connections the settings let run.
//! Ten downloads finishing together used to hash and copy ten files at once, each on a thread of
//! the pool every file write also waits for, with nothing to say how many. The work itself looks
//! at its download's control between pieces (`stopped`), so a Cancel ends it within a piece, not
//! at the file's end.

use std::sync::{Condvar, LazyLock, Mutex};

use crate::control::{Control, Wanted};
use crate::Error;

/// How many may run at once: half the cores, at least one. Set apart from the network's limits
/// (downloads at once, connections each), which say nothing of the computer's cores.
fn at_once() -> usize {
    std::thread::available_parallelism().map_or(1, |n| (n.get() / 2).max(1))
}

/// The turns free, and the bell rung as one is given back. Plain, not tokio's: a thread of its
/// own (an extraction's) waits for a turn as the blocking pool's threads do, where nothing can
/// be awaited.
struct Turns {
    free: Mutex<usize>,
    given_back: Condvar,
}

static TURNS: LazyLock<Turns> = LazyLock::new(|| Turns {
    free: Mutex::new(at_once()),
    given_back: Condvar::new(),
});

/// A turn at the computer's heavy work, held while it runs and given back as it is dropped.
#[must_use = "the turn is given back as soon as it is dropped"]
pub struct Turn(());

impl Drop for Turn {
    fn drop(&mut self) {
        *crate::lock(&TURNS.free) += 1;
        TURNS.given_back.notify_one();
        GIVEN_BACK.notify_waiters();
    }
}

/// Rung as a turn is given back, for work that waits for one on the runtime: it waits there,
/// holding no thread, and goes to the blocking pool only with its turn.
static GIVEN_BACK: LazyLock<tokio::sync::Notify> = LazyLock::new(tokio::sync::Notify::new);

/// A turn, if one is free now.
fn try_turn() -> Option<Turn> {
    let mut free = crate::lock(&TURNS.free);
    if *free == 0 {
        return None;
    }
    *free -= 1;
    Some(Turn(()))
}

/// Waits, on the calling thread, for a turn at the computer's heavy work: for work on a thread of
/// its own (a ZIP's extraction), never on the runtime's threads.
pub fn turn() -> Turn {
    let mut free = crate::lock(&TURNS.free);
    while *free == 0 {
        free = TURNS
            .given_back
            .wait(free)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
    }
    *free -= 1;
    Turn(())
}

/// Runs `work` on the blocking pool once a turn is free, and gives its answer. The turn is waited
/// for here, holding no thread: waiting in the pool, the work queued behind the turns would hold
/// threads every file write also needs.
pub(crate) async fn run<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, Error> {
    let turn = loop {
        // Asked for before the look, so a turn given back in between still wakes it.
        let given_back = GIVEN_BACK.notified();
        if let Some(turn) = try_turn() {
            break turn;
        }
        given_back.await;
    };
    tokio::task::spawn_blocking(move || {
        let _turn = turn;
        work()
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))
}

/// Whether work for the download `control` belongs to was cancelled, to be looked at between
/// pieces. A pause is not a stop here: the bytes are all here by then, and a join or a digest cut
/// short by a pause would lose the joined file a resume needs, so they finish as they always did.
pub(crate) fn stopped(control: &Control) -> Result<(), Error> {
    match control.wanted() {
        Wanted::Cancel => Err(Error::Cancelled),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Never none at once, whatever the computer.
    #[test]
    fn at_least_one_runs_at_a_time() {
        assert!(at_once() >= 1);
    }

    /// No more turns than there are, and each given back as it is dropped: a thread asking for
    /// one more waits until then.
    #[test]
    fn a_turn_waits_for_one_given_back() {
        let held: Vec<Turn> = (0..at_once()).map(|_| turn()).collect();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _turn = turn();
            tx.send(()).unwrap();
        });
        assert!(
            rx.recv_timeout(std::time::Duration::from_millis(200))
                .is_err(),
            "a turn was given past the limit"
        );
        drop(held);
        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok(),
            "a turn given back was not taken"
        );
    }

    /// A cancel is a stop; a pause is not.
    #[test]
    fn only_a_cancel_stops_the_work() {
        let control = Control::new();
        assert!(stopped(&control).is_ok());
        control.pause();
        assert!(stopped(&control).is_ok());
        control.cancel();
        assert!(matches!(stopped(&control), Err(Error::Cancelled)));
    }
}
