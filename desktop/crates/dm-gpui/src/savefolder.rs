// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! "Save to", as "Download links" and "Links on the clipboard" draw it:
//! each file to the folder of its category (as dm sorts a download), every
//! file to one category, or every file to one folder, typed or chosen with
//! Browse… in the system's own folder panel, where a new folder can be made.
//! What the choice means, and making a folder that is not there yet, is
//! `dm_desktop_base::save_to`'s; this is how a dialog draws it and takes its
//! clicks.

use dm_desktop_base::format;
use dm_desktop_base::i18n::t;
use dm_desktop_base::save_to::{SaveChoice, SaveTo};
use gpui::{
    div, prelude::*, App, Context, Div, Entity, PathPromptOptions, SharedString, WeakEntity, Window,
};
use gpui_component::input::{InputEvent, InputState};

use crate::list::Downloads;
use crate::native;
use crate::popup::{self, Pick};
use crate::screen::Screen;
use crate::theme::Theme;
use crate::zoom::px;

/// The pop-up of categories' field, for [`Pick`].
pub const CATEGORY_FIELD: &str = "save-category";

/// A dialog that has a Save to: its choice, and the field the folder is
/// typed in.
pub trait Saves: Sized + 'static {
    fn choice(&mut self) -> &mut SaveChoice;
    fn folder(&self) -> &Entity<InputState>;

    /// The choice with the folder as typed, for adding.
    fn chosen(&mut self, cx: &App) -> SaveChoice {
        let typed = self.folder().read(cx).value().to_string();
        let choice = self.choice();
        choice.folder = typed;
        choice.clone()
    }
}

/// The folder's field. Typing a folder chooses Every file to this folder,
/// as a person typing one means; Return in the field is `enter` (the
/// dialog's Start now).
pub fn field<T: Saves>(
    window: &mut Window,
    cx: &mut Context<T>,
    enter: impl Fn(&mut T, &mut Window, &mut Context<T>) + 'static,
) -> Entity<InputState> {
    let folder = cx.new(|cx| InputState::new(window, cx));
    cx.subscribe_in(
        &folder,
        window,
        move |this: &mut T, input: &Entity<InputState>, event, window, cx| match event {
            InputEvent::PressEnter { .. } => enter(this, window, cx),
            InputEvent::Change => {
                if !input.read(cx).value().trim().is_empty() {
                    this.choice().save_to = SaveTo::OneFolder;
                }
                cx.notify();
            }
            _ => {}
        },
    )
    .detach();
    folder
}

/// Browse…: the system's own folder panel (on macOS an `NSOpenPanel` with
/// New Folder; the desktop's chooser on Linux; on Windows the folder picker,
/// which has New folder), the folder chosen put in the field and Every file
/// to this folder chosen.
pub fn browse<T: Saves>(window: &mut Window, cx: &mut Context<T>) {
    let chosen = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: None,
    });
    cx.spawn_in(window, async move |this, cx| {
        if let Ok(Ok(Some(paths))) = chosen.await {
            if let Some(path) = paths.first() {
                let path = path.display().to_string();
                let _ = this.update_in(cx, |this, window, cx| {
                    this.folder()
                        .clone()
                        .update(cx, |f, cx| f.set_value(path, window, cx));
                    this.choice().save_to = SaveTo::OneFolder;
                    cx.notify();
                });
            }
        }
    })
    .detach();
}

/// A choice from the categories' pop-up, if `pick` is one: Every file to
/// this category, that category.
pub fn pick<T: Saves>(this: &mut T, pick: &Pick, categories: &[String]) -> bool {
    if pick.field != CATEGORY_FIELD {
        return false;
    }
    if let Some(word) = categories.get(pick.index) {
        let choice = this.choice();
        choice.category = word.clone();
        choice.save_to = SaveTo::OneCategory;
    }
    true
}

/// The categories Every file to this category offers, by their words: each
/// kind's, then each a person made.
pub fn categories(cx: &App, view: &WeakEntity<Downloads>) -> Vec<String> {
    let customs: Vec<String> = view
        .upgrade()
        .map(|v| v.read(cx).backend.settings().custom_categories.clone())
        .unwrap_or_default()
        .iter()
        .map(|c| c.name.trim().to_string())
        .filter(|n| !n.is_empty())
        .collect();
    dm_core::Category::ALL
        .iter()
        .map(|k| dm_shell::app::names::category_word(*k))
        .chain(customs)
        .collect()
}

/// A category's word as the window says it.
fn shown_category(word: &str) -> String {
    dm_desktop_base::i18n::name(&format::titled(word))
}

/// The heading and the three choices, for a dialog's body.
pub fn section<T: Saves>(
    this: &mut T,
    categories: &[String],
    theme: Theme,
    screen: &Screen,
    cx: &mut Context<T>,
) -> Div {
    let save_to = this.choice().save_to;
    let category = this.choice().category.clone();
    let folder = this.folder().clone();
    let chosen = categories.iter().position(|w| *w == category);
    let shown: Vec<SharedString> = categories
        .iter()
        .map(|w| shown_category(w).into())
        .collect();
    let radio = |id: &'static str, words: &'static str, how: SaveTo| {
        native::radio(id, t(words).into(), save_to == how, theme, screen)
    };
    let row = || div().flex().items_center().gap(px(native::LABEL_GAP));
    div()
        .flex()
        .flex_col()
        .gap(px(native::ROW_GAP))
        .child(
            div()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(screen.label(t("Save to"))),
        )
        .child(
            radio(
                "by-category",
                "Each file to the folder of its category",
                SaveTo::ByCategory,
            )
            .on_click(cx.listener(|this: &mut T, _, _, cx| {
                this.choice().save_to = SaveTo::ByCategory;
                cx.notify();
            })),
        )
        .child(
            row()
                .child(
                    radio(
                        "one-category",
                        "Every file to this category",
                        SaveTo::OneCategory,
                    )
                    .on_click(cx.listener(|this: &mut T, _, _, cx| {
                        this.choice().save_to = SaveTo::OneCategory;
                        cx.notify();
                    })),
                )
                .child(popup::button(
                    CATEGORY_FIELD,
                    shown_category(&category).into(),
                    shown,
                    chosen,
                    theme,
                    screen,
                )),
        )
        .child(
            row()
                .child(
                    radio("one-folder", "Every file to this folder", SaveTo::OneFolder).on_click(
                        // The field takes the typing at once, as the
                        // choice is to type a folder.
                        cx.listener(|this: &mut T, _, window, cx| {
                            this.choice().save_to = SaveTo::OneFolder;
                            this.folder()
                                .clone()
                                .update(cx, |f, cx| f.focus(window, cx));
                            cx.notify();
                        }),
                    ),
                )
                .child(div().flex_1().child(native::field(
                    &folder,
                    "Every file to this folder",
                    theme,
                )))
                .child(
                    native::button("browse", t("Browse…").into(), false, theme, screen)
                        .on_click(cx.listener(|_: &mut T, _, window, cx| browse(window, cx))),
                ),
        )
}
