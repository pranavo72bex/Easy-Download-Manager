// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window at a phone's width (gaps row N7): the top bar, the cards, the overflow menu
//! and the drawer, each the desktop's own views and commands; and the desktop's layout kept
//! at every width a desktop window had.

use dm_core::State;
use dm_shell::app::places::Place;

use crate::harness::world::{world, World};
use crate::harness::{settings_file, Harness};

/// A phone held upright, in points.
const PHONE: (f32, f32) = (412.0, 915.0);

/// `names` added for later, fetching nothing, and listed.
fn add_later(h: &mut Harness, world: &World, names: &[&str]) {
    let urls: Vec<String> = names.iter().map(|n| world.url(n)).collect();
    h.call(
        "downloads.add",
        serde_json::json!({ "urls": urls, "later": true, "again": true }),
    )
    .unwrap();
    h.until("the rows", |h| h.rows().len() == names.len());
}

/// The window at a phone's width.
fn phone(world: &World) -> Harness {
    let mut h = world.harness();
    h.resize(PHONE.0, PHONE.1);
    h
}

/// At a phone's width the menu bar and the toolbar are one top bar, and each row is a card
/// with its name, size and state; a tap chooses a card, its ⋯ opens the row menu as a sheet.
#[test]
fn at_a_phones_width_the_rows_are_cards() {
    let _file = settings_file();
    let world = world();
    let mut h = phone(&world);
    add_later(&mut h, &world, &["file.zip", "page.html"]);
    h.until_shown("file.zip");
    assert!(h.shows("page.html"), "{:?}", h.texts());
    // The top bar: no menus' titles, no toolbar; search and the menu's button; the bottom
    // navigation's Places; Add URL floating over the cards.
    assert!(!h.shows("Tasks"), "{:?}", h.texts());
    assert!(!h.shows("Stop All"), "{:?}", h.texts());
    for button in ["Places", "Add URL", "Menu", "Filter"] {
        assert!(h.shows(button), "{button}: {:?}", h.texts());
    }
    // Each card says its state, as the table's Status does.
    let row = h.row("file.zip");
    let state = dm_desktop_base::rows::status_of(&row);
    assert!(h.shows(&state), "{state}: {:?}", h.texts());
    // A tap chooses the card alone, and opens it as a double-click opens its row.
    h.person_click("file.zip", 1);
    h.until("the card chosen", |h| {
        h.view(|view| view.list.selected == [row.id].into())
    });
    // Its ⋯: the row menu, as a sheet, its items the row menu's.
    h.person_click("Row menu", 1);
    h.until("the row menu's sheet", |h| {
        h.view(|view| view.layout.narrow.sheet.is_some())
    });
    let menu = h.view(|view| crate::window::rowmenu::row_menu(view, false).owned());
    let first = menu
        .items
        .iter()
        .find_map(|item| match item {
            gpui::OwnedMenuItem::Action { name, .. } => Some(name.clone()),
            _ => None,
        })
        .unwrap();
    assert!(h.shows(&first), "{first}: {:?}", h.texts());
}

/// The overflow menu holds the toolbar's commands and the menus, and runs them as they run:
/// Stop All from the toolbar's part, Downloads › Resume all from the menus'.
#[test]
fn the_overflow_menu_runs_the_toolbars_and_the_menus_commands() {
    let _file = settings_file();
    let world = world();
    let mut h = phone(&world);
    h.call(
        "downloads.add",
        serde_json::json!({ "urls": [world.url("stalled.bin")] }),
    )
    .unwrap();
    h.until("the row to start", |h| {
        h.rows().iter().any(|r| r.state == State::Running)
    });
    h.until("Stop All to be offered", |h| {
        h.enabled(dm_shell::app::commands::Command::StopAll)
    });
    h.person_click("Menu", 1);
    h.until_shown("Downloads");
    for menu in ["Tasks", "File", "View", "Help"] {
        assert!(h.shows(menu), "{menu}: {:?}", h.texts());
    }
    h.person_click("Stop All", 1);
    h.until("Stop All", |h| {
        h.rows().iter().all(|r| r.state == State::Paused)
    });
    assert!(
        h.view(|view| view.layout.narrow.sheet.is_none()),
        "the sheet stayed open"
    );
    h.person_click("Menu", 1);
    h.until_shown("Downloads");
    h.person_click("Downloads", 1);
    h.until_shown("Resume all");
    h.person_click("Resume all", 1);
    h.until("Resume all", |h| {
        h.rows().iter().all(|r| r.state != State::Paused)
    });
}

