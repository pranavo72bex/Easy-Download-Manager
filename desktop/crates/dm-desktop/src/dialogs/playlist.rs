use super::*;

/// "Playlist": a YouTube playlist read by dm itself (`dm_core::youtube`),
/// or another site's read by yt-dlp (`dm_core::ytdlp::read_playlist`): its
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
                let mut words = crate::i18n::tn("{} video||{} videos", n as u64, &[&n]);
                if list.cut_short {
                    words = format!(
                        "{words} · {}",
                        tf("Only the first {} videos are listed.", &[&n])
                    );
                }
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
                                let mut words = format!("{}. {}", i + 1, entry.title);
                                if let Some(seconds) = entry.seconds {
                                    words = format!(
                                        "{words} · {}",
                                        dm_core::Span::clock(seconds as f64)
                                    );
                                }
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
    let urls: Vec<String> = list
        .entries
        .iter()
        .zip(&d.picked)
        .filter(|(_, picked)| **picked)
        .map(|(entry, _)| entry.url())
        .collect();
    if urls.is_empty() {
        d.says = t("Pick at least one video.").to_string();
        return;
    }
    let with = AddOptions {
        later,
        queue: if later {
            MAIN_QUEUE.into()
        } else {
            String::new()
        },
        quality: Some(d.quality.clone()).filter(|q| !q.is_empty()),
        referrer: d.url.clone(),
        ..AddOptions::default()
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
