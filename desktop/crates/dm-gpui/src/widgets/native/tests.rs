// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::{Each, WindowSize};

/// Each window's row, as each system had it before the table: a move
/// of the numbers, not a change to any.
#[test]
fn window_sizes_are_as_they_were() {
    let rows = [
        (
            WindowSize::Main,
            (900.0, 600.0),
            (840.0, 520.0),
            (900.0, 600.0),
        ),
        (
            WindowSize::Options,
            (680.0, 600.0),
            (620.0, 520.0),
            (680.0, 600.0),
        ),
        (
            WindowSize::Grabber,
            (640.0, 560.0),
            (600.0, 480.0),
            (640.0, 560.0),
        ),
        (
            WindowSize::Links,
            (760.0, 560.0),
            (680.0, 480.0),
            (760.0, 560.0),
        ),
        (
            WindowSize::ClipboardLinks,
            (520.0, 300.0),
            (520.0, 300.0),
            (520.0, 300.0),
        ),
        (
            WindowSize::AddUrl,
            (520.0, 220.0),
            (520.0, 220.0),
            (520.0, 220.0),
        ),
        (
            WindowSize::Series,
            (520.0, 300.0),
            (520.0, 300.0),
            (520.0, 300.0),
        ),
        (
            WindowSize::Playlist,
            (560.0, 420.0),
            (560.0, 420.0),
            (560.0, 420.0),
        ),
        (
            WindowSize::Complete,
            (440.0, 220.0),
            (440.0, 220.0),
            (440.0, 220.0),
        ),
        (
            WindowSize::Progress,
            (480.0, 320.0),
            (480.0, 320.0),
            (480.0, 320.0),
        ),
        (
            WindowSize::Properties,
            (560.0, 420.0),
            (560.0, 420.0),
            (560.0, 420.0),
        ),
        (
            WindowSize::Scheduler,
            (720.0, 520.0),
            (720.0, 520.0),
            (720.0, 520.0),
        ),
        (
            WindowSize::Welcome,
            (480.0, 300.0),
            (480.0, 300.0),
            (480.0, 300.0),
        ),
    ];
    for (window, macos, windows, gnome) in rows {
        assert_eq!(
            window.each(),
            Each {
                macos,
                windows,
                gnome
            },
            "{window:?}"
        );
        let here = if cfg!(target_os = "macos") {
            macos
        } else if cfg!(windows) {
            windows
        } else {
            gnome
        };
        assert_eq!(window.here(), here, "{window:?}");
    }
}
