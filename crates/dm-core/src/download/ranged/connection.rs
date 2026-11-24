// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! One connection of `run`: the ranges it takes one after another, each the next one not yet
//! taken, then one given up on, then the back half of the largest still under way, until none
//! is left for it.

use std::ops::ControlFlow;

use super::*;

/// One connection of `run`: what it shares with the others, where it stands among them, and
/// how it stands now.
pub(super) struct Connection {
    /// Its client: the second network path's, where it goes out on that.
    pub(super) client: reqwest::Client,
    pub(super) job: Arc<Job>,
    /// The probe's answer, for whichever connection takes range 0.
    pub(super) probe: Arc<std::sync::Mutex<Option<First>>>,
    /// How many of the planned ranges have been taken.
    pub(super) next: Arc<AtomicUsize>,
    /// Ranges a connection gave up on, for the others to take.
    pub(super) orphans: Arc<std::sync::Mutex<Vec<usize>>>,
    /// The connections at work.
    pub(super) active: Arc<AtomicUsize>,
    /// The connections with ranges of their own not yet done.
    pub(super) first_wave: Arc<AtomicUsize>,
    /// Told once those are done: helpers still waiting have nothing to wait for.
    pub(super) wave_over: tokio::sync::watch::Sender<bool>,
    pub(super) wave_over_seen: tokio::sync::watch::Receiver<bool>,
    /// How many of its connections `grow` lets in.
    pub(super) gate_seen: tokio::sync::watch::Receiver<Gate>,
    pub(super) planned: usize,
    pub(super) resumable: bool,
    /// When the connections started.
    pub(super) started: tokio::time::Instant,
    /// Whether it is one of the connections more than there are pieces.
    pub(super) helper: bool,
    /// Which of the connections `grow` lets in this one is.
    pub(super) grown_as: Option<usize>,
    /// The network it goes out on: 0 the system's route, 1 the second, 2 the third (gaps row
    /// K3).
    pub(super) network: u8,
    /// Counted among the connections at work (`active`): one with a range of its own from
    /// the start, a helper from when it takes one, and neither while it waits as one a
    /// limiting server turned away past the most it serves at once (`limits::rejoin`).
    pub(super) counted: bool,
    /// Whether this connection, left alone, has had its second go.
    pub(super) alone_again: bool,
}

/// Where a connection goes, other than on, after a step of its work.
enum Flow {
    /// Round again, to look for work.
    Again,
    /// Back to wait to be let in again by `grow`.
    Readmit,
    /// Done with ranges.
    Leave,
}

/// What a connection waiting for a range to halve found.
enum Found {
    Range(usize),
    /// The server turned a request away meanwhile: looked at again as one that limits.
    Again,
    Done,
    /// Taken back by `grow` while it waited: it waits to be let in again.
    TakenBack,
}

