// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the schedule adds to the queue beyond holding it (Scheduler › When done
//! and Try a failed download again).
//!
//! When its window opens with something waiting to run, the schedule's choice
//! of what dm does at the end is armed as the session's own (Downloads › When
//! done), so the same cancellable countdown stands between it and the
//! computer; closing the window with the choice unused puts it away again. And
//! while the window is open a download that failed is tried again, up to the
//! number the person set, each time the queue has run dry. When the scheduled
//! downloads are done, the program set is run and, on Windows, the dial-up or
//! VPN connection hung up where the person asked (reference A's Scheduler).

use super::*;

impl DmApp {
    /// A once-only date that is over is put away, so the schedule goes back to its weekly window
    /// (left set, its queue would stay shut for good).
    fn clear_spent_once_at(&mut self) {
        use chrono::{Datelike, Timelike};
        let now = chrono::Local::now();
        let Some(wanted) = dm_shell::app::schedule::spent_once_put_away(
            &self.applied,
            (now.year(), now.month() as u8, now.day() as u8),
            now.hour() * 60 + now.minute(),
        ) else {
            return;
        };
        if let Err(why) = self.apply_settings(wanted) {
            self.problems.push(why);
        }
    }

    /// Called after every read of the list.
    pub(super) fn schedule_hooks(&mut self) {
        self.clear_spent_once_at();
        // Done by the application (`Session::schedule_tick`); the window says
        // what the schedule armed and hangs up where it is asked to.
        let tick = self.session.schedule_tick(
            &self.backend,
            &self.applied,
            &self.rows,
            crate::power::available,
        );
        if let Some(wanted) = tick.armed {
            self.said = crate::i18n::Said::new(
                "The schedule has set what dm does when the downloads are done: {}.",
                &[&crate::i18n::t(wanted.verb())],
            );
        }
        self.problems.extend(tick.problems);
        if tick.hang_up && cfg!(windows) {
            self.hang_up_later();
        }
    }
}

impl DmApp {
    /// Hangs up the connection Options › Advanced › Dial Up / VPN sets, off
    /// the window's thread (Windows takes a moment to end it); what it came
    /// to is said when it is known.
    fn hang_up_later(&mut self) {
        if let Some(hung) = dm_shell::dialup::hang_up_later(Arc::clone(&self.hung_up)) {
            self.say_hung_up(hung);
        }
    }

    /// Says what hanging up came to: a connection that was not up needs no
    /// word, one Windows would not end is a problem.
    pub(super) fn say_hung_up(&mut self, hung: dm_shell::dialup::HungUp) {
        use dm_shell::dialup::HungUp;
        match hung {
            HungUp::Done(name) => {
                self.said = crate::i18n::Said::new(
                    "The scheduled downloads are done: {} is hung up.",
                    &[&name],
                );
            }
            HungUp::WasDown(_) => {}
            HungUp::NoneSet => {
                self.said = crate::i18n::Said::new(
                    "Hang up modem when done: no connection is set in Options › Advanced › Dial Up / VPN.",
                    &[],
                );
            }
            HungUp::Failed(name, why) => self
                .problems
                .push(crate::i18n::tf("{} was not hung up: {}", &[&name, &why])),
        }
    }
}
