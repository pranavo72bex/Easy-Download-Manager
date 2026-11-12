// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What dm does when the queue runs dry (Downloads › When done), as the
//! application decides it: the choice and its words. Carrying it out on this
//! computer (the system's sleep, shut down and hibernate) is the desktop's
//! (`dm-desktop` `power.rs`), which a presentation of its own implements.

/// The action taken when the last download settles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhenDone {
    /// Do nothing.
    #[default]
    Nothing,
    /// Close dm.
    Quit,
    /// Put the computer to sleep.
    Sleep,
    /// To disk and off: Linux and Windows have it; macOS does not offer it
    /// as a command, so the menu greys it there.
    Hibernate,
    /// Shut the computer down.
    ShutDown,
}

/// How long a sleep or shut down waits for someone to cancel it.
pub const COUNTDOWN_SECS: u64 = 30;

impl WhenDone {
    /// Every choice, in the order the menus list them.
    pub const ALL: [WhenDone; 5] = [
        WhenDone::Nothing,
        WhenDone::Quit,
        WhenDone::Sleep,
        WhenDone::Hibernate,
        WhenDone::ShutDown,
    ];

    /// What the countdown window says is about to happen.
    pub fn verb(self) -> &'static str {
        match self {
            WhenDone::Nothing => "do nothing",
            WhenDone::Quit => "quit",
            WhenDone::Sleep => "put the computer to sleep",
            WhenDone::Hibernate => "hibernate the computer",
            WhenDone::ShutDown => "shut the computer down",
        }
    }

    /// The word a schedule keeps it as (`Schedule::when_done`) and the
    /// command line and the control port name it by.
    pub fn word(self) -> &'static str {
        match self {
            WhenDone::Nothing => "nothing",
            WhenDone::Quit => "exit",
            WhenDone::Sleep => "sleep",
            WhenDone::Hibernate => "hibernate",
            WhenDone::ShutDown => "shutdown",
        }
    }

    /// The action a kept word names; anything else is nothing.
    pub fn from_word(word: &str) -> WhenDone {
        WhenDone::ALL
            .into_iter()
            .find(|w| w.word() == word.trim())
            .unwrap_or_default()
    }

    /// Whether it can be chosen on this computer: doing nothing and closing
    /// dm always; the rest where the system has the command (`available`,
    /// the desktop's: macOS does not hibernate). The menu and the Scheduler
    /// grey the rest, and an order from the command line is refused.
    pub fn possible(self, available: impl FnOnce(WhenDone) -> bool) -> bool {
        matches!(self, WhenDone::Nothing | WhenDone::Quit) || available(self)
    }
}

/// Where a refusal that comes back later is told: the window's list of
/// problems.
pub type Report = Box<dyn FnOnce(String) + Send>;

#[cfg(test)]
mod tests {
    use super::*;

    /// The word a schedule keeps and the command line and control port name
    /// an action by is read back as that action; anything else is nothing.
    #[test]
    fn each_action_is_read_back_from_its_word() {
        for action in WhenDone::ALL {
            assert_eq!(WhenDone::from_word(action.word()), action);
            assert_eq!(WhenDone::from_word(&format!(" {} ", action.word())), action);
            assert!(!action.verb().is_empty());
        }
        assert_eq!(WhenDone::from_word("reboot"), WhenDone::Nothing);
        assert_eq!(WhenDone::default(), WhenDone::Nothing);
    }

    #[test]
    fn doing_nothing_and_closing_are_always_possible_the_rest_where_the_system_can() {
        for w in WhenDone::ALL {
            let always = matches!(w, WhenDone::Nothing | WhenDone::Quit);
            assert_eq!(w.possible(|_| false), always, "{w:?}");
            assert!(w.possible(|_| true), "{w:?}");
        }
    }
}
