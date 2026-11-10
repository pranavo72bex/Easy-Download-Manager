// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// "Playlist": a YouTube playlist read by dm itself (`dm_core::youtube`),
/// or another site's read by dm (`dm_core::videopage::read_playlist`): its
/// videos listed with a tick each, to download at one quality, as
/// video or as sound alone, now or for later.
pub(super) fn playlist(app: &mut DmApp, ctx: &egui::Context) {
    let focus = app.dialogs.take_focus("Playlist");
    let Some(d) = app.dialogs.playlist.as_mut() else {
        return;
    };
    let read: Option<Result<dm_core::youtube::Playlist, String>> =
        d.read.as_ref().and_then(|slot| slot.lock().ok()?.clone());
    match &read {
        // The reading is looked at again shortly: nothing else repaints.
        None => ctx.request_repaint_after(std::time::Duration::from_millis(200)),
        Some(Ok(list)) if d.picked.len() != list.entries.len() => {
            d.picked = vec![true; list.entries.len()];
        }
        _ => {}
    }
    let mut open = true;
    let mut add: Option<bool> = None;
    let mut cancel = false;
    window(ctx, "Playlist").open(&mut open).show(ctx, |ui| {
        ui.label(RichText::new(&d.url).weak());
        let row = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
        match &read {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(t("Reading the playlist…")).weak());
                });
            }
            Some(Err(why)) => {
                warning_line(ui, &tf("The playlist could not be read: {}", &[why]));
            }
            Some(Ok(list)) => {
                ui.label(RichText::new(&list.title).strong());
                let n = list.entries.len();
                let words = dm_desktop_base::file_info::playlist_count_words(list);
                kept_line(ui, RichText::new(words).weak());
                ui.horizontal(|ui| {
                    let mut all = d.picked.iter().all(|p| *p);
                    if ui.checkbox(&mut all, t("Select all")).changed() {
                        d.picked.fill(all);
                    }
                    ui.separator();
                    let name = ui.label(t("Video quality"));
                    let current = QUALITIES
                        .iter()
                        .find(|(v, _)| *v == d.quality)
                        .map(|(_, l)| *l)
                        .unwrap_or("Best available");
                    combo("playlist-quality")
                        .selected_text(t(current))
                        .show_ui(ui, |ui| {
                            for (value, label) in QUALITIES {
                                pick(ui, &mut d.quality, value.to_string(), t(label));
                            }
                        })
                        .response
                        .labelled_by(name.id);
                });
                // Each video with its number and length, ticked to be
                // taken; only the rows in view are drawn.
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    egui::ScrollArea::vertical()
                        .id_salt("playlist-videos")
                        .min_scrolled_height(row * 8.0)
                        .max_height(row * 12.0)
                        .auto_shrink([false, false])
                        .show_rows(ui, row, n, |ui, rows| {
                            for i in rows {
                                let Some(entry) = list.entries.get(i) else {
                                    continue;
                                };
                                let words =
                                    dm_desktop_base::file_info::playlist_entry_words(i, entry);
                                if let Some(picked) = d.picked.get_mut(i) {
                                    ui.checkbox(picked, words);
                                }
                            }
                        });
                });
            }
        }
        if !d.says.is_empty() {
            warning_line(ui, &d.says);
        }
        let ready = matches!(&read, Some(Ok(_)));
        footer(ui, |ui| {
            ui.add_enabled_ui(ready, |ui| {
                if focused(primary(ui, "Start now"), focus).clicked() || (ready && enter_free(ui)) {
                    add = Some(false);
                }
                if ui.button(t("Add for later")).clicked() {
                    add = Some(true);
                }
            });
            if ui.button(t("Cancel")).clicked() {
                cancel = true;
            }
        });
    });
    if !open || cancel {
        app.dialogs.playlist = None;
        return;
    }
    let Some(later) = add else {
        return;
    };
    let Some(Ok(list)) = read else {
        return;
    };
    // What it adds is dm's (`dm_desktop_base::file_info::playlist_adds`),
    // as every window's.
    let (urls, with) = match dm_desktop_base::file_info::playlist_adds(
        &list, &d.picked, &d.quality, &d.url, later,
    ) {
        Ok(adds) => adds,
        Err(words) => {
            d.says = words.to_string();
            return;
        }
    };
    match app.add(&urls, with) {
        Ok(n) => {
            app.said = crate::i18n::Said::counting(
                "Added {} download.||Added {} downloads.",
                n as u64,
                &[&n],
            );
            app.dialogs.playlist = None;
        }
        Err(e) => {
            if let Some(d) = app.dialogs.playlist.as_mut() {
                d.says = e;
            }
        }
    }
}
