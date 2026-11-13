// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Remove and Delete file on the rows chosen, the question they ask, and Undo.

use dm_core::{Entry, State};
use gpui::Context;

use super::Downloads;

impl Downloads {
    /// Remove (or Delete file, `deleting_files`) on the rows chosen. Finished
    /// rows only leave the list, nothing lost, Undo on offer for a moment; any
    /// other is asked about first, as the egui window asks.
    pub(super) fn remove(
        &mut self,
        chosen: &[Entry],
        deleting_files: bool,
        cx: &mut Context<Self>,
    ) {
        let ids: Vec<dm_core::Id> = chosen.iter().map(|r| r.id).collect();
        if !deleting_files && chosen.iter().all(|r| r.state == State::Completed) {
            let rows = self.backend.finished_rows(&ids);
            self.backend.remove_many(&ids);
            let n = self.undo.keep(rows);
            if n > 0 {
                self.undo_expires(cx);
                self.say(dm_desktop_base::i18n::tn(
                    "Removed {} download from the list.||Removed {} downloads from the list.",
                    n as u64,
                    &[&n],
                ));
            }
            self.read();
            return;
        }
        // The finished files still on disk that could go with their rows.
        let files: Vec<String> = chosen
            .iter()
            .filter(|r| r.state == State::Completed && std::path::Path::new(&r.path).is_file())
            .map(|r| r.path.clone())
            .collect();
        let named = match chosen {
            [only] => Some(dm_shell::app::names::name_of(only)),
            _ => None,
        };
        let rows: Vec<Entry> = chosen.to_vec();
        let count_files = files.len();
        let words = move |ticked: bool| {
            let refs: Vec<&Entry> = rows.iter().collect();
            let ask = dm_desktop_base::asks::remove(
                &refs,
                count_files,
                ticked,
                deleting_files,
                named.clone(),
            );
            crate::question::Asked {
                title: ask.title.into(),
                lines: std::iter::once(ask.sentence)
                    .chain(ask.files_sentence)
                    .map(Into::into)
                    .collect(),
                named: Some(ask.named.into()),
                detail: None,
                toggle: ask.toggle.map(Into::into),
                ticked,
                buttons: vec![ask.keep.into(), ask.go.into()],
                default: 0,
                escape: 0,
            }
        };
        // Delete file starts with its file ticked to go too, as the egui window's did; Remove
        // never does.
        let asked = words(deleting_files && count_files > 0);
        let view = cx.entity().downgrade();
        let answer: crate::question::Answer = Box::new(move |button, delete, cx| {
            if button == 1 {
                let _ = view.update(cx, |view, cx| {
                    view.removed(&ids, &files, delete, cx);
                    cx.notify();
                });
            }
        });
        self.sheet(asked, Some(Box::new(words)), None, answer, cx);
    }

    /// The rows `ids` taken off the list, their finished `files` with them
    /// to the Trash where `delete`; the choice goes to the row after them.
    /// The Trash and the queue's change are made off the window's thread
    /// (each file is a round trip to the Finder on macOS: gaps row 183),
    /// and the choice moved and a failure said once they are done.
    pub(super) fn removed(
        &mut self,
        ids: &[dm_core::Id],
        files: &[String],
        delete: bool,
        cx: &mut Context<Self>,
    ) {
        // The row after those removed, or the one before where they were
        // the last, is chosen next, so the keys have somewhere to go on from.
        let shown: Vec<dm_core::Id> = (0..self.shown.len())
            .filter_map(|p| self.id_at(p))
            .collect();
        let next = shown
            .iter()
            .rposition(|id| ids.contains(id))
            .and_then(|last| {
                shown[last + 1..]
                    .iter()
                    .find(|id| !ids.contains(id))
                    .or_else(|| shown[..last].iter().rev().find(|id| !ids.contains(id)))
                    .copied()
            });
        let (ids, files) = (ids.to_vec(), files.to_vec());
        self.later(
            move |backend| {
                let mut failed: Vec<String> = Vec::new();
                if delete {
                    for path in &files {
                        // A finished torrent still shared lets go of its files first.
                        dm_core::stop_sharing(std::path::Path::new(path));
                        if let Err(e) = backend.platform.trash(path) {
                            failed.push(format!("{path}: {e}"));
                        }
                    }
                }
                backend.remove_many(&ids);
                failed
            },
            move |this, failed, _| {
                this.read();
                this.selected = next.into_iter().collect();
                this.anchor = next;
                this.cursor = next;
                if !failed.is_empty() {
                    this.say(dm_desktop_base::i18n::tf(
                        "Could not delete {}",
                        &[&failed.join("; ")],
                    ));
                }
            },
            cx,
        );
    }

    /// Draws the window again once Undo's time is over, so a lapsed Undo is not left drawn
    /// while nothing else wakes the window (gaps row U14).
    pub(super) fn undo_expires(&mut self, cx: &mut Context<Self>) {
        let over = dm_shell::app::backend::UNDO_FOR + std::time::Duration::from_millis(50);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(over).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    /// Puts back the rows the last Remove took off, while Undo is on offer.
    pub fn undo_remove(&mut self) {
        if let Some(ids) = self.undo.put_back(&self.backend) {
            let n = ids.len();
            self.read();
            self.selected = ids.into_iter().collect();
            self.say(dm_desktop_base::i18n::tn(
                "Put back {} download.||Put back {} downloads.",
                n as u64,
                &[&n],
            ));
        }
    }
}
