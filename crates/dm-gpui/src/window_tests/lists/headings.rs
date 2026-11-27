// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The list's section headings (`list::groups`): sorted by Status a heading stands over each
//! state's rows; sorted by name, or in the order added, none. A heading is a line of the table
//! and not a row: the keys step from row to row past it, and Select all chooses the rows.

use dm_core::State;
use dm_desktop_base::columns::Column;

use super::{add_later, shown};
use crate::harness::world::world;
use crate::window::list::Line;

#[test]
fn sorted_by_status_each_state_has_its_heading_and_the_keys_step_past_it() {
    let world = world();
    let mut h = world.harness();
    for name in ["a.bin", "b.bin", "c.bin"] {
        add_later(&mut h, &world, name);
    }
    h.until("three rows", |h| h.rows().len() == 3);
    let cancelled = h.row("b.bin").id;
    h.view(|view| view.backend.cancel(cancelled));
    h.until("one cancelled", |h| {
        h.rows().iter().any(|r| r.state == State::Cancelled)
    });
    // In the order added: no heading.
    assert!(h.view(|view| view.headings().is_empty()));
    h.click("Status");
    h.until("the sort by status", |h| {
        h.view(|view| view.list.sort) == Some((Column::Status, true))
    });
    let headings: Vec<(usize, String)> = h.view(|view| {
        view.headings()
            .iter()
            .map(|(at, words)| (*at, words.to_string()))
            .collect()
    });
    assert_eq!(
        headings,
        [(0, "Stopped".to_string()), (2, "Cancelled".to_string())]
    );
    assert_eq!(shown(&mut h)[2], "b.bin");
    // Five lines: a heading, two rows, a heading, a row; and back again.
    let lines: Vec<Option<Line>> = h.view(|view| (0..6).map(|l| view.line(l)).collect());
    assert_eq!(
        lines,
        [
            Some(Line::Heading(0)),
            Some(Line::Row { at: 0, nth: 0 }),
            Some(Line::Row { at: 1, nth: 1 }),
            Some(Line::Heading(1)),
            Some(Line::Row { at: 2, nth: 0 }),
            None,
        ]
    );
    assert_eq!(
        h.view(|view| (0..3).map(|at| view.line_of(at)).collect::<Vec<_>>()),
        [1, 2, 4]
    );
    assert_eq!(h.view(|view| view.rows_on(0..4)), 0..2);
    assert!(h.shows("Cancelled"));
    // The keys go from the last stopped row to the cancelled one, past its heading.
    h.select("c.bin");
    h.key("down", &[]);
    h.until("the cancelled row chosen", |h| {
        h.view(|view| view.list.selected.contains(&cancelled) && view.list.selected.len() == 1)
    });
    // Select all chooses the three rows, no heading among them.
    h.key("a", &["cmd"]);
    h.until("all three chosen", |h| {
        h.view(|view| view.list.selected.len() == 3)
    });
    // Sorted by name: no heading.
    h.click("File name");
    h.until("the sort by name", |h| {
        h.view(|view| view.list.sort.is_some_and(|(c, _)| c == Column::Name))
    });
    assert!(h.view(|view| view.headings().is_empty()));
    assert_eq!(h.view(|view| view.line_count()), 3);
}
