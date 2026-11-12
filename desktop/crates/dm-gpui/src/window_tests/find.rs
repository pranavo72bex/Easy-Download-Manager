// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Find (⌘F) and Find next and previous (F3, Shift-F3) over the rows the search box shows.

use dm_shell::app::commands::Command;
use serde_json::json;

use crate::harness::world::world;

/// The box narrows by name, address or note, and F3 steps through what it shows: a row shown
/// for its address alone is found, where F3 matched names alone and said "Nothing is called"
/// over a row on screen (gaps row U30, B48).
#[test]
fn find_next_steps_through_the_rows_the_box_shows() {
    let world = world();
    let mut h = world.harness();
    // Two rows waiting: one named for its file, one whose words are in its address alone.
    h.call(
        "downloads.add",
        json!({ "urls": [world.url("file.zip"), world.url("named")], "later": true }),
    )
    .unwrap();
    h.until("two rows", |h| h.rows().len() == 2);
    // The box's words: the server's port, in both addresses and in neither name.
    let port = world
        .url("")
        .rsplit(':')
        .next()
        .unwrap()
        .trim_end_matches('/')
        .to_string();
    let run = |h: &mut crate::harness::Harness, command: Command| {
        h.call("command.run", json!({ "id": command.id() }))
            .unwrap();
    };
    run(&mut h, Command::Find);
    h.type_text(&port);
    h.until("the box to narrow by address", |h| {
        h.view(|view| view.filter_text.clone()) == port
    });
    assert_eq!(h.view(|view| view.shown.len()), 2, "the box shows both");
    run(&mut h, Command::FindNext);
    let first = h.view(|view| view.selected.clone());
    assert_eq!(first.len(), 1, "Find next chose no row the box shows");
    run(&mut h, Command::FindNext);
    let second = h.view(|view| view.selected.clone());
    assert_eq!(second.len(), 1);
    assert_ne!(
        first, second,
        "Find next did not go on to the next row shown"
    );
    run(&mut h, Command::FindPrevious);
    assert_eq!(
        h.view(|view| view.selected.clone()),
        first,
        "Find previous did not go back"
    );
    // And by its key, from the search box's own field (B48).
    h.key("f3", &[]);
    assert_eq!(
        h.view(|view| view.selected.clone()),
        second,
        "F3 from the box did not go on"
    );
}
