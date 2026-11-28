// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A dialog's keys where one of its controls, not its text field, has them (gaps row E27).

use crate::harness::settings_file;
use crate::harness::world::world;

/// Return presses the dialog's default only while its text field or the dialog itself has
/// the keys: with Tab moved on to another of its controls, Return is that control's, and the
/// address typed is not added (`native::return_is_default`).
#[test]
fn return_on_another_control_does_not_press_the_default() {
    // Add URL saves its addresses.
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.key("n", &["cmd"]);
    h.until("Add URL", |h| h.dialog_open("Add URL"));
    h.type_text(&world.url("file.zip"));
    h.key("tab", &[]);
    h.key("enter", &[]);
    assert!(
        !h.dialog_open("Download details"),
        "Return on another control pressed OK"
    );
    assert!(h.rows().is_empty(), "the address was added");
    // Back in the field, Return is OK again.
    h.key("tab", &["shift"]);
    h.key("enter", &[]);
    h.until("Download details", |h| h.dialog_open("Download details"));
}

/// Closing a place's menu (Escape here) gives the keys back to what had them as it opened, the
/// search box here, rather than leaving a keyboard user nowhere (gaps row E45).
#[test]
fn the_place_menu_gives_the_keys_back_as_it_closes() {
    let world = world();
    let mut h = world.harness();
    let search_has_keys = |h: &mut crate::harness::Harness| {
        h.dialog(|view: &mut crate::window::list::Downloads, window, cx| {
            gpui::Focusable::focus_handle(view.list.filter.read(cx), cx).is_focused(window)
        })
    };
    h.key("f", &["cmd"]);
    h.until("the search box has the keys", search_has_keys);
    h.right_click("Main queue");
    h.until("the place menu", |h| {
        h.view(|view| view.menus.place_menu.is_some())
    });
    h.key("escape", &[]);
    h.until("the menu closed", |h| {
        h.view(|view| view.menus.place_menu.is_none())
    });
    assert!(search_has_keys(&mut h), "the keys were not given back");
}

/// A place's menu is used from the keyboard: its first item has the keys as it opens, the
/// arrows and Tab move among its items and stay in it, round from the last to the first, and
/// Return picks one (gaps row E50).
#[test]
fn the_place_menu_is_used_from_the_keyboard() {
    let world = world();
    let mut h = world.harness();
    h.right_click("Main queue");
    h.until("the place menu", |h| {
        h.view(|view| view.menus.place_menu.is_some())
    });
    let has_keys = |h: &mut crate::harness::Harness| {
        h.dialog(|view: &mut crate::window::list::Downloads, window, _| {
            view.menus
                .place_menu_keys
                .iter()
                .position(|k| k.is_focused(window))
        })
    };
    let count = h.view(|view| view.menus.place_menu_keys.len());
    assert!(count >= 2, "{count} items");
    assert_eq!(has_keys(&mut h), Some(0), "the first item has no keys");
    h.key("down", &[]);
    assert_eq!(has_keys(&mut h), Some(1));
    h.key("up", &[]);
    h.key("up", &[]);
    assert_eq!(
        has_keys(&mut h),
        Some(count - 1),
        "up from the first goes round"
    );
    h.key("tab", &[]);
    assert_eq!(has_keys(&mut h), Some(0), "Tab left the menu");
    h.key("enter", &[]);
    h.until("the menu closed by its item", |h| {
        h.view(|view| view.menus.place_menu.is_none())
    });
}
