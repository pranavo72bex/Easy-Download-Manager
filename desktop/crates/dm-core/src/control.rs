//! Stopping a download that is already running.
//!
//! Pausing is not a special mode the engine has to hold open — it is stopping,
//! and resuming is starting again. What each range has fetched is recorded
//! on disk as it arrives, so a paused download costs nothing to hold and a
//! resumed one asks again only for what it does not have. That is why
//! there is no state here beyond a single flag: everything else is already
//! on disk.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

const RUN: u8 = 0;
const PAUSE: u8 = 1;
const CANCEL: u8 = 2;

/// What a running download has been asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wanted {
    Run,
    Pause,
    Cancel,
}

/// A handle for stopping one download, shared with whoever is running it.
///
/// Cheap to clone and safe to hold after the download has finished — asking a
/// download that has already ended to pause is a no-op rather than an error,
/// because the alternative is every caller racing to check first.
#[derive(Debug, Clone, Default)]
pub struct Control(Arc<AtomicU8>);

impl Control {
    pub fn new() -> Self {
        Control::default()
    }

    pub fn pause(&self) {
        // Never over a cancellation: a download being torn down should not be
        // talked back into merely pausing.
        let _ = self
            .0
            .compare_exchange(RUN, PAUSE, Ordering::SeqCst, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        let _ = self
            .0
            .compare_exchange(PAUSE, RUN, Ordering::SeqCst, Ordering::SeqCst);
    }

    /// Cancelling is final. A cancelled download is not resumed, it is added
    /// again — which is a different thing, with a different id.
    pub fn cancel(&self) {
        self.0.store(CANCEL, Ordering::SeqCst);
    }

    /// Whether two handles speak for the same download run.
    ///
    /// A row that was stopped and put back in line gets a fresh control;
    /// the task that was stopped still holds the old one, and what it has
    /// to say when it finally returns is about a run the row has moved on
    /// from.
    pub fn is(&self, other: &Control) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub fn wanted(&self) -> Wanted {
        match self.0.load(Ordering::Relaxed) {
            PAUSE => Wanted::Pause,
            CANCEL => Wanted::Cancel,
            _ => Wanted::Run,
        }
    }

    /// Stops here if stopping is what was asked for.
    ///
    /// The three-armed match this replaces was written out at every point a
    /// download can be interrupted — the chunk loop, the segment loop, the
    /// playlist loop — so a new reason to stop had to be found by grep.
    pub fn check(&self) -> Result<(), crate::Error> {
        match self.wanted() {
            Wanted::Run => Ok(()),
            Wanted::Pause => Err(crate::Error::Paused),
            Wanted::Cancel => Err(crate::Error::Cancelled),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_control_is_running() {
        assert_eq!(Control::new().wanted(), Wanted::Run);
    }

    #[test]
    fn pausing_and_resuming_go_back_and_forth() {
        let c = Control::new();
        c.pause();
        assert_eq!(c.wanted(), Wanted::Pause);
        c.resume();
        assert_eq!(c.wanted(), Wanted::Run);
    }

    #[test]
    fn cancelling_is_final() {
        // A download being torn down must not be talked back into running by
        // a resume that was already in flight when it was cancelled.
        let c = Control::new();
        c.cancel();
        c.resume();
        c.pause();
        assert_eq!(c.wanted(), Wanted::Cancel);
    }

    #[test]
    fn a_clone_speaks_for_the_same_download() {
        let c = Control::new();
        let held_elsewhere = c.clone();
        held_elsewhere.pause();
        assert_eq!(c.wanted(), Wanted::Pause);
    }

    #[test]
    fn resuming_something_that_was_never_paused_changes_nothing() {
        let c = Control::new();
        c.resume();
        assert_eq!(c.wanted(), Wanted::Run);
    }
}
