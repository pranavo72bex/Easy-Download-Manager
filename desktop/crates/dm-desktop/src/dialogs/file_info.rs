// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
pub(super) fn file_info(app: &mut DmApp, ctx: &egui::Context) {
    if app.dialogs.file_info.is_none() {
        return;
    }
    // A page that turned out to be a playlist: its videos are listed to be
    // ticked, as a YouTube playlist's are, rather than one download refused.
    let found = app
        .dialogs
        .file_info
        .as_ref()
        .and_then(|d| Some((d.urls.first()?.clone(), dm_core::lock(&d.playlist).take()?)));
    if let Some((url, playlist)) = found {
        app.dialogs.file_info = None;
        let quality = app.applied.stream_quality.clone();
        app.dialogs.open_playlist_read(url, playlist, quality);
        return;
    }
    let focus = app.dialogs.take_focus("Download details");
    begin_early(app);
    let queues = app.queues();
    let Some(d) = app.dialogs.file_info.as_mut() else {
        return;
    };
    let default_dir = app.applied.download_dir.display().to_string();
    // Each kind's folder, and each category a person made, to pick from:
    // a file can go to another kind's folder without typing its path.
    let places: Vec<(String, String)> = {
        // Each kind's own folder, whether or not finished files are sorted
        // into them: picked here, it is asked for (with sorting off every
        // kind named the download folder itself).
        let mut categories = app.applied.categories();
        categories.enabled = true;
        let root = &app.applied.download_dir;
        let mut places: Vec<(String, String)> = dm_core::Category::ALL
            .iter()
            .map(|kind| {
                let word = format::titled(&category_word(*kind));
                (
                    crate::i18n::name(&word),
                    categories.destination(root, *kind).display().to_string(),
                )
            })
            .collect();
        places.extend(app.applied.custom_categories.iter().map(|custom| {
            let name = custom.name.trim().to_string();
            (name.clone(), root.join(&name).display().to_string())
        }));
        places
    };
    // The folder an empty field means: sorted into kinds' folders only
    // where Options › Folders sorts them.
    let default_dir = if app.applied.categorize {
        tf("{} (sorted by kind)", &[&default_dir])
    } else {
        default_dir
    };
    let platform = std::sync::Arc::clone(&app.backend.platform);
    // The server is asked what it calls the file, once, as the dialog
    // opens; Save as had only the address's last part to show.
    if d.probed.is_none() && d.urls.len() == 1 && d.name.is_empty() {
        let slot: std::sync::Arc<std::sync::Mutex<Option<Probed>>> = Default::default();
        let remux = app.applied.remux_ts;
        d.probed = Some(slot.clone());
        let url = d.urls.first().cloned().unwrap_or_default();
        // Not a torrent's: what the server says of a .torrent is the
        // metainfo's few kilobytes and its file name, which Download details
        // showed as the download's.
        if (url.starts_with("http://") || url.starts_with("https://")) && !dm_core::is_torrent(&url)
        {
            // Asked by the application (`lookups::probe_later`).
            let sign_in = dm_core::cookies::SignIn::with_headers(&url, &d.cookies, &d.headers);
            let referrer = d.referrer.clone();
            let ctx = ctx.clone();
            dm_shell::app::lookups::probe_later(
                &app.backend,
                url,
                referrer,
                sign_in,
                remux,
                move |found| {
                    *dm_core::lock(&slot) = Some(found);
                    ctx.request_repaint();
                },
            );
        } else {
            // Nothing to ask: known at once, so nothing waits on it.
            *dm_core::lock(&slot) = Some(Probed::default());
        }
    }
    let probed = d.probed.as_ref().and_then(|p| dm_core::lock(p).clone());
    let server_name = probed.as_ref().and_then(|p| p.name.clone());
    let problem = probed.as_ref().and_then(|p| p.problem.clone());
    let page = probed.as_ref().is_some_and(|p| p.page);
    let size = probed.and_then(|p| p.size);
    let remux = app.applied.remux_ts;
    if let Some(name) = &server_name {
        if !d.name_typed && d.name.is_empty() {
            d.name.clone_from(name);
        }
    }
    // A YouTube video's qualities, each with its size, read once as the
    // dialog opens from its own data (under a second).
    let first = d.urls.first().cloned().unwrap_or_default();
    let youtube = d.urls.len() == 1 && dm_core::youtube::video_id(&first).is_some();
    let video = d.urls.len() == 1 && (page || youtube);
    // A ZIP's files, read once from the end of the archive as the dialog
    // opens, and listed below, so a person sees what the download holds
    // before it is fetched (`zip_list`).
    if d.urls.len() == 1
        && !page
        && zip_list::is_zip_address(&first)
        && (first.starts_with("http://") || first.starts_with("https://"))
    {
        let referrer = Some(d.referrer.clone()).filter(|r| !r.is_empty());
        let sign_in = dm_core::cookies::SignIn::with_headers(&first, &d.cookies, &d.headers);
        zip_list::read_address(
            &app.backend,
            ctx,
            &mut d.zip,
            first.clone(),
            referrer,
            sign_in,
        );
    }
    if video && d.choices.is_none() {
        let slot: std::sync::Arc<std::sync::Mutex<Option<Vec<dm_core::videopage::Choice>>>> =
            Default::default();
        d.choices = Some(slot.clone());
        let url = first.clone();
        let referrer = Some(d.referrer.clone()).filter(|r| !r.is_empty());
        let sign_in = dm_core::cookies::SignIn::with_headers(&url, &d.cookies, &d.headers);
        let ctx = ctx.clone();
        let found = d.playlist.clone();
        // Read by the application (`lookups::list_choices_later`): the
        // qualities, or the videos another site's page lists.
        dm_shell::app::lookups::list_choices_later(
            &app.backend,
            url,
            referrer,
            sign_in,
            youtube,
            move |listed, several| {
                if several.is_some() {
                    *dm_core::lock(&found) = several;
                }
                if let Ok(mut slot) = slot.lock() {
                    *slot = Some(listed);
                }
                ctx.request_repaint();
            },
        );
    }
    let choices: Option<Vec<dm_core::videopage::Choice>> = d
        .choices
        .as_ref()
        .and_then(|slot| slot.lock().ok()?.clone());
    // The quality the setting would choose, shown as picked until one is.
    let setting = dm_core::Quality::named(&app.applied.stream_quality);
    // The folder this kind of file went to last time, where it still is.
    if !d.folder_offered && d.urls.len() == 1 && !d.name.trim().is_empty() {
        d.folder_offered = true;
        if d.folder.trim().is_empty() {
            if let Some(last) = app
                .applied
                .last_folders
                .get(kind_key(d.name.trim()))
                .filter(|f| std::path::Path::new(f).is_dir())
            {
                d.folder = last.clone();
            }
        }
    }
    // The name it will really get where its folder has one of that name
    // already: Save as said `big.zip` and the file came as `big (1).zip`.
    // Where a file taken out of a ZIP goes: beside the archive.
    let archive_folder = folder_for(&app.applied, &d.folder, &first, d.name.trim());
    let clash = (d.urls.len() == 1 && !d.name.trim().is_empty())
        .then(|| {
            let name = d.name.trim();
            let url = d.urls.first().map(String::as_str).unwrap_or_default();
            let dir = folder_for(&app.applied, &d.folder, url, name);
            // Every download of that name still waiting to be placed there
            // takes a free name first (two rows read
            // "big (1).zip" and one landed as "big (2).zip").
            let waiting = app
                .rows
                .iter()
                .filter(|r| {
                    // The row begun at once for this very dialog is this file.
                    Some(r.id) != d.early
                        && !r.state.settled()
                        && r.path.is_empty()
                        && name_of(r) == name
                        && r.save_dir
                            .as_deref()
                            .is_none_or(|s| s.trim().is_empty() || std::path::Path::new(s) == dir)
                })
                .count();
            let free = dm_core::nth_free_name(&dir, name, waiting);
            let taken = dir.join(name);
            if free == name {
                None
            } else if dm_core::free_name(&dir, name) == name {
                // Free on disk: it is the list that has the name.
                Some(Clash::Listed)
            } else if waiting == 0 && taken.is_file() {
                Some(Clash::OnDisk { free, taken })
            } else {
                Some(Clash::Listed)
            }
        })
        .flatten();
    // Only a file of that name can be replaced: offered again for the
    // next name, not carried to it.
    if !matches!(clash, Some(Clash::OnDisk { .. })) {
        d.replace = false;
    }
    let mut open = true;
    let mut chosen: Option<bool> = None; // Some(later)
    let mut cancel = false;
    // No wider than the window: a warning in it wraps rather than pushing
    // Start now off the edge.
    let room = (super::room(ctx).width() - sides(ctx)).clamp(360.0, 600.0);
    window(ctx, "Download details")
        .max_width(room)
        .open(&mut open)
        .show(ctx, |ui| {
            let first = d.urls.first().cloned().unwrap_or_default();
            ui.label(RichText::new(format::shown_url(&first)).weak());
            // Where a handed-over download came from, as the host saw it.
            if let (true, Some(browser)) = (d.handed, &d.browser) {
                ui.label(RichText::new(tf("From {}", &[browser])).weak());
            }
            if d.urls.len() > 1 {
                ui.label(tf("and {} more", &[&(d.urls.len() - 1)]));
            }
            ui.separator();
            egui::Grid::new("file-info")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    let name = ui.label(t("Category"));
                    combo("details-category")
                        .selected_text(
                            places
                                .iter()
                                .find(|(_, path)| *path == d.folder.trim())
                                .map(|(name, _)| name.clone())
                                .unwrap_or_else(|| t("As its type says").to_string()),
                        )
                        .show_ui(ui, |ui| {
                            if option(ui, d.folder.trim().is_empty(), t("As its type says"))
                                .clicked()
                            {
                                d.folder.clear();
                            }
                            for (label, path) in &places {
                                if option(ui, d.folder.trim() == path, label).clicked() {
                                    d.folder.clone_from(path);
                                }
                            }
                        })
                        .response
                        .labelled_by(name.id);
                    ui.end_row();
                    let name = ui.label(t("Save to folder"));
                    ui.horizontal(|ui| {
                        let folder = ui
                            .add(
                                egui::TextEdit::singleline(&mut d.folder)
                                    .hint_text(&default_dir)
                                    .desired_width(280.0),
                            )
                            .labelled_by(name.id);
                        if entered(&folder) {
                            chosen = Some(false);
                        }
                        if let Some(chose) = browse_button(ui, &*platform, &mut d.folder) {
                            d.no_chooser = !chose;
                        }
                    });
                    ui.end_row();
                    // A download the browser caught: the next ones can go
                    // straight to their folder (Options › Browser says so
                    // afterwards).
                    if d.caught {
                        ui.label("");
                        ui.checkbox(&mut d.always_here, t("Always save here without asking"));
                        ui.end_row();
                    }
                    if d.urls.len() == 1 {
                        let name = ui.label(t("Save as"));
                        let save_as = ui
                            .add(
                                egui::TextEdit::singleline(&mut d.name)
                                    .hint_text(if d.quality.is_some() || page {
                                        // A video picked from a player page is
                                        // named from its title: the page's own
                                        // name ("player2.html") was shown, and
                                        // the file came as something else.
                                        t("The video's own title").to_string()
                                    } else if dm_core::is_torrent(&first) {
                                        t("The torrent's own name").to_string()
                                    } else {
                                        stream_name(&first, None, remux)
                                            .unwrap_or_else(|| format::provisional(&first))
                                    })
                                    .desired_width(360.0),
                            )
                            .labelled_by(name.id);
                        if save_as.changed() {
                            d.name_typed = true;
                        }
                        if entered(&save_as) {
                            chosen = Some(false);
                        }
                        ui.end_row();
                    }
                    // A video's qualities, each with its size, to pick from:
                    // the one the setting would take is shown picked until
                    // another is. Sound alone is one of them.
                    if video {
                        ui.label(t("Quality"));
                        match &choices {
                            None => {
                                ui.label(RichText::new(t("Reading the video's qualities…")).weak());
                            }
                            Some(list) if list.is_empty() => {
                                ui.label(RichText::new("—").weak());
                            }
                            Some(list) => {
                                let default = default_choice(list, setting);
                                let picked = d.quality.clone().or(default);
                                // Four to a row, each its height over its
                                // size, as a person compares them.
                                ui.vertical(|ui| {
                                    for row in list.chunks(4) {
                                        ui.horizontal(|ui| {
                                            for choice in row {
                                                let on = picked.as_deref()
                                                    == Some(choice.quality.as_str());
                                                if ui
                                                    .selectable_label(on, chip_words(choice))
                                                    .clicked()
                                                {
                                                    d.quality = Some(choice.quality.clone());
                                                    d.label = Some(choice.label.clone());
                                                }
                                            }
                                        });
                                    }
                                });
                            }
                        }
                        ui.end_row();
                    }
                    // A ZIP's files, by name and size: text only, never opened.
                    // Nothing where the server or the file gave none.
                    match zip_list::listing(&d.zip) {
                        zip_list::Listing::Nothing => {}
                        zip_list::Listing::Reading => {
                            ui.label(t("Contents"));
                            ui.label(RichText::new(zip_list::reading_words()).weak());
                            ui.end_row();
                        }
                        zip_list::Listing::Files(files, more) => {
                            ui.label(t("Contents"));
                            ui.vertical(|ui| {
                                // One file taken out alone, before the
                                // archive is fetched, into the folder the
                                // archive is going to.
                                if let Some(entry) = zip_list::show(ui, &files, more, true) {
                                    let referrer =
                                        Some(d.referrer.clone()).filter(|r| !r.is_empty());
                                    let sign_in = dm_core::cookies::SignIn::with_headers(
                                        &first, &d.cookies, &d.headers,
                                    );
                                    let ctx = ui.ctx().clone();
                                    d.taking = dm_core::sanitize_name(&entry.name);
                                    d.taken = Some(dm_shell::app::files::take_from_zip(
                                        &app.backend,
                                        first.clone(),
                                        referrer,
                                        sign_in,
                                        entry,
                                        archive_folder.clone(),
                                        move || ctx.request_repaint(),
                                    ));
                                }
                                if let Some(words) = zip_list::taken_words(&d.taken, &d.taking) {
                                    ui.label(RichText::new(words).weak());
                                }
                            });
                            ui.end_row();
                        }
                    }
                    // A part of the video, from one moment to another: cut
                    // from the streams dm fetches itself, which today are a
                    // YouTube video's.
                    if youtube || d.handed || d.part {
                        ui.label(t("Part"));
                        let starts = dm_core::youtube::start_of(&first);
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut d.part, t("Only a part of the video"));
                            // A link copied at a moment of the video: that
                            // moment offered as the part's start.
                            if let (false, Some(at)) = (d.part, starts) {
                                let at = clock(at);
                                ui.label(
                                    RichText::new(tf("This link starts at {}.", &[&at])).weak(),
                                );
                                if ui.button(tf("From {}", &[&at])).clicked() {
                                    d.part = true;
                                    d.part_from = at;
                                }
                            }
                            if d.part {
                                ui.label(t("part:from").trim_start_matches("part:"));
                                ui.add(
                                    egui::TextEdit::singleline(&mut d.part_from)
                                        .hint_text(t("m:ss"))
                                        .desired_width(64.0),
                                );
                                ui.label(t("part:to").trim_start_matches("part:"));
                                ui.add(
                                    egui::TextEdit::singleline(&mut d.part_to)
                                        .hint_text(t("m:ss"))
                                        .desired_width(64.0),
                                );
                            }
                        });
                        ui.end_row();
                        // The part's shape: as it is, or the middle of the
                        // picture made vertical or square, nothing re-encoded.
                        if d.part {
                            use dm_core::shape::Shape;
                            ui.label(t("Shape"));
                            ui.horizontal(|ui| {
                                for (shape, words) in [
                                    (None, t("As it is")),
                                    (Some(Shape::Vertical), t("Vertical 9:16")),
                                    (Some(Shape::Square), t("Square 1:1")),
                                ] {
                                    if ui.selectable_label(d.shape == shape, words).clicked() {
                                        d.shape = shape;
                                    }
                                }
                            });
                            ui.end_row();
                            // As a GIF: moving pictures with no sound,
                            // decoded by the system (macOS, Windows) or, on
                            // Linux, by dm from a YouTube video's AV1 stream.
                            if d.urls
                                .first()
                                .is_some_and(|u| dm_core::gif::possible_for(u))
                            {
                                ui.label(t("Part as"));
                                ui.horizontal(|ui| {
                                    if ui.selectable_label(!d.gif, t("Video")).clicked() {
                                        d.gif = false;
                                    }
                                    if ui.selectable_label(d.gif, "GIF").clicked() {
                                        d.gif = true;
                                    }
                                });
                                ui.end_row();
                            }
                        }
                    }
                    // The size, once the server has said it: its
                    // line is there from the start, so the buttons below do
                    // not move when the answer comes.
                    if d.urls.len() == 1 {
                        ui.label(t("Size"));
                        // A video page's size is learned once it starts:
                        // said here, in the row that is always there, so
                        // the dialog does not grow as the answer comes and
                        // move Start now from under the pointer.
                        match size {
                            Some(size) => ui.label(format::bytes(size)),
                            None if page => {
                                ui.label(RichText::new(t("Known once the video starts")).weak())
                            }
                            None => ui.label(RichText::new("—").weak()),
                        };
                        ui.end_row();
                    }
                    let name = ui.label(t("Note"));
                    let note = ui
                        .add(egui::TextEdit::singleline(&mut d.description).desired_width(360.0))
                        .labelled_by(name.id);
                    if entered(&note) {
                        chosen = Some(false);
                    }
                    ui.end_row();
                    // Chosen as it is saved, as the row's menu offers it
                    // after: the file to the Trash that long after it finishes.
                    let times = crate::table::delete_after_times();
                    let name = ui.label(t("Delete file after"));
                    let now = times
                        .iter()
                        .find(|(_, s)| *s == d.delete_after)
                        .map_or(times[0].0, |(label, _)| *label);
                    crate::dialogs::combo("delete-after")
                        .selected_text(now)
                        .show_ui(ui, |ui| {
                            for (label, seconds) in times {
                                ui.selectable_value(&mut d.delete_after, seconds, label);
                            }
                        })
                        .response
                        .labelled_by(name.id);
                    ui.end_row();
                    if let (Some(label), false) = (&d.label, video) {
                        ui.label(t("Quality"));
                        ui.label(label);
                        ui.end_row();
                    }
                });
            // Below the fields and wrapped to the dialog's width: beside
            // Browse… it widened the dialog past a 720 px window.
            if d.no_chooser {
                ui.add(
                    egui::Label::new(
                        RichText::new(t("No folder picker here: type the path.")).color(warn(ui)),
                    )
                    .wrap(),
                );
            }
            match &clash {
                Some(Clash::Listed) => {
                    ui.add(
                        egui::Label::new(
                            RichText::new(t(
                                "A download with this name exists. This one will be numbered.",
                            ))
                            .color(warn(ui)),
                        )
                        .wrap(),
                    );
                }
                Some(Clash::OnDisk { free, .. }) => {
                    ui.add(
                        egui::Label::new(
                            RichText::new(t("A file of that name is in the folder already."))
                                .color(warn(ui)),
                        )
                        .wrap(),
                    );
                    if radio_flag(
                        ui,
                        !d.replace,
                        tf("Save this one as {}", &[&crate::bidi::visual(free)]),
                    )
                    .clicked()
                    {
                        d.replace = false;
                    }
                    if radio_flag(ui, d.replace, t("Replace that file once this one is whole"))
                        .clicked()
                    {
                        d.replace = true;
                    }
                }
                None => {}
            }
            if let Some(why) = &problem {
                let plain = format::reason(why).map(t).unwrap_or(why.as_str());
                ui.add(
                    egui::Label::new(
                        RichText::new(tf("Not available: {}.", &[&plain])).color(warn(ui)),
                    )
                    .wrap(),
                )
                .on_hover_text(why);
            }
            if !d.says.is_empty() {
                ui.add(egui::Label::new(RichText::new(&d.says).color(warn(ui))).wrap());
            }
            footer(ui, |ui| {
                if focused(primary(ui, "Start now"), focus).clicked() || enter_free(ui) {
                    chosen = Some(false);
                }
                // Read as one phrase: "Add for later to Main queue ▾"; the
                // queue's name used to sit between the button and an "into"
                // after it.
                if ui.button(t("Add for later")).clicked() {
                    chosen = Some(true);
                }
                // "to" by itself is many words in other languages: the key
                // says which one.
                let to = ui.label(t("queue:to").trim_start_matches("queue:"));
                combo("details-queue")
                    .selected_text(crate::i18n::name(&d.queue))
                    .show_ui(ui, |ui| {
                        for q in queues.iter() {
                            pick(ui, &mut d.queue, q.clone(), crate::i18n::name(q));
                        }
                    })
                    .response
                    .labelled_by(to.id);
                cancel = ui.button(t("Cancel")).clicked();
            });
        });
    if cancel || !open {
        let site = app
            .dialogs
            .file_info
            .as_ref()
            .filter(|d| d.handed)
            .and_then(|d| d.urls.first())
            .map(|url| dm_core::host_of(url))
            .filter(|host| !host.is_empty());
        let early = app.dialogs.file_info.as_ref().and_then(|d| d.early);
        app.dialogs.file_info = None;
        // What was fetched while it asked goes with it.
        if let Some(id) = early {
            app.backend.remove_later(id);
        }
        if let Some(site) = site {
            app.hand_off_cancelled(&site);
        }
        return;
    }
    let Some(later) = chosen else {
        return;
    };
    // The part's ends, where one was asked for; a pair that is not a span
    // keeps the dialog, with the shape they take.
    if d.part {
        match dm_core::Span::parse(&d.part_from, &d.part_to) {
            Some(span) => d.span = Some(span),
            None => {
                d.says = t("Enter the start and end as minutes and seconds, such as 4:10 and 4:45. The end comes after the start.").to_string();
                return;
            }
        }
    } else {
        d.span = None;
    }
    let with = AddOptions {
        description: d.description.trim().to_string(),
        queue: if later {
            d.queue.clone()
        } else {
            String::new()
        },
        title_hint: None,
        save_dir: Some(d.folder.trim().to_string()).filter(|f| !f.is_empty()),
        // The server's own name, untouched, is what dm names the file
        // anyway: left for it to place as it always does.
        save_name: Some(d.name.trim().to_string())
            .filter(|n| !n.is_empty())
            .filter(|n| d.name_typed || server_name.as_deref() != Some(n.as_str())),
        later,
        referrer: d.referrer.clone(),
        quality: d.quality.clone(),
        sound: d.sound.clone(),
        span: d.span,
        shape: d.shape.filter(|_| d.span.is_some()),
        // An MP3 is asked for by its quality ("audio/mp3").
        mp3: false,
        gif: d.gif && d.span.is_some(),
        delete_after: d.delete_after,
        cookies: d.cookies.clone(),
        cookies_once: d.cookies_once,
        headers: d.headers.clone(),
        // The form for the first address alone, which is the one it was sent to.
        post: d.post.clone(),
        checksum: None,
        speed_limit: 0,
        // Added from Download details, by a person: not a hand-over that
        // may take a waiting row's place.
        fresh_link_ok: false,
        replaces: match &clash {
            Some(Clash::OnDisk { taken, .. }) if d.replace => {
                Some(taken.to_string_lossy().into_owned())
            }
            _ => None,
        },
    };
    let urls = d.urls.clone();
    let always_here = d.caught && d.always_here;
    // Where this kind of file was put, for next time; the kind's own folder
    // chosen again forgets it.
    if urls.len() == 1 && !d.name.trim().is_empty() {
        let kind = kind_key(d.name.trim());
        let folder = Some(d.folder.trim().to_string()).filter(|f| !f.is_empty());
        if app.applied.last_folders.get(kind) != folder.as_ref() {
            match app.backend.remember_folder(kind, folder.as_deref()) {
                Ok(now) => app.applied.last_folders = now.last_folders,
                Err(e) => app.said = e.into(),
            }
        }
    }
    // "Always save here without asking": caught downloads go to their
    // folder unasked from now on.
    if always_here {
        match app.backend.set_ask_caught_downloads(false) {
            Ok(now) => app.applied.ask_caught_downloads = now.ask_caught_downloads,
            Err(e) => app.said = e.into(),
        }
    }
    // The size the dialog showed stays on the row until the row learns its
    // own: a row added for later had none to show.
    let sizes: HashMap<String, u64> = urls
        .first()
        .zip(size)
        .filter(|_| urls.len() == 1)
        .map(|(url, size)| (url.clone(), size))
        .into_iter()
        .collect();
    let handed = d.handed;
    // Begun while it asked: placed where it was asked to go, not added again.
    if let Some(id) = d.early {
        let dir = with.save_dir.clone().map(std::path::PathBuf::from);
        let placed = app.backend.settle_early(
            id,
            dir,
            with.save_name.clone(),
            &with.description,
            &with.queue,
            later,
        );
        app.dialogs.file_info = None;
        if let Err(e) = placed {
            app.said = e.into();
        }
        if handed {
            for url in &urls {
                app.taken_from_browser(url);
            }
        }
        app.refresh();
        return;
    }
    match app.add_sized(&urls, &with, &sizes) {
        Ok(_) => {
            app.dialogs.file_info = None;
            if handed {
                for url in &urls {
                    app.taken_from_browser(url);
                }
            }
            // Start now while the schedule or the download limit holds
            // downloads: asked, as Resume asks, whether to start it outside
            // the schedule; it only waited, without a word.
            // Asked of the schedule itself: the list, not yet refreshed,
            // had no row waiting on it, so nothing asked.
            let shut = app.applied.schedule.enabled && !app.applied.schedule.open();
            let spent = app.backend.allowance_spent().is_some();
            if !later && !app.headless && (shut || spent) {
                app.dialogs.confirm_outside = Some(app.last_added.clone());
            }
        }
        Err(e) => {
            if let Some(d) = app.dialogs.file_info.as_mut() {
                d.says = e;
            }
        }
    }
}

