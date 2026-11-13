// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Keeping the computer awake while a download is running: a 25 GB file on a
//! machine set to sleep after ten minutes would otherwise stop at the first
//! idle spell, every socket dropped. Windows is asked not to sleep for as
//! long as a row is Running, and told it may again when none is. Other
//! systems leave sleep to their own power settings, so there this does
//! nothing.

/// What the queue's busyness last asked of the system, so the system is
/// told only when that changes.
#[derive(Default)]
pub struct Keeper {
    held: bool,
}

impl Keeper {
    /// The new state to set when it differs from the last, else `None`.
    pub fn step(&mut self, busy: bool) -> Option<bool> {
        if busy == self.held {
            return None;
        }
        self.held = busy;
        Some(busy)
    }
}

/// Holds the system awake while `busy` says a download is running. Looked at on a thread of
/// its own each time the queue changes (`changes::subscribe`), not every few seconds (gaps row
/// S18): the setting belongs to the thread that made it and lasts as long as the thread does.
pub fn watch(busy: impl Fn() -> bool + Send + 'static) {
    #[cfg(windows)]
    {
        // One nudge waiting at most: changes that come while the queue is being looked at are
        // one more look, not one each.
        let (nudge, nudged) = std::sync::mpsc::sync_channel::<()>(1);
        dm_shell::app::changes::subscribe(move || {
            let _ = nudge.try_send(());
        });
        let _ = std::thread::Builder::new()
            .name("dm-awake".into())
            .spawn(move || {
                let mut keeper = Keeper::default();
                loop {
                    if let Some(hold) = keeper.step(busy()) {
                        win::hold(hold);
                    }
                    if nudged.recv().is_err() {
                        return;
                    }
                }
            });
    }
    #[cfg(not(windows))]
    let _ = busy;
}

#[cfg(windows)]
mod win {
    use windows_sys::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
    };

    pub(super) fn hold(on: bool) {
        let flags = if on {
            ES_CONTINUOUS | ES_SYSTEM_REQUIRED
        } else {
            ES_CONTINUOUS
        };
        // SAFETY: a plain call with flags; it affects only this thread's request.
        unsafe {
            SetThreadExecutionState(flags);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_is_told_only_when_the_answer_changes() {
        let mut keeper = Keeper::default();
        assert_eq!(keeper.step(false), None);
        assert_eq!(keeper.step(true), Some(true));
        assert_eq!(keeper.step(true), None);
        assert_eq!(keeper.step(false), Some(false));
        assert_eq!(keeper.step(false), None);
    }
}
