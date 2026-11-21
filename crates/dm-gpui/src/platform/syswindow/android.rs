// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Android (gaps row N17): the window fills the phone's screen and the app's activity holds
//! it. There is no tray to put it away into, and no place to move it to: hiding and showing
//! do nothing (the phone's own Home and Back do that), and moving or picturing it is refused
//! in words.

use super::*;

/// Nothing: the phone puts the app away itself.
pub fn hide(_: &Window) {}

/// Nothing: the window is already the phone's screen.
pub fn show(_: &Window) {}

/// Refused: a phone's window cannot be moved.
pub fn move_to(_: &Window, _: f32, _: f32) -> Result<(), Error> {
    Err("a phone's window fills its screen and cannot be moved".into())
}

/// Refused, as `move_to`.
pub fn place(window: &Window, x: f32, y: f32) -> Result<(), Error> {
    move_to(window, x, y)
}

/// Refused: the phone gives an app no picture of its own window.
pub fn picture(_: &Window, _: &std::path::Path) -> Result<(), Error> {
    Err("a phone gives dm no picture of its own window".into())
}

/// There is nowhere to put the window away: closing it ends dm, asked first while downloads
/// run, as Exit is.
pub fn nowhere_to_hide(_: &Window) -> bool {
    true
}