/// Starts the dialog's one file at once, where Options asks for that (reference A's "start
/// downloading while the dialog is open"; gaps row 85): a plain file, not a video page, a
/// torrent, a form's answer or several addresses, which wait for the person as before.
fn begin_early(app: &mut DmApp) {
    if !app.applied.start_while_asking {
        return;
    }
    let Some(d) = app.dialogs.file_info.as_mut() else {
        return;
    };
    if d.early_tried {
        return;
    }
    d.early_tried = true;
    let [url] = d.urls.as_slice() else {
        return;
    };
    let plain = (url.starts_with("https://") || url.starts_with("http://"))
        && !dm_core::is_torrent(url)
        && d.choices.is_none()
        && d.quality.is_none()
        && d.post.is_none();
    if !plain {
        return;
    }
    let with = AddOptions {
        referrer: d.referrer.clone(),
        cookies: d.cookies.clone(),
        cookies_once: d.cookies_once,
        headers: d.headers.clone(),
        ..AddOptions::default()
    };
    let url = url.clone();
    match app.backend.add_with(&url, with) {
        Ok(id) => {
            if let Some(d) = app.dialogs.file_info.as_mut() {
                d.early = Some(id);
            }
            app.refresh();
        }
        Err(e) => app.said = e.into(),
    }
}

