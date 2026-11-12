// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use dm_desktop_base::file_info;
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
    // Each kind's folder, and each category a person made, to pick from.
    let places = file_info::places(&app.applied);
    let default_dir = file_info::default_folder_words(&app.applied);
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
            if let Some(last) = file_info::last_folder(&app.applied, &d.name) {
                d.folder = last;
            }
        }
    }
    // The name it will really get where its folder has one of that name
    // already: Save as said `big.zip` and the file came as `big (1).zip`.
    // Where a file taken out of a ZIP goes: beside the archive.
    let archive_folder = file_info::folder_for(&app.applied, &d.folder, &first, d.name.trim());
    let clash = (d.urls.len() == 1)
        .then(|| {
            let url = d.urls.first().map(String::as_str).unwrap_or_default();
            file_info::clash(&app.applied, &app.rows, &d.folder, url, &d.name, d.early)
        })
        .flatten();
    // Only a file of that name can be replaced: offered again for the
    // next name, not carried to it.
    if !matches!(clash, Some(file_info::Clash::OnDisk { .. })) {
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
                                    .hint_text(file_info::name_hint(
                                        &first,
                                        d.quality.is_some() || page,
                                        remux,
                                    ))
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
                                let default = file_info::default_choice(list, setting);
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
                                                    .selectable_label(
                                                        on,
                                                        file_info::chip_words(choice),
                                                    )
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
                                let at = file_info::clock(at);
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
                    let times = file_info::delete_after_times();
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
                Some(file_info::Clash::Listed) => {
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
                Some(file_info::Clash::OnDisk { free, .. }) => {
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
                ui.add(
                    egui::Label::new(RichText::new(file_info::problem_words(why)).color(warn(ui)))
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
    match file_info::span_of(d.part, &d.part_from, &d.part_to) {
        Ok(span) => d.span = span,
        Err(words) => {
            d.says = words.to_string();
            return;
        }
    }
    let form = file_info::Form {
        referrer: d.referrer.clone(),
        quality: d.quality.clone(),
        sound: d.sound.clone(),
        span: d.span,
        shape: d.shape,
        gif: d.gif,
        delete_after: d.delete_after,
        folder: d.folder.clone(),
        name: d.name.clone(),
        name_typed: d.name_typed,
        description: d.description.clone(),
        queue: d.queue.clone(),
        cookies: d.cookies.clone(),
        cookies_once: d.cookies_once,
        headers: d.headers.clone(),
        post: d.post.clone(),
        replace: d.replace,
    };
    let with = file_info::options(&form, later, server_name.as_deref(), clash.as_ref());
    let urls = d.urls.clone();
    let always_here = d.caught && d.always_here;
    // Where this kind of file was put, for next time; the kind's own folder
    // chosen again forgets it.
    if urls.len() == 1 && !d.name.trim().is_empty() {
        let kind = file_info::kind_key(d.name.trim());
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
    let form = file_info::Form {
        quality: d.quality.clone(),
        post: d.post.clone(),
        ..file_info::Form::default()
    };
    if !file_info::begins_early(&app.applied, &d.urls, &form, d.choices.is_some()) {
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
