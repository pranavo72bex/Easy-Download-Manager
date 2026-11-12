// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Every action dm's window can take, by name.
//!
//! The registry itself is the application's (`dm_shell::app::commands`):
//! the menu bar, the toolbar, the row menu, the keyboard and the control
//! protocol all run a `Command`, so a command that exists is reachable from
//! all of them. What is the window's is here: a command's words in the
//! language in force, its keys as egui shortcuts, and which were pressed.

pub use dm_shell::app::commands::{Chord, Command, Key, SPEED_HIGH, SPEED_LOW, SPEED_MEDIUM};

/// What the window shows of a command: its words and its keys.
pub trait Shown {
    /// What a person reads on the menu item or button, in the language in
    /// force.
    fn title(self) -> &'static str;
    /// The key that runs it, said the way the menu shows it.
    fn shortcut(self) -> Option<egui::KeyboardShortcut>;
    /// Every key that runs it, the one the menu shows first.
    fn keys(self) -> Vec<egui::KeyboardShortcut>;
}

impl Shown for Command {
    fn title(self) -> &'static str {
        crate::i18n::t(self.label())
    }

    fn shortcut(self) -> Option<egui::KeyboardShortcut> {
        self.keys().first().copied()
    }

    fn keys(self) -> Vec<egui::KeyboardShortcut> {
        self.chords().into_iter().map(shortcut_of).collect()
    }
}

/// `chord` as egui's shortcut: `command` is ⌘ on a Mac and Ctrl elsewhere.
pub fn shortcut_of(chord: Chord) -> egui::KeyboardShortcut {
    use egui::Modifiers;
    let mut modifiers = Modifiers::NONE;
    if chord.command {
        modifiers |= Modifiers::COMMAND;
    }
    if chord.shift {
        modifiers |= Modifiers::SHIFT;
    }
    let key = match chord.key {
        Key::A => egui::Key::A,
        Key::F => egui::Key::F,
        Key::I => egui::Key::I,
        Key::N => egui::Key::N,
        Key::Q => egui::Key::Q,
        Key::R => egui::Key::R,
        Key::Comma => egui::Key::Comma,
        Key::Period => egui::Key::Period,
        Key::F1 => egui::Key::F1,
        Key::F3 => egui::Key::F3,
        Key::Backspace => egui::Key::Backspace,
        Key::Delete => egui::Key::Delete,
    };
    egui::KeyboardShortcut::new(modifiers, key)
}

/// The commands whose keys were pressed, each key taken by one command.
///
/// egui matches a shortcut ignoring an extra Shift, so ⌘⇧N matched ⌘N first
/// and opened Add URL rather than the numbered series: the keys with more
/// modifiers are tried first, and the one that matches takes the key press.
pub fn pressed(input: &mut egui::InputState) -> Vec<Command> {
    let mut chords: Vec<(Command, Chord)> = Command::ALL
        .iter()
        .flat_map(|c| c.chords().into_iter().map(move |k| (*c, k)))
        .collect();
    chords.sort_by_key(|(_, k)| std::cmp::Reverse(k.held()));
    let mut ran = Vec::new();
    for (command, chord) in chords {
        if input.consume_shortcut(&shortcut_of(chord)) && !ran.contains(&command) {
            ran.push(command);
        }
    }
    ran
}