/// The choice the quality setting would take from a page's list (tallest
/// first, the sound alone last): the tallest at or under the height asked,
/// the first for "best", the sound for "audio"; `None` for an empty list.
fn default_choice(
    list: &[dm_core::videopage::Choice],
    setting: dm_core::Quality,
) -> Option<String> {
    use dm_core::Quality;
    let height = |choice: &dm_core::videopage::Choice| {
        choice
            .quality
            .split('/')
            .next()
            .and_then(|h| h.parse::<u32>().ok())
    };
    let chosen = match setting {
        Quality::Audio => list.iter().find(|c| c.quality.starts_with("audio")),
        Quality::UpTo(limit) => list
            .iter()
            .find(|c| height(c).is_some_and(|h| h <= limit))
            .or_else(|| list.iter().find(|c| height(c).is_some())),
        Quality::Best => list.iter().find(|c| height(c).is_some()),
    };
    chosen.or(list.first()).map(|c| c.quality.clone())
}

/// A quality's chip: its height and container over its size ("1080p MP4"
/// over "275.0 MB"), the sound alone said so.
fn chip_words(choice: &dm_core::videopage::Choice) -> String {
    let (head, ext) = choice
        .quality
        .split_once('/')
        .unwrap_or((&choice.quality, ""));
    let top = if head == "audio" {
        format!("{} {}", t("Sound only"), ext.to_ascii_uppercase())
    } else if let Ok(side) = head.parse::<u32>() {
        // By its class, as the panel's list says it: "4K MP4", "1080p MP4".
        format!(
            "{} {}",
            dm_core::videopage::class_name(0, side),
            ext.to_ascii_uppercase()
        )
    } else {
        choice.label.clone()
    };
    match choice.size {
        Some(size) => format!("{}\n{}", top.trim(), format::bytes(size)),
        None => top.trim().to_string(),
    }
}

