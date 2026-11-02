//! What the schedule adds to the queue beyond holding it (Scheduler › When done
//! and Try a failed download again).
//!
//! When its window opens with something waiting to run, the schedule's choice
//! of what dm does at the end is armed as the session's own (Downloads › When
//! done), so the same cancellable countdown stands between it and the
//! computer; closing the window with the choice unused puts it away again. And
//! while the window is open a download that failed is tried again, up to the
//! number the person set, each time the queue has run dry.

use super::*;

/// Watches the window open and close between two reads of the list.
#[derive(Debug, Default)]
pub(crate) struct Watch {
    was_open: bool,
    /// The action armed from the schedule when the window opened, until it
    /// is used or the window closes.
    armed: Option<crate::power::WhenDone>,
    /// Rounds of tries again made since the window opened.
    rounds: u8,
    /// Whether something was waiting to run since the window opened.
    had_work: bool,
    /// Whether the program to run at the end has been run in this window.
    ran: bool,
}

/// What a read of the list finds of the window.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Nothing,
    Opened,
    Closed,
}

impl Watch {
    pub(crate) fn step(&mut self, open: bool) -> Step {
        match (self.was_open, open) {
            (false, true) => {
                self.was_open = true;
                self.rounds = 0;
                self.had_work = false;
                self.ran = false;
                Step::Opened
            }
            (true, false) => {
                self.was_open = false;
                Step::Closed
            }
            _ => Step::Nothing,
        }
    }

    /// Whether the program to run at the end is due: the window is open, something
    /// was waiting in it, nothing is now, and it has not been run in this window.
    pub(crate) fn run_due(&mut self, open: bool, work_left: bool) -> bool {
        if !open {
            return false;
        }
        if work_left {
            self.had_work = true;
            return false;
        }
        if self.had_work && !self.ran {
            self.ran = true;
            return true;
        }
        false
    }

    pub(crate) fn arm(&mut self, action: crate::power::WhenDone) {
        self.armed = Some(action);
    }

    /// The armed action, taken back: the window closed or it was spent.
    pub(crate) fn disarm(&mut self) -> Option<crate::power::WhenDone> {
        self.armed.take()
    }

    /// Whether another round of tries again is owed: the person allowed
    /// `retries`, and this window has made fewer.
    pub(crate) fn retry_round(&mut self, retries: u8) -> bool {
        if self.rounds < retries.min(5) {
            self.rounds += 1;
            true
        } else {
            false
        }
    }
}

/// Whether anything is left to run: running, queued, or held by the schedule
/// (which starts again when it opens). What is stopped by a person or has
/// settled is not.
pub(crate) fn work_left(rows: &[Arc<dm_core::Entry>]) -> bool {
    rows.iter().any(|r| {
        matches!(r.state, State::Running | State::Queued)
            || (r.state == State::Paused && r.paused_by_schedule)
    })
}

