// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Decisions the application makes about a row, once, for every place that
//! acts on them: the menu, the progress window, Add URL, the control port and
//! any other presentation. Each place keeps its own way of saying what it
//! decided (a question, a sentence, an error), never its own copy of the rule.

use dm_core::State;

/// What "go on" means for a row as it stands: Resume on a stopped one, Retry
/// on a failed one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoOn {
    /// Stopped by a person: resumed where it stopped.
    Resume,
    /// Failed: tried again.
    Retry,
    /// Stopped by the schedule or the download limit: resuming would only put
    /// it back to be held again, so the person is asked (or told) instead.
    Held,
    /// Finished or cancelled: there is nothing to go on with.
    Settled,
    /// Running or waiting its turn already.
    NotStopped,
}

/// What going on with a row in `state` is; `held` is whether the schedule or
/// the download limit stopped it (`Entry::paused_by_schedule`).
pub fn go_on(state: &State, held: bool) -> GoOn {
    match state {
        State::Paused if held => GoOn::Held,
        State::Paused => GoOn::Resume,
        State::Failed(_) => GoOn::Retry,
        State::Completed | State::Cancelled => GoOn::Settled,
        _ => GoOn::NotStopped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stopped_row_resumes_a_failed_one_retries_and_a_held_one_is_asked_about() {
        assert_eq!(go_on(&State::Paused, false), GoOn::Resume);
        assert_eq!(go_on(&State::Paused, true), GoOn::Held);
        assert_eq!(go_on(&State::Failed("x".into()), false), GoOn::Retry);
        // Held says nothing of a row that is not stopped.
        assert_eq!(go_on(&State::Failed("x".into()), true), GoOn::Retry);
        assert_eq!(go_on(&State::Completed, false), GoOn::Settled);
        assert_eq!(go_on(&State::Cancelled, true), GoOn::Settled);
        assert_eq!(go_on(&State::Queued, false), GoOn::NotStopped);
        assert_eq!(go_on(&State::Running, false), GoOn::NotStopped);
    }
}