/// The side panel is a drawer from the bottom navigation's Places: its categories and queues,
/// and a place chosen in it shows its rows and closes it.
#[test]
fn the_drawer_shows_the_categories_and_queues() {
    let _file = settings_file();
    let world = world();
    let mut h = phone(&world);
    h.until_shown("Places");
    assert!(!h.shows("Main queue"), "the side panel beside the cards");
    h.person_click("Places", 1);
    h.until_shown("Main queue");
    assert!(h.shows("Videos"), "{:?}", h.texts());
    h.person_click("Main queue", 1);
    h.until("the queue chosen and the drawer closed", |h| {
        h.view(|view| {
            view.list.place == Place::Queue("Main queue".into())
                && view.layout.narrow.drawer.is_none()
        })
    });
    assert!(!h.shows("Videos"), "{:?}", h.texts());
}

/// Escape (a phone's Back, gaps row N16) closes what the phone's layout holds over the
/// cards: a submenu walked into goes back to the overflow menu, then the sheet closes, then
/// the drawer; with nothing open the cards stay as they are.
#[test]
fn escape_closes_the_sheet_and_the_drawer_one_step_at_a_time() {
    let _file = settings_file();
    let world = world();
    let mut h = phone(&world);
    h.until_shown("Menu");
    h.person_click("Menu", 1);
    h.until_shown("Downloads");
    h.person_click("Downloads", 1);
    h.until_shown("Resume all");
    h.key("escape", &[]);
    h.until("back to the overflow menu", |h| {
        h.view(|view| view.layout.narrow.sheet.is_some()) && h.shows("Tasks")
    });
    h.key("escape", &[]);
    h.until("the sheet closed", |h| {
        h.view(|view| view.layout.narrow.sheet.is_none())
    });
    h.person_click("Places", 1);
    h.until_shown("Main queue");
    h.key("escape", &[]);
    h.until("the drawer closed", |h| {
        h.view(|view| view.layout.narrow.drawer.is_none())
    });
    assert!(!h.shows("Main queue"), "{:?}", h.texts());
}

/// At the least width the desktop window had (640 points) and down to the breakpoint, the
/// window is the desktop's: the menus' titles and the toolbar, no phone's top bar; under it,
/// the phone's.
#[test]
fn at_the_desktops_least_width_the_layout_is_the_desktops() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    for width in [640.0, crate::window::narrow::NARROW_BELOW] {
        h.resize(width, 400.0);
        h.until_shown("Tasks");
        assert!(h.shows("Stop All"), "{width}: {:?}", h.texts());
        assert!(!h.shows("Places"), "{width}: {:?}", h.texts());
        assert!(!h.shows("Menu"), "{width}: {:?}", h.texts());
    }
    h.resize(PHONE.0, PHONE.1);
    h.until_shown("Places");
    assert!(!h.shows("Tasks"), "{:?}", h.texts());
}

