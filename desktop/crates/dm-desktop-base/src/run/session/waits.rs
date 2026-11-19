// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Every download held while the connection is metered or the battery low, as Options ›
//! Downloads says (gaps row W11): the state read every few seconds while Options asks for it,
//! the queue told what holds it (`Backend::hold_for`), and "ask" asked once each time the
//! connection becomes metered. With no window nobody is asked, and "ask" waits.

use std::time::{Duration, Instant};

use dm_core::Hold;
use dm_shell::app::backend::Backend;

use crate::metered::{hold_of, watched, Reading, Source, System};

use super::{Driver, Effect, Session};

/// How often the connection and the battery are read while Options asks for them: soon enough
/// that a download waits within moments of a hotspot, rarely enough to cost nothing.
const READ_EVERY: Duration = Duration::from_secs(5);

/// What the tick knows of the computer's state between reads.
#[derive(Default)]
pub(super) struct Waits {
    /// Where the state is read from: the computer's own calls, unless a test stood a fake in.
    source: Option<Box<dyn Source>>,
    /// When it was last read; none while Options asks for nothing.
    read_at: Option<Instant>,
    /// What it said then.
    reading: Reading,
    /// The person's answer for this metered period: go on (`true`) or wait.
    answer: Option<bool>,
    /// Whether this metered period's question has been asked.
    asked: bool,
}

impl std::fmt::Debug for Waits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Waits")
            .field("read_at", &self.read_at)
            .field("reading", &self.reading)
            .field("answer", &self.answer)
            .field("asked", &self.asked)
            .finish_non_exhaustive()
    }
}

impl Session {
    /// The computer's state read from `source` from now on, in place of the computer's own
    /// calls: a test's fake.
    pub fn read_state_from(&mut self, source: Box<dyn Source>) {
        self.waits.source = Some(source);
        self.waits.read_at = None;
    }

    /// The person's answer to "the connection is metered" ([`Effect::AskMetered`]): go on
    /// (`true`) or wait, until the connection is no longer metered. In force at the next tick.
    pub fn answer_metered(&mut self, go_on: bool) {
        self.waits.answer = Some(go_on);
    }

    /// The state read where due, and the queue held for it or let go, as Options › Downloads
    /// says; the question asked once a metered period where Options says to ask and someone
    /// sees the window.
    pub(super) fn waits(&mut self, backend: &Backend, driver: Driver, effects: &mut Vec<Effect>) {
        let downloads = backend.settings().downloads;
        let now = Instant::now();
        let waits = &mut self.waits;
        if !watched(&downloads.on_metered, downloads.battery_below) {
            waits.read_at = None;
            waits.reading = Reading::default();
        } else if waits
            .read_at
            .is_none_or(|at| now.duration_since(at) >= READ_EVERY)
        {
            waits.read_at = Some(now);
            waits.reading = match &waits.source {
                Some(source) => source.read(),
                None => System.read(),
            };
        }
        // A metered period ended: its answer and its question go with it.
        if waits.reading.metered != Some(true) {
            waits.answer = None;
            waits.asked = false;
        }
        // With no window nobody is asked: "ask" waits, its reason said on the rows.
        let asks = downloads.on_metered == dm_shell::settings::METERED_ASK && !driver.nobody_sees;
        let answer = if asks { waits.answer } else { None };
        let hold = hold_of(
            &waits.reading,
            &downloads.on_metered,
            downloads.battery_below,
            answer,
        );
        if hold == Some(Hold::Metered) && asks && answer.is_none() && !waits.asked {
            waits.asked = true;
            effects.push(Effect::AskMetered);
        }
        if backend.held_by() != hold {
            backend.hold_for(hold);
        }
    }
}