impl Connection {
    /// The connection's work, until no range is left for it.
    pub(super) async fn work(mut self) -> Result<(), Error> {
        // However it ends, the checks are told it is done with ranges.
        let mut leaving = Leaving {
            job: Arc::clone(&self.job),
            fetched: false,
        };
        if self.helper && self.grown_as.is_none() {
            tokio::select! {
                () = tokio::time::sleep(HELP_AFTER) => {}
                _ = self.wave_over_seen.wait_for(|over| *over) => return Ok(()),
            }
            self.job.control.check()?;
        }
        'connection: loop {
            if let Some(me) = self.grown_as {
                let let_in = tokio::select! {
                    gate = self.gate_seen.wait_for(|g| g.closed || me < g.allowed) => {
                        gate.is_ok_and(|g| me < g.allowed)
                    }
                    _ = self.wave_over_seen.wait_for(|over| *over) => false,
                };
                if !let_in {
                    break 'connection;
                }
                self.job.control.check()?;
            }
            loop {
                // Turned away past the most the server serves at once: back in when one at
                // work is done.
                if !self.counted && !self.helper {
                    rejoin(&self.job, &self.active).await?;
                    self.counted = true;
                }
                let index = match self.next_range().await? {
                    ControlFlow::Continue(index) => index,
                    ControlFlow::Break(Flow::Again) => continue,
                    ControlFlow::Break(Flow::Readmit) => continue 'connection,
                    ControlFlow::Break(Flow::Leave) => break 'connection,
                };
                if !self.counted {
                    self.counted = true;
                    self.active.fetch_add(1, Ordering::SeqCst);
                }
                crate::second_path::fetched_by(&self.job.parts, index, self.network);
                let first = if index == 0 {
                    crate::lock(&self.probe).take().map(Answer::Probe)
                } else {
                    None
                };
                let turned_away = AtomicBool::new(false);
                leaving.fetched = true;
                let (index, fetched) = fetch_chain(
                    &self.client,
                    &self.job,
                    index,
                    self.resumable,
                    &self.active,
                    first,
                    &turned_away,
                    self.network,
                )
                .await;
                // A connection waiting for a range worth halving looks again.
                self.job.moved.notify_waiters();
                match self.fetched(fetched, index, &turned_away)? {
                    ControlFlow::Continue(()) => {}
                    ControlFlow::Break(Flow::Again) => continue,
                    ControlFlow::Break(Flow::Readmit) => continue 'connection,
                    ControlFlow::Break(Flow::Leave) => break 'connection,
                }
                self.alone_again = false;
                // The others take up its share for a moment before any is judged.
                *crate::lock(&self.job.last_ended) = Some(tokio::time::Instant::now());
                // Taken back by `grow`, with its piece done: it waits to be let in again.
                if self
                    .grown_as
                    .is_some_and(|me| me >= self.gate_seen.borrow().allowed)
                {
                    if self.counted {
                        self.counted = false;
                        self.active.fetch_sub(1, Ordering::SeqCst);
                    }
                    continue 'connection;
                }
            }
        }
        if self.counted {
            self.active.fetch_sub(1, Ordering::SeqCst);
            // One waiting for a place may have one now.
            self.job.moved.notify_waiters();
        }
        if !self.helper && self.first_wave.fetch_sub(1, Ordering::SeqCst) == 1 {
            let _ = self.wave_over.send(true);
        }
        Ok(())
    }

    /// The range this connection takes next: one the server's limits leave it, the next
    /// planned one, one given up on, or the back half of the largest under way; or where it
    /// goes instead.
    async fn next_range(&mut self) -> Result<ControlFlow<Flow, usize>, Error> {
        if self.job.limits.limited() {
            // A helper is not worth asking a server that limits connections for.
            if self.helper {
                return Ok(ControlFlow::Break(Flow::Leave));
            }
            return Ok(match limited_work(&self.job).await? {
                Some(index) => ControlFlow::Continue(index),
                None => ControlFlow::Break(Flow::Leave),
            });
        }
        let counter = self.next.fetch_add(1, Ordering::Relaxed);
        let given = if counter < self.planned {
            Some(in_order(counter, self.planned))
        } else {
            crate::lock(&self.orphans).pop()
        };
        if let Some(index) = given {
            // An answer may have gone on into it already (`limits`).
            if !take(&self.job, index) {
                return Ok(ControlFlow::Break(Flow::Again));
            }
            return Ok(ControlFlow::Continue(index));
        }
        /* The watching ends early once the connections with
         * ranges of their own are done: they end only when no
         * range is left to halve (or a refusal, which ends a
         * helper here anyway), and ranges only shrink, so
         * there is nothing for a helper either. The last helper
         * to finish its range otherwise watched out its whole
         * window after the file was in, and the download waited
         * for it: 80 ms of every small slow file (gaps row 165). */
        // One `grow` let in has been judged worth it already.
        if self.helper
            && (self.job.limits.limited()
                || (self.grown_as.is_none()
                    && !tokio::select! {
                        worth = slow_enough_to_help(&self.job, self.started) => worth,
                        _ = self.wave_over_seen.wait_for(|over| *over) => false,
                    })
                // Looked at again after the watching: a refusal
                // while it went on is as good as one before.
                || self.job.limits.limited())
        {
            return Ok(ControlFlow::Break(Flow::Leave));
        }
        Ok(match self.wait_for_range().await? {
            Found::Range(index) => ControlFlow::Continue(index),
            Found::Again => ControlFlow::Break(Flow::Again),
            Found::Done => ControlFlow::Break(Flow::Leave),
            Found::TakenBack => ControlFlow::Break(Flow::Readmit),
        })
    }

    /// A range worth halving, or a wait for one: a range can stop or slow down, and one just
    /// asked for is judged once its bytes come. Looked at again as soon as a range ends or is
    /// given up, or after `SPLIT_LOOK`, and given up only once no range under way is long
    /// enough to halve.
    async fn wait_for_range(&mut self) -> Result<Found, Error> {
        loop {
            // Asked for before looking, so a range that ends meanwhile still
            // wakes it.
            let moved = self.job.moved.notified();
            tokio::pin!(moved);
            moved.as_mut().enable();
            let now = tokio::time::Instant::now();
            let judge_from =
                crate::lock(&self.job.last_ended).map_or(now, |ended| ended + SPLIT_SETTLE);
            match split_largest(&self.job, now >= judge_from).await {
                Halving::Range(index) => return Ok(Found::Range(index)),
                Halving::Nothing => return Ok(Found::Done),
                Halving::Later => {}
            }
            let wake = if now < judge_from {
                judge_from
            } else {
                now + SPLIT_LOOK
            };
            tokio::select! {
                () = &mut moved => {}
                () = tokio::time::sleep_until(wake) => {}
            }
            self.job.control.check()?;
            // The server has turned a request away: looked at as one that
            // limits connections (above).
            if self.job.limits.limited() {
                return Ok(Found::Again);
            }
            let orphan = crate::lock(&self.orphans).pop();
            if let Some(index) = orphan.filter(|&index| take(&self.job, index)) {
                return Ok(Found::Range(index));
            }
            // Taken back by `grow` while it waited: it waits to be let in again.
            if self
                .grown_as
                .is_some_and(|me| me >= self.gate_seen.borrow().allowed)
            {
                if self.counted {
                    self.counted = false;
                    self.active.fetch_sub(1, Ordering::SeqCst);
                }
                return Ok(Found::TakenBack);
            }
        }
    }

    /// What follows range `index`'s fetch: on, round again for other work, done with
    /// ranges, or the download's failure.
    fn fetched(
        &mut self,
        fetched: Result<(), Error>,
        index: usize,
        turned_away: &AtomicBool,
    ) -> Result<ControlFlow<Flow>, Error> {
        match fetched {
            Ok(()) => Ok(ControlFlow::Continue(())),
            /* The server would not take this connection (a 429 or
             * 503 at once, a refused connect after its retries)
             * while others are working: this one steps aside and
             * leaves its range to them, as a server that limits
             * connections asks, rather than failing a download the
             * rest are fetching. */
            Err(e) if crate::retry::transient(&e) && self.active.load(Ordering::SeqCst) > 1 => {
                give_back(&self.job, &self.orphans, index);
                // Only a server's refusal makes it one that limits connections;
                // a range that keeps failing otherwise is left to the others.
                if self.helper || !turned_away.load(Ordering::SeqCst) {
                    return Ok(ControlFlow::Break(Flow::Leave));
                }
                /* Past the most the server has served at once, it waits for a
                 * place among those at work; within it, for the server's pace,
                 * and looks for work again. Ending, it never came back: from the
                 * twelfth request on, a two-at-once server's file was fetched on
                 * one connection (gaps row 197). */
                if self.active.load(Ordering::SeqCst) > self.job.together.most().max(1) {
                    self.counted = false;
                    self.active.fetch_sub(1, Ordering::SeqCst);
                    self.job.moved.notify_waiters();
                }
                Ok(ControlFlow::Break(Flow::Again))
            }
            /* The others stepped aside between the judgement and
             * now, and this is the last connection: its range is
             * nobody else's to fetch, so it is tried once more
             * here, with the tries a connection alone gets. */
            Err(e) if crate::retry::transient(&e) && !self.alone_again => {
                self.alone_again = true;
                // Given back, so it is this range that is tried
                // again and not a fresh one taken in its place.
                give_back(&self.job, &self.orphans, index);
                Ok(ControlFlow::Break(Flow::Again))
            }
            Err(e) => Err(e),
        }
    }
}
