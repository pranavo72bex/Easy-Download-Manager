// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Every action against the phone's sheets and the computer's menus (gaps row N20): the one
//! list (`window/reach.rs`) says where each shows and whether a phone has it; this walks what
//! each layout really draws.

use edm_shell::app::places::Place;

use crate::harness::settings_file;
use crate::harness::world::world;
use crate::window::reach::{on_phone, walk, Act, Phone};

/// The places a sheet or a right-click opens a menu for, one of each kind.
fn places() -> Vec<Place> {
    vec![
        Place::All,
        Place::Kind(edm_core::Category::Audio),
        Place::Custom("Mine".into()),
        Place::Unfinished,
        Place::Finished,
        Place::Queue("Main queue".into()),
    ]
}

/// The phone's ⋮ sheet, a card's sheet for a finished and an unfinished download, and each
/// place's sheet reach every action a phone has, and show none it leaves out; each command
/// they show is one the window carries out; each action is drawn somewhere on a computer.
#[test]
fn every_action_is_reached_by_touch() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.resize(412.0, 915.0);
    h.call(
        "downloads.add",
        serde_json::json!({ "urls": [world.url("file.zip")] }),
    )
    .unwrap();
    h.call(
        "downloads.add",
        serde_json::json!({ "urls": [world.url("page.html")], "later": true, "again": true }),
    )
    .unwrap();
    h.until("one finished, one waiting", |h| {
        let rows = h.rows();
        rows.len() == 2 && rows.iter().any(|r| r.state == edm_core::State::Completed)
    });
    let ids: Vec<edm_core::Id> = h.rows().iter().map(|r| r.id).collect();
    let mut phone = Vec::new();
    let mut computer = Vec::new();
    let mut unknown = Vec::new();
    for id in ids {
        let (on_phone_card, on_computer_row) = h.view(|view| {
            view.list.selected = [id].into();
            (
                crate::window::rowmenu::card_menu_on(view, true).owned(),
                crate::window::rowmenu::row_menu(view, false).owned(),
            )
        });
        walk(&on_phone_card.items, &mut phone, &mut unknown);
        walk(&on_computer_row.items, &mut computer, &mut unknown);
    }
    let (overflow, narrowed) = h.view(|view| {
        (
            crate::window::narrow::sheet::overflow_on(view, true),
            crate::window::narrow::sheet::overflow_on(view, false),
        )
    });
    walk(&overflow.items, &mut phone, &mut unknown);
    // A computer: its menu bar (macOS's app, Edit and Window menus too), the Dock's menu, and
    // its window narrowed to a phone's width (its cards' Select).
    for menu in crate::window::menus::bar_and_dock(&Default::default()) {
        walk(&menu.owned().items, &mut computer, &mut unknown);
    }
    walk(&narrowed.items, &mut computer, &mut unknown);
    let narrowed_card = h.view(|view| crate::window::rowmenu::card_menu_on(view, false).owned());
    walk(&narrowed_card.items, &mut computer, &mut unknown);
    for place in places() {
        let items = gpui::Menu::new("place")
            .items(crate::window::placemenu::place_items(&place))
            .owned()
            .items;
        walk(&items, &mut phone, &mut unknown);
        walk(&items, &mut computer, &mut unknown);
    }
    assert!(
        unknown.is_empty(),
        "items the list does not know: {unknown:?}"
    );
    let mut failures = Vec::new();
    for act in Act::all() {
        let by_touch = phone.contains(&act);
        let reason = match on_phone(act) {
            Phone::Shown => None,
            Phone::Greyed(why) | Phone::Left(why) | Phone::PhoneOnly(why) => Some(why),
        };
        if reason.is_some_and(|why| why.trim().is_empty()) {
            failures.push(format!("{act:?}: left out or greyed with no reason"));
        }
        match on_phone(act) {
            Phone::Shown | Phone::Greyed(_) | Phone::PhoneOnly(_) if !by_touch => failures.push(
                format!("{act:?}: a phone has it, and touch cannot reach it"),
            ),
            Phone::Left(_) if by_touch => {
                failures.push(format!("{act:?}: left off the phone, and drawn there"))
            }
            _ => {}
        }
        let on_computer = computer.contains(&act);
        match on_phone(act) {
            Phone::PhoneOnly(_) if on_computer => {
                failures.push(format!("{act:?}: a phone's alone, and drawn on a computer"))
            }
            Phone::Shown | Phone::Greyed(_) | Phone::Left(_) if !on_computer => {
                failures.push(format!("{act:?}: drawn nowhere on a computer"))
            }
            _ => {}
        }
        // A command a phone shows does something there: the window carries it out, and one
        // greyed is one Android gives EDM no way to do.
        if let (true, Act::Run(command)) = (by_touch, act) {
            if !crate::window::menus::carried_out(command) {
                failures.push(format!("{act:?}: shown, and not carried out"));
            }
        }
    }
    for (command, when) in crate::window::reach::WHEN_DONE {
        let greyed = matches!(on_phone(Act::Run(command)), Phone::Greyed(_));
        let android = when.possible(|w| edm_desktop_base::power::way(w, "android").is_some());
        if greyed == android {
            failures.push(format!("{command:?}: greyed on the phone is {greyed}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
