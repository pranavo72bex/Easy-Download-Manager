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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_two_commands_share_a_key() {
        let keys: Vec<_> = Command::ALL.iter().flat_map(|c| c.keys()).collect();
        for (i, a) in keys.iter().enumerate() {
            assert!(!keys[i + 1..].contains(a), "{a:?} runs two commands");
        }
    }

    /// Presses a key with its modifiers in a frame of its own, and says
    /// what the window would run.
    fn press(key: egui::KeyboardShortcut) -> Vec<Command> {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: key.logical_key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: key.modifiers,
            }],
            ..Default::default()
        };
        let mut ran = Vec::new();
        let mut out = ctx.run_ui(input, |ui| {
            ran = ui.ctx().input_mut(pressed);
        });
        out.textures_delta.clear();
        ran
    }

    #[test]
    fn each_key_runs_its_own_command_and_no_other() {
        let mut checked = 0;
        for command in Command::ALL {
            for key in command.keys() {
                assert_eq!(press(key), vec![*command], "{key:?}");
                checked += 1;
            }
        }
        assert!(checked >= 12, "only {checked} keys");
        // The one that went wrong: ⌘⇧N is the numbered series, not Add URL.
        let series = egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::N,
        );
        assert_eq!(press(series), vec![Command::AddSeries]);
    }

    #[test]
    fn a_mac_removes_with_backspace_and_others_with_delete() {
        let backspace = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Backspace);
        let delete = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Delete);
        if cfg!(target_os = "macos") {
            assert_eq!(press(backspace), vec![Command::Remove]);
            assert_eq!(
                Command::Remove.shortcut().map(|k| k.logical_key),
                Some(egui::Key::Backspace),
                "the menu shows ⌘⌫"
            );
        } else {
            assert!(press(backspace).is_empty());
        }
        assert_eq!(press(delete), vec![Command::Remove]);
    }
}
