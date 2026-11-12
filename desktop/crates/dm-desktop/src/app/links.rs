// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Links in text and in files (`dm_shell::app::links`), and a file egui hands
//! over as a drop read for them.

pub use dm_shell::app::links::*;

/// The links in a file dropped on the window: read from its path, else from
/// the bytes egui carries for it.
pub fn dropped_links(file: &dyn egui::DroppedFile) -> Vec<String> {
    links_at(file.path(), || file.bytes().ok())
}
