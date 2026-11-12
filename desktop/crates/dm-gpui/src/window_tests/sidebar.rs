// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The side panel: its groups and the Grabber's projects.

use crate::harness::world::world;

/// A saved Grabber search is listed under Grabber projects in the side panel, as reference A
/// keeps its projects there; clicking it opens the Grabber with it filled in (gaps rows U40,
/// U41). Moved from the egui window's tests.
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
    let typed = h.dialog::<crate::grabber::Grabber, _>(|g, _, cx| g.typed(cx));
    assert_eq!(typed, (page, "pdf".into(), "20".into(), "Manuals".into()));
}

/// A group of places shuts from its heading and opens again from it, kept for the next
/// launch (gaps row U42). Moved from the egui window's tests.
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
