// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A place's right-click menu in the side panel (gaps row U12): a category's Open folder and
//! Add or edit categories…, Unfinished's Resume all and Stop All, Finished's Delete Completed,
//! a queue's Start Queue, Stop Queue and Scheduler…. Its items are the egui window's
//! (`dm_shell::app::places::place_menu`), drawn as the row menu is (`rowmenu::panel`).

use dm_desktop_base::i18n::t;
use dm_shell::app::places::{folder_of, place_menu, Place, PlaceAction};
use gpui::{prelude::*, Context, Pixels, Point};

use crate::list::Downloads;
use crate::menus::Run;
use crate::screen::Screen;
use crate::theme::Theme;

impl Downloads {
    /// Opens `place`'s menu at `at`, the place chosen first, as a right-click chooses it; the
    /// row menu closed.
    pub fn open_place_menu(&mut self, place: Place, at: Point<Pixels>) {
        self.close_menu();
        self.choose(place.clone());
        self.place_menu = Some((place, at));
    }

    /// Closes the place menu.
    pub fn close_place_menu(&mut self) {
        self.place_menu = None;
    }

    /// Carries out what the place menu asked for.
    fn act_on_place(
        &mut self,
        place: Place,
        action: PlaceAction,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.close_place_menu();
        match action {
            PlaceAction::Run(command) => window.dispatch_action(Box::new(Run(command)), cx),
            PlaceAction::EditCategories => {
                self.open_options_at(dm_desktop_base::options::Tab::SaveTo, cx);
            }
            PlaceAction::OpenFolder => {
                let settings = self.backend.settings();
                let folder = folder_of(&place, &settings.download_dir, &settings.categories());
                // Made where it is not there yet: a category nothing has gone to has no folder.
                let _ = std::fs::create_dir_all(&folder);
                self.open_path(&folder.to_string_lossy());
            }
        }
        cx.notify();
    }
}

/// The place menu, open at `at`.
pub fn menu(
    place: &Place,
    at: Point<Pixels>,
    theme: Theme,
    screen: &Screen,
    cx: &mut Context<Downloads>,
) -> impl IntoElement {
    let items = place_menu(place)
        .into_iter()
        .enumerate()
        .map(|(n, (words, action))| {
            let place = place.clone();
            crate::rowmenu::item(("place-menu", n), t(words), true, false, theme)
                .on_click(cx.listener(move |view, _, window, cx| {
                    view.act_on_place(place.clone(), action, window, cx);
                }))
                .child(screen.label(t(words)))
                .into_any_element()
        })
        .collect();
    crate::rowmenu::panel(
        "place-menu",
        "Place menu",
        at,
        theme,
        items,
        cx,
        Downloads::close_place_menu,
    )
}
