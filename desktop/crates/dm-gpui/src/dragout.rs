// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Finished files dragged out of the window, into Finder or a file
//! manager's folder, a mail or a chat, as the egui window lets them be.
//!
//! A finished row is dragged as any GPUI element is (`on_drag`), its file's
//! name under the pointer while it is over the window. Once the pointer
//! leaves the window, GPUI hands the drag to the system with the files the
//! row carries (`external_drag_payload`): AppKit's dragging session on
//! macOS, the data device on Wayland, and dm's own in the vendored GPUI
//! (`desktop/vendor/README.md`): an XDND source on X11, the shell's
//! `SHDoDragDrop` on Windows. The system's drag runs beside GPUI's event
//! loop rather than inside an update of it, so nothing is re-entered.

use std::collections::BTreeSet;
use std::path::PathBuf;

use dm_core::{Entry, Id, State};
use gpui::{
    div, prelude::*, App, Context, Entity, ExternalDragPayload, FileDragPaths, SharedString,
    WeakEntity, Window,
};

use crate::list::Downloads;
use crate::theme::Theme;
use crate::zoom::px;

/// A row being dragged: which one, and the list it was dragged from.
pub(crate) struct DraggedRow {
    pub(crate) id: Id,
    pub(crate) list: WeakEntity<Downloads>,
    pub(crate) name: SharedString,
    pub(crate) theme: Theme,
}

impl DraggedRow {
    /// What is drawn under the pointer while the drag is over the window:
    /// the file's name, as a dragged heading shows its title.
    pub(crate) fn picture(&self, cx: &mut App) -> Entity<DraggedFile> {
        let (name, theme) = (self.name.clone(), self.theme);
        cx.new(|_| DraggedFile { name, theme })
    }

    /// The files handed to the system as the drag leaves the window: read
    /// then, not as the drag began, so a file deleted meanwhile is not
    /// offered. None when nothing is left to carry; the drag then stays a
    /// drag inside the window and lands nowhere.
    pub(crate) fn payload(&self, cx: &App) -> Option<ExternalDragPayload> {
        let list = self.list.upgrade()?;
        let files = carried(list.read(cx), self.id);
        if files.is_empty() {
            return None;
        }
        Some(ExternalDragPayload::Files(FileDragPaths::new(
            files.into_iter().map(|file| {
                // A torrent's download can be a folder.
                let folder = file.is_dir();
                (file, folder)
            }),
        )))
    }
}

/// Whether a row looks like it holds a file to drag, without asking the
/// disk: a finished row with a path. Rows are drawn many times a second.
pub(crate) fn may_carry(row: &Entry) -> bool {
    row.state == State::Completed && !row.path.is_empty()
}

/// The files a drag from row `id` carries, as the egui window's
/// (`dm-desktop` `columns::files_to_drag`): the chosen rows, in the list's
/// order, when the row is one of them, else that row alone; of those, the
/// finished ones whose file is still where dm saved it.
pub(crate) fn carried(view: &Downloads, id: Id) -> Vec<PathBuf> {
    files_of(
        view.shown
            .iter()
            .filter_map(|&r| view.rows.get(r))
            .map(|r| &**r),
        &view.selected,
        id,
        dm_desktop_base::rows::gone,
    )
}

/// `carried`, given the rows shown in order, the chosen ones and how to
/// tell a file that has gone from the disk.
fn files_of<'a>(
    shown: impl Iterator<Item = &'a Entry>,
    selected: &BTreeSet<Id>,
    id: Id,
    gone: impl Fn(&Entry) -> bool,
) -> Vec<PathBuf> {
    let from_chosen = selected.contains(&id);
    shown
        .filter(|r| {
            if from_chosen {
                selected.contains(&r.id)
            } else {
                r.id == id
            }
        })
        .filter(|r| may_carry(r) && !gone(r))
        .map(|r| PathBuf::from(&r.path))
        .filter(|p| p.is_absolute())
        .collect()
}

/// A dragged file's name, drawn under the pointer.
pub(crate) struct DraggedFile {
    name: SharedString,
    theme: Theme,
}

impl Render for DraggedFile {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py(px(2.0))
            .rounded(px(4.0))
            .border_1()
            .border_color(self.theme.separator)
            .bg(self.theme.window)
            .text_size(px(crate::native::SMALL_TEXT))
            .text_color(self.theme.text)
            .child(self.name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file in a folder `d` at the top of the disk: on Windows, of drive C (`/d/a.zip` is not
    /// a whole path there, and is rightly not carried).
    fn at(name: &str) -> String {
        if cfg!(windows) {
            format!("C:\\d\\{name}")
        } else {
            format!("/d/{name}")
        }
    }

    fn row(id: Id, state: State, path: &str) -> Entry {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "url": format!("https://example.com/{id}"),
            "state": serde_json::to_value(state).unwrap(),
            "progress": {"downloaded": 10, "total": 10, "speed": 0.0, "eta": null},
            "filename": "f",
            "path": path,
        }))
        .unwrap()
    }

    #[test]
    fn a_chosen_row_carries_every_chosen_finished_file_in_the_lists_order() {
        let rows = [
            row(3, State::Completed, &at("c.zip")),
            row(1, State::Completed, &at("a.zip")),
            row(2, State::Running, &at("b.zip")),
            row(4, State::Completed, &at("d.zip")),
        ];
        let chosen = BTreeSet::from([1, 2, 3]);
        let files = files_of(rows.iter(), &chosen, 2, |_| false);
        assert_eq!(
            files,
            [PathBuf::from(at("c.zip")), PathBuf::from(at("a.zip"))]
        );
    }

    #[test]
    fn a_row_outside_the_selection_carries_its_own_file_alone() {
        let rows = [
            row(1, State::Completed, &at("a.zip")),
            row(2, State::Completed, &at("b.zip")),
        ];
        let chosen = BTreeSet::from([1]);
        assert_eq!(
            files_of(rows.iter(), &chosen, 2, |_| false),
            [PathBuf::from(at("b.zip"))]
        );
    }

    #[test]
    fn a_file_gone_unfinished_or_without_a_full_path_is_not_carried() {
        let rows = [
            row(1, State::Completed, "/d/gone.zip"),
            row(2, State::Failed("refused".into()), &at("b.zip")),
            row(3, State::Completed, "relative.zip"),
            row(4, State::Completed, ""),
        ];
        let chosen = BTreeSet::from([1, 2, 3, 4]);
        let gone = |r: &Entry| r.path.ends_with("gone.zip");
        assert!(files_of(rows.iter(), &chosen, 1, gone).is_empty());
        assert!(!may_carry(&rows[1]) && !may_carry(&rows[3]));
    }
}
