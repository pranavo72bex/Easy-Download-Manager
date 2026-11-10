// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window's words in the language a person picked: the catalogs and
//! the rules are `dm_desktop_base::i18n`'s, re-exported here; the list of
//! languages to pick from is drawn here.

pub use dm_desktop_base::i18n::*;

/// The languages to choose from, the one in force marked; the one picked,
/// if any. View › Language and Options › General both offer it.
pub fn pick(ui: &mut egui::Ui, now: &str) -> Option<&'static str> {
    // Each language's name in its own script: 日本語 and 한국어 need the
    // computer's fonts for them, found the first time the list is shown.
    crate::theme::cover(ui.ctx(), LANGUAGES.iter().map(|(_, name)| *name));
    let mut picked = None;
    for (code, name) in LANGUAGES {
        if crate::dialogs::radio_flag(ui, now == code, name.to_string()).clicked() {
            picked = Some(code);
        }
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picker_offers_every_language_and_picks_none_unclicked() {
        let ctx = egui::Context::default();
        let mut picked = Some("unset");
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            picked = pick(ui, "de");
        });
        output.textures_delta.clear();
        assert_eq!(picked, None);
    }
}
