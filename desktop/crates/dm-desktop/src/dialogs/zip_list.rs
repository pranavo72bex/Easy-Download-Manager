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

/// The most files shown; the rest are counted.
const MOST_SHOWN: usize = 500;

/// How long the reading of an address may take before the list is left out.
const WITHIN: std::time::Duration = std::time::Duration::from_secs(30);

/// The height of the list before it scrolls.
const HEIGHT: f32 = 140.0;

/// Whether an address names a ZIP: its path ends in `.zip`, a query ignored.
pub(super) fn is_zip_address(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.to_ascii_lowercase().ends_with(".zip")
}

/// Starts reading the files of the ZIP at `url` once, in the background
/// (`dm_core::zip_contents`: two or three small range requests, never the
/// archive). A reading under way or done is left as it is.
pub(super) fn read_address(
    runtime: &tokio::runtime::Handle,
    ctx: &egui::Context,
    slot: &mut Option<ZipSlot>,
    client: dm_core::reqwest::Client,
    url: String,
    referrer: Option<String>,
    sign_in: Option<dm_core::cookies::SignIn>,
) {
    if slot.is_some() {
        return;
    }
    let held: ZipSlot = Default::default();
    *slot = Some(held.clone());
    let ctx = ctx.clone();
    runtime.spawn(async move {
        let read = dm_core::zip_contents(&client, &url, referrer.as_deref(), sign_in.as_ref());
        let found = tokio::time::timeout(WITHIN, read)
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
        *dm_core::lock(&held) = Some(found);
        ctx.request_repaint();
    });
}

/// Starts reading the files of the finished ZIP at `path` once, in the
/// background.
pub(super) fn read_file(
    runtime: &tokio::runtime::Handle,
    ctx: &egui::Context,
    slot: &mut Option<ZipSlot>,
    path: std::path::PathBuf,
) {
    if slot.is_some() {
        return;
    }
    let held: ZipSlot = Default::default();
    *slot = Some(held.clone());
    let ctx = ctx.clone();
    runtime.spawn_blocking(move || {
        let found = dm_core::zip_contents_of_file(&path).unwrap_or_default();
        *dm_core::lock(&held) = Some(found);
        ctx.request_repaint();
    });
}

/// What a reading has found so far, as far as the window shows it.
pub(super) enum Listing {
    /// Nothing asked for, or nothing to show: no section.
    Nothing,
    /// Under way.
    Reading,
    /// The first files, and how many more there are.
    Files(Vec<dm_core::ZipEntry>, usize),
}

/// The state of `slot`, with at most [`MOST_SHOWN`] files copied out of it.
pub(super) fn listing(slot: &Option<ZipSlot>) -> Listing {
    let Some(slot) = slot else {
        return Listing::Nothing;
    };
    match &*dm_core::lock(slot) {
        None => Listing::Reading,
        Some(files) if files.is_empty() => Listing::Nothing,
        Some(files) => Listing::Files(
            files.iter().take(MOST_SHOWN).cloned().collect(),
            files.len().saturating_sub(MOST_SHOWN),
        ),
    }
}

/// The words of a section that is still being read.
pub(super) fn reading_words() -> String {
    t("Reading the contents…").to_string()
}

/// The list, as text: each file's name and, beside it, its size once
/// unpacked (a folder has none), scrolling past [`HEIGHT`].
pub(super) fn show(ui: &mut egui::Ui, files: &[dm_core::ZipEntry], more: usize) {
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
                            ui.label(RichText::new(format::bytes(file.size)).weak());
                        });
                    }
                });
            }
            if more > 0 {
                ui.label(RichText::new(tf("and {} more", &[&more.to_string()])).weak());
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(count: usize) -> Option<ZipSlot> {
        let files = (0..count)
            .map(|n| dm_core::ZipEntry {
                name: format!("file{n}.txt"),
                size: 10,
                packed: 10,
            })
            .collect();
        Some(std::sync::Arc::new(std::sync::Mutex::new(Some(files))))
    }

    #[test]
    fn an_address_names_a_zip_by_its_path_whatever_follows_it() {
        assert!(is_zip_address("http://host/files/pack.zip"));
        assert!(is_zip_address("http://host/files/PACK.ZIP?token=a.b#frag"));
        assert!(is_zip_address(r"C:\Users\me\Downloads\pack.zip"));
        assert!(!is_zip_address("http://host/files/pack.zip.txt"));
        assert!(!is_zip_address("http://host/download?file=pack.zip"));
    }

    #[test]
    fn at_most_five_hundred_files_are_copied_out_and_the_rest_counted() {
        match listing(&read(600)) {
            Listing::Files(shown, more) => {
                assert_eq!(shown.len(), 500);
                assert_eq!(shown[0].name, "file0.txt");
                assert_eq!(more, 100);
            }
            _ => panic!("expected files"),
        }
        assert!(matches!(listing(&read(500)), Listing::Files(shown, 0) if shown.len() == 500));
    }

    #[test]
    fn nothing_asked_for_or_nothing_found_shows_no_section_and_a_reading_under_way_says_so() {
        assert!(matches!(listing(&None), Listing::Nothing));
        assert!(matches!(listing(&read(0)), Listing::Nothing));
        let under_way: Option<ZipSlot> = Some(Default::default());
        assert!(matches!(listing(&under_way), Listing::Reading));
    }
}
