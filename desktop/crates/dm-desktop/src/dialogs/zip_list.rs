// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A ZIP's files, listed where a person decides about the download: Download
//! details lists them before the file is fetched, read from the end of the
//! archive alone with range requests, and a finished ZIP's Properties lists
//! them from the file on disk (reference A shows a ZIP's contents; the
//! maintainer's word of 2026-11-01). Only text: nothing in an archive is
//! opened, unpacked or followed.

use egui::RichText;

use super::ZipSlot;
use crate::format;
use crate::i18n::{t, tf};
pub(super) use dm_desktop_base::zip_list::{
    is_zip_address, listing, reading_words, taken_words, Listing,
};

/// The height of the list before it scrolls.
const HEIGHT: f32 = 140.0;

/// Starts reading the files of the ZIP at `url` once, in the background
/// (the application's `files::list_zip_at`). A reading under way or done is
/// left as it is.
pub(super) fn read_address(
    backend: &dm_shell::app::backend::Backend,
    ctx: &egui::Context,
    slot: &mut Option<ZipSlot>,
    url: String,
    referrer: Option<String>,
    sign_in: Option<dm_core::cookies::SignIn>,
) {
    if slot.is_some() {
        return;
    }
    let ctx = ctx.clone();
    *slot = Some(dm_shell::app::files::list_zip_at(
        backend,
        url,
        referrer,
        sign_in,
        move || ctx.request_repaint(),
    ));
}

/// Starts reading the files of the finished ZIP at `path` once, in the
/// background (the application's `files::list_zip_file`).
pub(super) fn read_file(
    backend: &dm_shell::app::backend::Backend,
    ctx: &egui::Context,
    slot: &mut Option<ZipSlot>,
    path: std::path::PathBuf,
) {
    if slot.is_some() {
        return;
    }
    let ctx = ctx.clone();
    *slot = Some(dm_shell::app::files::list_zip_file(
        backend,
        path,
        move || ctx.request_repaint(),
    ));
}

/// The list, as text: each file's name and, beside it, its size once
/// unpacked (a folder has none), scrolling past [`HEIGHT`]. With `take`, each
/// file of a size that can be taken out alone has a "Take out" button
/// (reference A takes one file out of an archive before it is downloaded);
/// the file clicked is returned.
pub(super) fn show(
    ui: &mut egui::Ui,
    files: &[dm_core::ZipEntry],
    more: usize,
    take: bool,
) -> Option<dm_core::ZipEntry> {
    let mut chosen = None;
    egui::ScrollArea::vertical()
        .id_salt("zip-contents")
        .max_height(HEIGHT)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for file in files {
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(file.name.as_str()).truncate());
                    if !file.is_folder() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if take
                                && file.packed <= dm_core::ZIP_MOST_TAKEN
                                && ui.small_button(t("Take out")).clicked()
                            {
                                chosen = Some(file.clone());
                            }
                            ui.label(RichText::new(format::bytes(file.size)).weak());
                        });
                    }
                });
            }
            if more > 0 {
                ui.label(RichText::new(tf("and {} more", &[&more.to_string()])).weak());
            }
        });
    chosen
}
