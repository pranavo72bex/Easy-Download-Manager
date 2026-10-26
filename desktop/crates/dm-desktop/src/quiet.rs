//! Not while the person is busy with something else: a fullscreen app or
//! game, a presentation, or Focus Assist. What dm would say then is held, and
//! said once the computer accepts notifications again; a sound is not held,
//! since a chime that comes late is only noise.
//!
//! Only Windows says whether the person is busy (`SHQueryUserNotificationState`);
//! elsewhere nothing is held. The rule and the holding are plain code that
//! the tests run on any system; the one system call is [`quiet_now`].

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Whether a `QUERY_USER_NOTIFICATION_STATE` is one in which notifications
/// are not wanted: a full-screen or busy application (2), a Direct3D game
/// (3), a presentation (4), or Focus Assist's quiet time (6). Not
/// "not present" (1: a screen saver or a locked computer, where a toast
/// waits in the Action Center), "accepts notifications" (5) or a Store app
/// (7).
pub fn is_quiet_state(code: i32) -> bool {
    matches!(code, 2..=4 | 6)
}

/// Whether the computer is in such a state now.
#[cfg(windows)]
pub fn quiet_now() -> bool {
    let mut state = 0;
    // SAFETY: a pointer to a local the call fills in; the call has no other
    // input.
    let result = unsafe { windows_sys::Win32::UI::Shell::SHQueryUserNotificationState(&mut state) };
    result >= 0 && is_quiet_state(state)
}

/// Nothing says so here, so nothing is held.
#[cfg(not(windows))]
pub fn quiet_now() -> bool {
    false
}

/// The most that is held: a long fullscreen game with many downloads ending
/// would otherwise bring a hundred toasts at once when it closes.
const MOST_HELD: usize = 12;

/// How often the state is asked again while something is held.
pub const ASK_EVERY: Duration = Duration::from_secs(2);

/// Things held until the computer accepts them.
pub struct Hold<T> {
    items: Vec<T>,
    asked: Option<Instant>,
}

impl<T> Default for Hold<T> {
    fn default() -> Self {
        Hold {
            items: Vec::new(),
            asked: None,
        }
    }
}

impl<T> Hold<T> {
    /// What to do with `item` now: `Some` to say it, `None` when it is held.
    /// Behind an item already held it is held too, so they come out in order.
    pub fn offer(&mut self, item: T, quiet: bool) -> Option<T> {
        if !quiet && self.items.is_empty() {
            return Some(item);
        }
        if self.items.len() == MOST_HELD {
            self.items.remove(0);
        }
        self.items.push(item);
        None
    }

    /// What is held, once it is no longer quiet; nothing before.
    pub fn release(&mut self, quiet: bool) -> Vec<T> {
        if quiet {
            return Vec::new();
        }
        std::mem::take(&mut self.items)
    }

    /// Whether anything is held.
    pub fn is_holding(&self) -> bool {
        !self.items.is_empty()
    }

    /// Whether it is time to ask the system again, and if so, noted: not on
    /// every frame, and not at all while nothing is held.
    pub fn due(&mut self, now: Instant) -> bool {
        if self.items.is_empty() {
            return false;
        }
        if self
            .asked
            .is_some_and(|at| now.duration_since(at) < ASK_EVERY)
        {
            return false;
        }
        self.asked = Some(now);
        true
    }
}

/// A toast dm wanted to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub title: String,
    pub body: String,
    /// The finished file a click opens, when there is one.
    pub path: Option<String>,
}

static HELD: Mutex<Hold<Note>> = Mutex::new(Hold {
    items: Vec::new(),
    asked: None,
});

/// Whether `note` is to be said now (`Some`), or was held (`None`).
pub fn offer(note: Note) -> Option<Note> {
    let quiet = quiet_now();
    HELD.lock().ok()?.offer(note, quiet)
}

/// The held toasts that may be said now, when it is time to look.
pub fn due_notes() -> Vec<Note> {
    let Ok(mut held) = HELD.lock() else {
        return Vec::new();
    };
    if !held.due(Instant::now()) {
        return Vec::new();
    }
    held.release(quiet_now())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_busy_computer_is_quiet() {
        for code in [2, 3, 4, 6] {
            assert!(is_quiet_state(code), "{code}");
        }
        for code in [0, 1, 5, 7, 8] {
            assert!(!is_quiet_state(code), "{code}");
        }
    }

    #[test]
    fn what_is_said_while_quiet_is_held_and_comes_out_in_order() {
        let mut hold = Hold::default();
        assert_eq!(hold.offer("one", false), Some("one"), "said at once");
        assert_eq!(hold.offer("two", true), None);
        assert_eq!(hold.offer("three", true), None);
        // Still quiet: nothing out.
        assert!(hold.release(true).is_empty());
        // Not quiet any more, but something is held: a new one waits its
        // turn, so the order is kept.
        assert_eq!(hold.offer("four", false), None);
        assert_eq!(hold.release(false), ["two", "three", "four"]);
        assert!(!hold.is_holding());
    }

    #[test]
    fn not_more_than_a_dozen_are_held() {
        let mut hold = Hold::default();
        for n in 0..40 {
            hold.offer(n, true);
        }
        let out = hold.release(false);
        assert_eq!(out.len(), MOST_HELD);
        assert_eq!(out.last(), Some(&39), "the newest are kept");
    }

    #[test]
    fn the_system_is_asked_now_and_then_and_only_while_something_is_held() {
        let mut hold: Hold<u8> = Hold::default();
        let start = Instant::now();
        assert!(!hold.due(start), "nothing held");
        hold.offer(1, true);
        assert!(hold.due(start));
        assert!(!hold.due(start + Duration::from_millis(500)), "too soon");
        assert!(hold.due(start + ASK_EVERY));
    }
}
