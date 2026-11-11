// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The View menu's choices kept in the settings, as the egui window's View
//! menu has them: Columns (each shown or hidden; a heading dragged in the
//! table moves it), Customize toolbar (the buttons' style, each button
//! shown or hidden), Text size and Language. Each is an item of the system's
//! menu bar, ticked as it stands (`menus::Ticks`).

use dm_desktop_base::columns::Column;
use dm_shell::settings::TEXT_SIZE;
use gpui::{Action, Context};

use crate::list::Downloads;

/// View › Columns: a column shown or hidden, by its key.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = dm, no_json)]
pub struct ToggleColumn {
    /// The column's key (`Column::key`).
    pub key: String,
}

/// View › Customize toolbar: how the buttons are drawn, by the style's name
/// (`ToolStyle::name`).
#[derive(Clone, PartialEq, Action)]
#[action(namespace = dm, no_json)]
pub struct ToolbarStyle {
    /// `both`, `icons` or `text`.
    pub style: String,
}

/// View › Customize toolbar: a button shown or hidden, by its command's id.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = dm, no_json)]
pub struct ToggleToolbarButton {
    /// The command's id (`Command::id`).
    pub id: String,
}

/// View › Language: the language dm speaks, by its code.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = dm, no_json)]
pub struct SetLanguage {
    /// As `i18n::LANGUAGES` spells it.
    pub code: String,
}

/// View › Text size › Larger (Ctrl or ⌘ with +): everything a stop
/// larger (60, 80, 100, 120%), in every window.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = dm, no_json)]
pub struct TextLarger;

/// View › Text size › Smaller (Ctrl or ⌘ with −).
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = dm, no_json)]
pub struct TextSmaller;

/// View › Text size › Normal size (Ctrl or ⌘ with 0): 100%.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = dm, no_json)]
pub struct TextNormal;

impl Downloads {
    /// The window's scale, held to its nearest stop: every window drawn at
    /// it at once, and kept for the next start.
    pub(crate) fn set_text_size(&mut self, size: f32, cx: &mut Context<Self>) {
        let mut wanted = self.backend.settings();
        wanted.text_size = TEXT_SIZE.held(size);
        if (wanted.text_size - crate::zoom::factor()).abs() < 0.001 {
            return;
        }
        self.keep_view(wanted, cx);
    }

    /// A stop larger (`Some(true)`), smaller, or back to 100% (`None`).
    pub(crate) fn step_text_size(&mut self, up: Option<bool>, cx: &mut Context<Self>) {
        let now = crate::zoom::factor();
        let size = up.map_or(TEXT_SIZE.default, |up| TEXT_SIZE.stepped(now, up));
        self.set_text_size(size, cx);
    }

    /// A column shown or hidden; the file name always stays.
    pub(crate) fn toggle_column(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(column) = Column::from_key(key).filter(|c| *c != Column::Name) else {
            return;
        };
        let mut wanted = self.backend.settings();
        let key = column.key().to_string();
        if wanted.columns_hidden.contains(&key) {
            wanted.columns_hidden.retain(|h| *h != key);
        } else {
            wanted.columns_hidden.push(key);
            wanted.columns_hidden.sort();
        }
        self.keep_view(wanted, cx);
    }

    /// The toolbar's style.
    pub(crate) fn set_toolbar_style(&mut self, style: &str, cx: &mut Context<Self>) {
        let mut wanted = self.backend.settings();
        wanted.toolbar_style = dm_desktop_base::toolbar::ToolStyle::named(style)
            .name()
            .to_string();
        self.keep_view(wanted, cx);
    }

    /// A toolbar button shown or hidden.
    pub(crate) fn toggle_toolbar_button(&mut self, id: &str, cx: &mut Context<Self>) {
        if !dm_shell::app::commands::TOOLBAR
            .iter()
            .any(|(c, _)| c.id() == id)
        {
            return;
        }
        let mut wanted = self.backend.settings();
        if wanted.toolbar_hidden.iter().any(|h| h == id) {
            wanted.toolbar_hidden.retain(|h| h != id);
        } else {
            wanted.toolbar_hidden.push(id.to_string());
        }
        self.keep_view(wanted, cx);
    }

    /// The language dm speaks: the menus and every window in it at once.
    pub(crate) fn set_language(&mut self, code: &str, cx: &mut Context<Self>) {
        if !dm_desktop_base::i18n::LANGUAGES
            .iter()
            .any(|(c, _)| *c == code)
        {
            return;
        }
        let mut wanted = self.backend.settings();
        wanted.language = code.to_string();
        self.keep_view(wanted, cx);
    }

    /// The settings a View menu choice asked for, put in force and kept;
    /// what could not be is said.
    fn keep_view(&mut self, wanted: dm_shell::Settings, cx: &mut Context<Self>) {
        if let Err(e) = self.apply_settings(wanted, cx) {
            self.say(e);
        }
        cx.notify();
    }
}
