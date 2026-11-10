// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! How a row's numbers are said: `dm_desktop_base::format`'s, re-exported
//! here; and a key shortcut as a menu shows it, from egui's own names for
//! its keys.

pub use dm_desktop_base::format::*;

use crate::i18n::t;

/// A key shortcut as a menu shows it: the modifiers as egui names them
/// (symbols on a Mac), and the key by its symbol where it has one — "." and
/// "," rather than "Period" and "Comma", which no keyboard prints and no
/// translation turns into anything.
pub fn shortcut(ctx: &egui::Context, shortcut: &egui::KeyboardShortcut) -> String {
    let key = shortcut.logical_key;
    let modifiers = ctx.format_modifiers(shortcut.modifiers);
    // A Mac's symbols are every language's; the names are words, and a
    // German keyboard says Strg, Umschalt and Entf.
    let symbols = !modifiers.is_ascii();
    let mut said = if symbols {
        modifiers
    } else {
        let names = egui::ModifierNames {
            alt: key_word(t("key:Alt")),
            ctrl: key_word(t("key:Ctrl")),
            shift: key_word(t("key:Shift")),
            mac_cmd: key_word(t("key:Cmd")),
            mac_alt: key_word(t("key:Option")),
            ..egui::ModifierNames::NAMES
        };
        let mut said = names.format(&shortcut.modifiers, ctx.os().is_mac());
        if !said.is_empty() {
            said.push('+');
        }
        said
    };
    said.push_str(&key_name(key));
    said
}

/// A key's name on its keycap, in the language in force: the named keys a
/// shortcut uses; any other by its symbol (".", ",") or its letter. The
/// arrows by name, their symbols not being in every font.
pub fn key_name(key: egui::Key) -> String {
    use egui::Key;
    let named = match key {
        Key::Delete => t("key:Delete"),
        Key::Backspace => t("key:Backspace"),
        Key::Enter => t("key:Enter"),
        Key::Escape => t("key:Esc"),
        Key::Tab => t("key:Tab"),
        Key::Space => t("key:Space"),
        Key::Insert => t("key:Insert"),
        Key::Home => t("key:Home"),
        Key::End => t("key:End"),
        Key::PageUp => t("key:Page Up"),
        Key::PageDown => t("key:Page Down"),
        Key::ArrowUp => t("key:Up"),
        Key::ArrowDown => t("key:Down"),
        Key::ArrowLeft => t("key:Left"),
        Key::ArrowRight => t("key:Right"),
        _ => return key.symbol_or_name().to_string(),
    };
    key_word(named).to_string()
}

/// A key's word without the `key:` its catalog entry is told apart by:
/// "Delete" the key is "Entf" in German, "Delete" the command "Löschen".
fn key_word(word: &str) -> &str {
    word.strip_prefix("key:").unwrap_or(word)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A key with a symbol is shown by it.
    #[test]
    fn a_shortcut_shows_its_keys_symbol() {
        crate::region::pin(None);
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
        out.textures_delta.clear();
        let ctrl = |key| egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, key);
        let shown = shortcut(&ctx, &ctrl(egui::Key::Period));
        assert!(shown.ends_with('.'), "{shown}");
        assert!(!shown.contains("Period"), "{shown}");
        assert!(shortcut(&ctx, &ctrl(egui::Key::Comma)).ends_with(','));
        assert!(shortcut(&ctx, &ctrl(egui::Key::R)).ends_with('R'));
        let plain = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Delete);
        assert_eq!(shortcut(&ctx, &plain), "Delete");
        assert_eq!(key_name(egui::Key::ArrowUp), "Up");
    }

    /// A German menu says Strg, Umschalt and Entf.
    #[test]
    fn a_shortcut_names_its_keys_in_the_language_in_force() {
        crate::region::pin(None);
        let ctx = egui::Context::default();
        ctx.set_os(egui::os::OperatingSystem::Nix);
        let both = egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::N,
        );
        let delete = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Delete);
        assert_eq!(shortcut(&ctx, &both), "Ctrl+Shift+N");
        crate::i18n::set("de");
        let german = (shortcut(&ctx, &both), shortcut(&ctx, &delete));
        crate::i18n::set("en");
        assert_eq!(german, ("Strg+Umschalt+N".to_string(), "Entf".to_string()));
    }
}
