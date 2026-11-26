// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The side panel: its groups, the Grabber's projects, and its places reached by the keyboard.

use crate::harness::world::world;

/// A saved Grabber search is listed under Grabber projects in the side panel, as reference A
/// keeps its projects there; clicking it opens the Grabber with it filled in (gaps rows U40,
/// U41).
#[test]
fn a_saved_grabber_search_is_a_project_in_the_side_panel() {
    let _file = crate::harness::settings_file();
    let world = world();
    let mut h = world.harness();
    let page = world.url("page.html");
    let search = dm_shell::lists::Search {
        name: "Manuals".into(),
        page: page.clone(),
        types: vec!["pdf".into()],
        min_kb: 20,
        ..Default::default()
    };
    h.apply(|settings| settings.site_searches.push(search));
    h.until_shown("Grabber projects");
    h.click("Manuals");
    h.until("the Grabber", |h| h.dialog_open("Grabber"));
    let typed = h.dialog::<crate::dialogs::grabber::Grabber, _>(|g, _, cx| g.typed(cx));
    assert_eq!(typed, (page, "pdf".into(), "20".into(), "Manuals".into()));
}

/// A group of places shuts from its heading and opens again from it, kept for the next
/// launch (gaps row U42).
#[test]
fn a_group_of_places_shuts_and_opens_from_its_row() {
    let _file = crate::harness::settings_file();
    let world = world();
    let mut h = world.harness();
    h.until_shown("Main queue");
    h.click("Queues");
    h.until("the queues to be put away", |h| !h.shows("Main queue"));
    assert!(h.shows("Queues"), "the heading went with its places");
    assert!(h.shows("Unfinished"), "another group went too");
    assert_eq!(
        h.view(|view| view.backend.settings().shut_groups),
        ["Queues"]
    );
    h.click("Queues");
    h.until_shown("Main queue");
    assert!(h.view(|view| view.backend.settings().shut_groups.is_empty()));
}

/// The side panel's places are reached by Tab and chosen by Space, as a person without a mouse
/// chooses one: they were the pointer's alone.
#[test]
fn a_place_is_reached_by_tab_and_chosen_by_space() {
    let world = world();
    let mut h = world.harness();
    let first = h.view(|view| view.list.place.clone());
    let mut chosen = None;
    // A heading pressed shuts or opens its group; the walk goes on to a place all the same.
    for _ in 0..60 {
        h.key("tab", &[]);
        h.key("space", &[]);
        assert!(h.dialogs().is_empty(), "Space opened a dialog");
        let now = h.view(|view| view.list.place.clone());
        if now != first {
            chosen = Some(now);
            break;
        }
    }
    assert!(
        chosen.is_some(),
        "no place was reached by Tab and chosen by Space"
    );
}

/// A place with nothing in it says which place, under its picture, over "Nothing here yet.":
/// "Nothing here" over "Nothing here yet." said nothing, twice.
#[test]
fn an_empty_place_names_itself() {
    let world = world();
    let mut h = world.harness();
    let url = world.url("x.bin");
    h.view(|view| view.backend.add(&url)).unwrap();
    h.until("the row", |h| h.rows().len() == 1);
    h.update(|view, cx| {
        view.choose(dm_shell::app::places::Place::Kind(
            dm_core::Category::Pictures,
        ));
        cx.notify();
    });
    h.until_shown("Nothing here yet.");
    assert!(!h.shows("Nothing here"), "the empty place's old heading");
    let named = h
        .texts()
        .iter()
        .filter(|t| t.as_str() == "Pictures")
        .count();
    assert_eq!(named, 2, "the place's name in the panel and over the list");
}