/// Seconds as a person writes a moment of a video: `0:34`, `12:05`,
/// `1:02:03`.
fn clock(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// The folder a download of `name` from `url` is saved in, as the dialog
/// stands: the folder typed, else the site rule's, else its category's, else
/// the download folder.
fn folder_for(applied: &Settings, typed: &str, url: &str, name: &str) -> std::path::PathBuf {
    if !typed.trim().is_empty() {
        std::path::PathBuf::from(typed.trim())
    } else if let Some(rule) = applied
        .site_rule_for(url)
        .filter(|rule| !rule.folder.is_empty())
    {
        // The site's own folder, as its rule says.
        std::path::PathBuf::from(&rule.folder)
    } else {
        let mut categories = applied.categories();
        if categories.enabled {
            categories.site = dm_core::host_of(url);
            let kind = dm_core::by_name(name).unwrap_or(dm_core::Category::Other);
            categories.place(&applied.download_dir, kind, name)
        } else {
            applied.download_dir.clone()
        }
    }
}

#[cfg(test)]
mod clock_tests {
    #[test]
    fn a_moment_reads_as_a_player_shows_it() {
        assert_eq!(super::clock(34), "0:34");
        assert_eq!(super::clock(725), "12:05");
        assert_eq!(super::clock(3723), "1:02:03");
    }
}

#[cfg(test)]
mod choice_tests {
    use super::{chip_words, default_choice};
    use dm_core::videopage::Choice;
    use dm_core::Quality;

    fn choice(quality: &str, label: &str, size: Option<u64>) -> Choice {
        Choice {
            label: label.into(),
            quality: quality.into(),
            size,
        }
    }

    fn list() -> Vec<Choice> {
        vec![
            choice("2160/mp4", "MP4 3840×2160", Some(900_000_000)),
            choice("1080/mp4", "MP4 1920×1080", Some(275_000_000)),
            choice("480/webm", "WebM 854×480", None),
            choice("audio/m4a", "M4A audio", Some(4_000_000)),
        ]
    }

    #[test]
    fn the_setting_picks_the_tallest_it_allows_or_the_sound() {
        let list = list();
        let pick = |q| default_choice(&list, q);
        assert_eq!(pick(Quality::Best).as_deref(), Some("2160/mp4"));
        assert_eq!(pick(Quality::UpTo(1080)).as_deref(), Some("1080/mp4"));
        assert_eq!(pick(Quality::UpTo(720)).as_deref(), Some("480/webm"));
        // Nothing that short: the tallest there is rather than nothing.
        assert_eq!(pick(Quality::UpTo(144)).as_deref(), Some("2160/mp4"));
        assert_eq!(pick(Quality::Audio).as_deref(), Some("audio/m4a"));
        assert_eq!(default_choice(&[], Quality::Best), None);
        // A list of only labels: its first.
        let odd = [choice("hls", "Stream", None)];
        assert_eq!(default_choice(&odd, Quality::Audio).as_deref(), Some("hls"));
        assert_eq!(
            default_choice(&odd, Quality::UpTo(720)).as_deref(),
            Some("hls")
        );
    }

    #[test]
    fn a_chip_says_the_class_and_container_over_the_size() {
        let chip = chip_words(&choice("1080/mp4", "MP4 1920×1080", Some(2048)));
        assert!(chip.starts_with("1080p"), "{chip}");
        assert!(chip.contains("MP4\n"), "{chip}");
        assert!(chip.ends_with(&crate::format::bytes(2048)), "{chip}");
        let sound = chip_words(&choice("audio/m4a", "M4A audio", None));
        assert!(sound.ends_with("M4A"), "{sound}");
        assert!(!sound.contains('\n'), "no size, no second line: {sound}");
        // Not a height: the list's own words.
        assert_eq!(
            chip_words(&choice("hls", "Live stream", None)),
            "Live stream"
        );
    }
}
