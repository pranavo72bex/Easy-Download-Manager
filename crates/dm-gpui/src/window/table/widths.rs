// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Each column's own width: its default, and as wide as its heading and its longest figures.

use dm_desktop_base::columns::Column;
use dm_desktop_base::format;

/// A column's width in points; the file name takes what the others leave.
pub(super) fn width(column: Column) -> Option<f32> {
    match column {
        Column::Name => None,
        // The queue's name, where it was a dot (gaps row E7).
        Column::Queue => Some(72.0),
        Column::Size => Some(72.0),
        // Its least: "Complete" and a percentage with their marks. It grows to twice that where
        // the window has room (`cell`); at 150 a new window's columns ran past the list's edge
        // (900 points on macOS and GNOME, the name at its least and Last try date cut).
        Column::Status => Some(108.0),
        Column::TimeLeft => Some(68.0),
        Column::Rate => Some(84.0),
        // Its own (`own_width`): as wide as its heading and a date of another year.
        Column::LastTry => Some(96.0),
        Column::Description | Column::Referrer | Column::Folder => Some(140.0),
    }
}

/// A column's own width where a person gave it none: its default, widened to its heading and a
/// long figure in the language shown, so neither is cut while the window has room for them
/// (`docs/ui-design.md` rule 7: an Arabic size, "2.1 ميغ…", and a cut heading had no way to be
/// read whole). Queue, where no row shown waits in one, as wide as its heading alone: its room
/// stood empty while the file's name beside it was cut. Status grows on its own (`cell`).
/// Without `headings` (the list has no room for every heading whole), a column as wide as its
/// figures need: its heading cut, whole on hover, rather than the list scrolling sideways at a
/// new window's size (`docs/ui-design.md` rule 7: cut only where there is no more room).
pub(super) fn own_width(column: Column, any_queue: bool, headings: bool) -> Option<f32> {
    use crate::widgets::native::{small_width, text_width};
    let default = width(column)?;
    // The cell's margins (`px_2` each side), and the sort arrow's room beside a heading.
    let margins = 16.0;
    let heading = small_width(&format!("{} ▼", column.title())) + margins;
    // A figure as long as the column says: a size and a rate just under a thousand of their
    // unit, the time left in its two largest units.
    // Under a thousand of each unit, as the computer counts (a KB 1000 bytes on macOS, 1024 on
    // Windows): "999.9 MB" and "1023.5 MB", wider than the "1.07 GB" a single number gave,
    // which "419.4 MB" outgrew; and in KB and GB too, whose words are wider in some languages
    // (Arabic's "ك.ب" than its "م.ب": "305.4 ك.ب/s" was cut).
    let large = [
        999_949_u64,
        1_047_552,
        999_949_999,
        1_073_217_536,
        999_949_999_999,
    ];
    let widest = |write: &dyn Fn(u64) -> String| {
        large
            .iter()
            .map(|&n| text_width(&write(n)))
            .fold(0.0_f32, f32::max)
            + margins
    };
    let figure = match column {
        Column::Size => widest(&format::bytes),
        Column::Rate => widest(&|n| format::rate(n as f64)),
        Column::TimeLeft => {
            text_width(&format::span(std::time::Duration::from_secs(59 * 60 + 59))) + margins
        }
        // A day of another year, as the computer writes one ("Sep 28, 2025", "28 Sep 2025"):
        // longer than a time today.
        Column::LastTry => text_width(&format::when(Some(LONG_DATE))) + margins,
        _ => 0.0,
    };
    let own = match column {
        Column::Status => default,
        Column::Queue if !any_queue => heading,
        _ if headings => default.max(heading).max(figure),
        _ => default.max(figure),
    };
    Some(own.ceil())
}

/// A moment of another year whose date is among the longest written (September, a two-figure
/// day): 2025-09-28 at noon, UTC.
const LONG_DATE: u64 = 1_759_060_800;
