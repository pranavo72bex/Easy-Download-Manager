//! Keeping the computer awake while a download is running: a 25 GB file on a
//! machine set to sleep after ten minutes would otherwise stop at the first
//! idle spell, every socket dropped. Windows is asked not to sleep for as
//! long as a row is Running, and told it may again when none is. Other
//! systems leave sleep to their own power settings, so there this does
//! nothing.

use std::time::Duration;

/// How often the queue is looked at.
const EVERY: Duration = Duration::from_secs(5);

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

/// Holds the system awake while `busy` says a download is running. Looked at
/// every few seconds on a thread of its own: the setting belongs to the
/// thread that made it and lasts as long as the thread does.
pub fn watch(busy: impl Fn() -> bool + Send + 'static) {
    #[cfg(windows)]
    {
        let _ = std::thread::Builder::new()
            .name("dm-awake".into())
            .spawn(move || {
                let mut keeper = Keeper::default();
                loop {
                    if let Some(hold) = keeper.step(busy()) {
                        win::hold(hold);
                    }
                    std::thread::sleep(EVERY);
                }
            });
    }
    #[cfg(not(windows))]
    let _ = (busy, EVERY);
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
