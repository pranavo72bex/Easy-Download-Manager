// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A row's picture of its file's kind, and what its line says.

use dm_core::{Entry, State};
use gpui::SharedString;

/// A row's picture of its file's kind (B45), told apart by its shape (a film strip, a note, a
/// page, four tiles, a box, a landscape, three dots).
pub(crate) fn kind_path(kind: Option<dm_core::Category>) -> &'static str {
    use dm_core::Category as K;
    match kind {
        Some(K::Videos) => "icons/kind-videos.svg",
        Some(K::Audio) => "icons/kind-audio.svg",
        Some(K::Documents) => "icons/kind-documents.svg",
        Some(K::Apps) => "icons/kind-apps.svg",
        Some(K::Archives) => "icons/kind-archives.svg",
        Some(K::Pictures) => "icons/kind-pictures.svg",
        _ => "icons/kind-other.svg",
    }
}

/// A row's picture of its file's kind and its colour in `theme`'s look (`Theme::kinds`).
pub(crate) fn kind_picture(
    kind: Option<dm_core::Category>,
    theme: crate::window::theme::Theme,
) -> (&'static str, gpui::Rgba) {
    (kind_path(kind), theme.kinds.of_kind(kind))
}

/// What a row's line says, in the shell: its name and its state.
pub(crate) fn line(row: &Entry) -> (SharedString, SharedString) {
    let name = if row.filename.is_empty() {
        dm_shell::app::names::provisional(&row.url)
    } else {
        row.filename.clone()
    };
    let state = match &row.state {
        State::Running => match row.progress.total {
            Some(total) if total > 0 => {
                format!("{}%", row.progress.downloaded.saturating_mul(100) / total)
            }
            _ => dm_desktop_base::format::bytes(row.progress.downloaded),
        },
        State::Failed(why) => format!("Failed: {why}"),
        other => format!("{other:?}"),
    };
    (name.into(), state.into())
}