/// The top bar's search opens the desktop's own search box across it: typing narrows the cards
/// as the desktop's box narrows the rows, how many are left said as it says it; its close
/// clears the words and gives the bar back.
#[test]
fn search_at_a_phones_width_narrows_the_cards() {
    let _file = settings_file();
    let world = world();
    let mut h = phone(&world);
    add_later(&mut h, &world, &["file.zip", "page.html", "other.pdf"]);
    h.until_shown("other.pdf");
    let filter = dm_desktop_base::i18n::t("Filter");
    h.person_click(filter, 1);
    h.until("the search box open", |h| {
        h.view(|view| view.narrow_searching())
    });
    h.until("the bar to give way to the box", |h| !h.shows("Menu"));
    h.type_text("page");
    h.until("the cards narrowed", |h| {
        h.view(|view| view.list.shown.len() == 1)
    });
    h.until_shown("1 shown");
    assert!(h.shows("page.html"), "{:?}", h.texts());
    assert!(!h.shows("file.zip"), "{:?}", h.texts());
    h.person_click(dm_desktop_base::i18n::t("Close"), 1);
    h.until("every card again, the bar back", |h| {
        h.view(|view| view.list.shown.len() == 3 && !view.narrow_searching())
    });
    h.until_shown("Menu");
    assert!(
        h.view(|view| view.list.filter_text.is_empty()),
        "the words stayed"
    );
}

/// The top bar names the place chosen, as the drawer lists it; at All, "dm".
#[test]
fn the_top_bar_names_the_place_chosen() {
    let _file = settings_file();
    let world = world();
    let mut h = phone(&world);
    h.until_shown("dm");
    let queue = "Main queue";
    assert!(!h.shows(queue), "{:?}", h.texts());
    h.update(|view, cx| {
        view.choose(Place::Queue(queue.into()));
        cx.notify();
    });
    h.until_shown(queue);
}

/// The list's kept place across a refresh counts cards at a phone's width: the card at the
/// top stays there when the cards are sorted the other way under it.
#[test]
fn the_card_at_the_top_holds_its_place_across_a_refresh() {
    let _file = settings_file();
    let world = world();
    let mut h = phone(&world);
    let names: Vec<String> = (1..=40).map(|n| format!("part{n:02}.bin")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    add_later(&mut h, &world, &names);
    // By name, A to Z, whatever sort another test kept.
    h.update(|view, cx| {
        view.list.sort = None;
        view.sort_by(dm_desktop_base::columns::Column::Name, cx);
    });
    let card = f32::from(crate::window::zoom::px(crate::window::narrow::CARD_HEIGHT));
    h.update(|view, _| {
        let state = view.list.scroll.0.borrow();
        state
            .base_handle
            .set_offset(gpui::point(gpui::px(0.0), gpui::px(-20.0 * card)));
    });
    h.until("the cards laid out scrolled", |h| {
        h.view(|view| view.list.visible.start >= 19)
    });
    let top = |view: &mut crate::window::list::Downloads| {
        let offset = f32::from(view.list.scroll.0.borrow().base_handle.offset().y);
        view.id_at((-offset / card).floor() as usize)
    };
    let was = h.view(top).expect("a card at the top");
    // Sorted the other way (Z to A): the card at 20 of 0 to 39 is at 19, above it one fewer.
    h.update(|view, cx| view.sort_by(dm_desktop_base::columns::Column::Name, cx));
    h.until("the sort", |h| {
        h.view(|view| view.position_of(was) == Some(19))
    });
    assert_eq!(h.view(top), Some(was), "the list moved");
}

/// The bottom navigation shows All, Unfinished and Finished in a tap, each the side panel's own
/// place, and its Places opens the drawer of the rest.
#[test]
fn the_bottom_navigation_chooses_the_places() {
    let _file = settings_file();
    let world = world();
    let mut h = phone(&world);
    h.until_shown("Places");
    for (words, place) in [
        ("Unfinished", Place::Unfinished),
        ("Finished", Place::Finished),
        ("All", Place::All),
    ] {
        h.person_click(dm_desktop_base::i18n::t(words), 1);
        h.until(words, |h| h.view(|view| view.list.place == place));
    }
    h.person_click("Places", 1);
    h.until("the drawer open", |h| {
        h.view(|view| view.layout.narrow.drawer.is_some())
    });
}
