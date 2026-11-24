// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! On a phone, Options in force as it changes (gaps row C21); and the guide's page for each of
//! Options' pages (gaps row E11).

use dm_desktop_base::options::{self, Tab};
use gpui::Context;

use super::Options;

impl Options {
    /// On a phone, what the draft says put in force now, where it passes OK's checks and
    /// differs from what was last put in force; a change that does not pass yet (a number half
    /// typed, a proxy with no address) waits, and Done says why (gaps row C21).
    pub(crate) fn apply_now(&mut self, cx: &mut Context<Self>) {
        if !self.instant || self.done {
            return;
        }
        let Some(view) = self.view.upgrade() else {
            return;
        };
        let now = view.read(cx).backend.settings();
        let Ok(wanted) = options::ok(&self.opened, &self.draft, &self.typed, &now) else {
            return;
        };
        let text = serde_json::to_string(&wanted).ok();
        // The first look is of the settings as Options opened, in force already: kept as
        // what is in force, nothing put in force again.
        let first = self.applied.is_none();
        if text == self.applied {
            return;
        }
        self.applied = text;
        if first {
            return;
        }
        match view.update(cx, |view, cx| view.apply_settings(wanted, cx)) {
            Ok(()) => self.says = None,
            Err(e) => self.says = Some(e.into()),
        }
    }
}

/// The guide's page for Options' page `tab` (gaps row E11).
pub(crate) fn help_page(tab: Tab) -> &'static str {
    match tab {
        Tab::General => "options-general",
        Tab::FileTypes => "options-browser",
        Tab::SaveTo => "options-folders",
        Tab::Downloads => "options-downloads",
        Tab::Advanced => "options-advanced",
        _ => "options",
    }
}