impl DmApp {
    /// Called after every read of the list.
    pub(super) fn schedule_hooks(&mut self) {
        let schedule = self.applied.schedule.clone();
        let open = schedule.enabled && schedule.open();
        match self.schedule_watch.step(open) {
            Step::Opened => {
                let wanted = crate::power::WhenDone::from_word(&schedule.when_done);
                let can = wanted == crate::power::WhenDone::Nothing
                    || wanted == crate::power::WhenDone::Quit
                    || crate::power::available(wanted);
                // Only for something waiting to run, and never over a choice
                // the person made this session in the Downloads menu.
                if wanted != crate::power::WhenDone::Nothing
                    && can
                    && self.when_done == crate::power::WhenDone::Nothing
                    && work_left(&self.rows)
                {
                    self.when_done = wanted;
                    self.schedule_watch.arm(wanted);
                    self.said = crate::i18n::Said::new(
                        "The schedule has set what dm does when the downloads are done: {}.",
                        &[&crate::i18n::t(wanted.verb())],
                    );
                }
            }
            Step::Closed => {
                // Unused: put away, unless it was changed in the menu since.
                if let Some(armed) = self.schedule_watch.disarm() {
                    if self.when_done == armed {
                        self.when_done = crate::power::WhenDone::Nothing;
                    }
                }
            }
            Step::Nothing => {}
        }
        // Fired (the choice was taken as the queue ran dry): nothing to put away.
        if self.when_done == crate::power::WhenDone::Nothing {
            self.schedule_watch.disarm();
        }
        // The program a person set to run when the scheduled downloads are done,
        // once in a window, after something ran in it.
        let command = schedule.run_when_done.trim();
        if self.schedule_watch.run_due(open, work_left(&self.rows)) && !command.is_empty() {
            if let Err(why) = dm_shell::shell::run_program(command) {
                self.problems.push(why);
            }
        }
        // Failed downloads tried again while the window is open, a round each
        // time the queue has run dry.
        if open && schedule.retries > 0 && !work_left(&self.rows) {
            let failed: Vec<Id> = self
                .rows
                .iter()
                .filter(|r| matches!(r.state, State::Failed(_)))
                .map(|r| r.id)
                .collect();
            if !failed.is_empty() && self.schedule_watch.retry_round(schedule.retries) {
                for id in failed {
                    self.backend.retry(id);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_opening_and_closing_is_seen_once_each() {
        let mut watch = Watch::default();
        assert_eq!(watch.step(false), Step::Nothing);
        assert_eq!(watch.step(true), Step::Opened);
        assert_eq!(watch.step(true), Step::Nothing);
        assert_eq!(watch.step(false), Step::Closed);
        assert_eq!(watch.step(false), Step::Nothing);
    }

    #[test]
    fn tries_again_are_as_many_as_allowed_in_one_window_and_start_over_at_the_next() {
        let mut watch = Watch::default();
        watch.step(true);
        assert!(!watch.retry_round(0), "zero leaves it failed, as before");
        assert!(watch.retry_round(2));
        assert!(watch.retry_round(2));
        assert!(!watch.retry_round(2), "no more than two rounds");
        watch.step(false);
        watch.step(true);
        assert!(watch.retry_round(2), "a new window starts over");
        let mut capped = Watch::default();
        capped.step(true);
        let rounds = (0..10).filter(|_| capped.retry_round(200)).count();
        assert_eq!(rounds, 5, "never more than five");
    }

    #[test]
    fn an_armed_action_is_taken_back_once() {
        let mut watch = Watch::default();
        watch.arm(crate::power::WhenDone::Sleep);
        assert_eq!(watch.disarm(), Some(crate::power::WhenDone::Sleep));
        assert_eq!(watch.disarm(), None);
    }

    #[test]
    fn the_kept_words_name_the_actions_and_back() {
        use crate::power::WhenDone;
        for w in WhenDone::ALL {
            assert_eq!(WhenDone::from_word(w.word()), w);
        }
        assert_eq!(WhenDone::from_word(""), WhenDone::Nothing);
        assert_eq!(WhenDone::from_word("reboot"), WhenDone::Nothing);
        assert_eq!(WhenDone::from_word(" sleep "), WhenDone::Sleep);
    }
}

#[cfg(test)]
mod run_tests {
    use super::Watch;

    #[test]
    fn the_program_at_the_end_runs_once_after_something_ran_in_the_window() {
        let mut watch = Watch::default();
        watch.step(true);
        // Nothing waited in this window: nothing to do at its end.
        assert!(!watch.run_due(true, false));
        // Something runs, then the queue runs dry: due once.
        assert!(!watch.run_due(true, true));
        assert!(watch.run_due(true, false));
        assert!(!watch.run_due(true, false), "twice in one window");
        // Closed: never. A new window starts over.
        assert!(!watch.run_due(false, false));
        watch.step(false);
        watch.step(true);
        assert!(!watch.run_due(true, true));
        assert!(watch.run_due(true, false));
    }
}
