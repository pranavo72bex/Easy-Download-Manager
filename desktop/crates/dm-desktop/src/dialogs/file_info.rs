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
            let client = dm_core::proxy::client_with(&app.backend.settings().proxy)
                .unwrap_or_else(|_| dm_core::client());
            let sign_in = dm_core::cookies::SignIn::new(&url, &d.cookies);
            let referrer = d.referrer.clone();
            let ctx = ctx.clone();
            app.backend.runtime.spawn(async move {
                let asked = tokio::time::timeout(
                    std::time::Duration::from_secs(20),
                    dm_core::probe::probe(
                        &client,
                        &url,
                        Some(&referrer)
                            .filter(|r| !r.is_empty())
                            .map(String::as_str),
                        sign_in.as_ref(),
                    ),
                )
                .await;
                // A web page's name ("watch") and size are no file's: left
                // unsaid. A stream's are its file's, not its playlist's.
                if let Ok(Ok(probe)) = asked {
                    let kind = probe.content_type.as_deref();
                    let found = if let Some(name) = stream_name(&url, kind, remux) {
                        Probed {
                            name: Some(name),
                            ..Probed::default()
                        }
                    } else if dm_core::ytdlp::is_page(kind) {
                        Probed {
                            page: !dm_core::download::names_a_document(&url),
                            ..Probed::default()
                        }
                    } else {
                        Probed {
                            name: Some(probe.filename).filter(|n| !n.is_empty()),
                            size: probe.size,
                            ..Probed::default()
                        }
                    };
                    *dm_core::lock(&slot) = Some(found);
                    ctx.request_repaint();
                } else if let Ok(Err(e)) = asked {
                    // A watch page that turned dm's quick look away (a 429
                    // bot check, a 403) is the video helper's to read when
                    // the download starts, as the engine does: no
                    // "Not available" and the bot page's text.
                    let refused_page = matches!(
                        e,
                        dm_core::Error::Refused {
                            status: 403 | 429,
                            ..
                        }
                    ) && !dm_core::download::names_a_document(&url);
                    *dm_core::lock(&slot) = Some(if refused_page {
                        Probed {
                            page: true,
                            ..Probed::default()
                        }
                    } else {
                        Probed {
                            problem: Some(e.to_string()),
                            ..Probed::default()
                        }
                    });
                    ctx.request_repaint();
                }
            });
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
            d.name = name.clone();
        }
    }
    // A video page's qualities, each with its size, read once as the
    // dialog opens: a YouTube watch page's from its own data (under a
    // second), any other's from yt-dlp's reading of it.
    let first = d.urls.first().cloned().unwrap_or_default();
    let youtube = d.urls.len() == 1 && dm_core::youtube::video_id(&first).is_some();
    let video = d.urls.len() == 1 && (page || youtube);
    if video && d.choices.is_none() {
        let slot: std::sync::Arc<std::sync::Mutex<Option<Vec<dm_core::ytdlp::Choice>>>> =
            Default::default();
        d.choices = Some(slot.clone());
        let url = first.clone();
        let referrer = Some(d.referrer.clone()).filter(|r| !r.is_empty());
        let proxy = app.backend.settings().proxy.url_for_tools();
        let sign_in = dm_core::cookies::SignIn::new(&url, &d.cookies);
        let ctx = ctx.clone();
        let found = d.playlist.clone();
        app.backend.runtime.spawn(async move {
            let listed = tokio::time::timeout(
                std::time::Duration::from_secs(40),
                dm_core::ytdlp::list_choices(
                    &url,
                    referrer.as_deref(),
                    proxy.as_deref(),
                    sign_in.as_ref(),
                ),
            )
            .await;
            let listed = match listed {
                Ok(Ok(list)) => list,
                _ => Vec::new(),
            };
            // No qualities from another site's page: perhaps it lists
            // several videos (a channel, a showcase, an album).
            if listed.is_empty() && !youtube {
                let read = dm_core::ytdlp::read_playlist(
                    &url,
                    referrer.as_deref(),
                    proxy.as_deref(),
                    sign_in.as_ref(),
                );
                if let Ok(Ok(Some(playlist))) =
                    tokio::time::timeout(std::time::Duration::from_secs(40), read).await
                {
                    if playlist.entries.len() > 1 {
                        *dm_core::lock(&found) = Some(playlist);
                    }
                }
            }
            if let Ok(mut slot) = slot.lock() {
                *slot = Some(listed);
            }
            ctx.request_repaint();
        });
    }
    let choices: Option<Vec<dm_core::ytdlp::Choice>> = d
        .choices
        .as_ref()
        .and_then(|slot| slot.lock().ok()?.clone());
    // The quality the setting would choose, shown as picked until one is.
    let setting = dm_core::stream::Quality::named(&app.applied.stream_quality);
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
    let clash = (d.urls.len() == 1 && !d.name.trim().is_empty())
        .then(|| {
            let name = d.name.trim();
            let url = d.urls.first().map(String::as_str).unwrap_or_default();
            let dir = if !d.folder.trim().is_empty() {
                std::path::PathBuf::from(d.folder.trim())
            } else if let Some(rule) = app
                .applied
                .site_rule_for(url)
                .filter(|rule| !rule.folder.is_empty())
            {
                // The site's own folder, as its rule says.
                std::path::PathBuf::from(&rule.folder)
            } else {
                let mut categories = app.applied.categories();
                if categories.enabled {
                    categories.site = dm_core::category::host_of(url);
                    let kind = dm_core::category::by_name(name).unwrap_or(dm_core::Category::Other);
                    categories.place(&app.applied.download_dir, kind, name)
                } else {
                    app.applied.download_dir.clone()
                }
            };
            // Every download of that name still waiting to be placed there
            // takes a free name first (two rows read
            // "big (1).zip" and one landed as "big (2).zip").
            let waiting = app
                .rows
                .iter()
                .filter(|r| {
                    !r.state.settled()
                        && r.path.is_empty()
                        && name_of(r) == name
                        && r.save_dir
                            .as_deref()
                            .is_none_or(|s| s.trim().is_empty() || std::path::Path::new(s) == dir)
                })
                .count();
            let free = dm_core::download::nth_free_name(&dir, name, waiting);
            let taken = dir.join(name);
            if free == name {
                None
            } else if dm_core::download::free_name(&dir, name) == name {
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
    let room = (ctx.content_rect().width() - sides(ctx)).clamp(360.0, 600.0);
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
                                    d.folder = path.clone();
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
                    // A part of the video, from one moment to another: cut
                    // from the streams dm fetches itself, which today are a
                    // YouTube video's.
                    if youtube {
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
                        for q in &queues {
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
            .map(|url| dm_core::category::host_of(url))
            .filter(|host| !host.is_empty());
        app.dialogs.file_info = None;
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
        delete_after: None,
        cookies: d.cookies.clone(),
        cookies_once: d.cookies_once,
        checksum: None,
        speed_limit: 0,
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
            match app.backend.remember_folder(kind, folder) {
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
    match app.add_sized(&urls, with, &sizes) {
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
            let spent = app.backend.queue.allowance_spent().is_some();
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

/// The choice the quality setting would take from a page's list (tallest
/// first, the sound alone last): the tallest at or under the height asked,
/// the first for "best", the sound for "audio"; `None` for an empty list.
fn default_choice(
    list: &[dm_core::ytdlp::Choice],
    setting: dm_core::stream::Quality,
) -> Option<String> {
    use dm_core::stream::Quality;
    let height = |choice: &dm_core::ytdlp::Choice| {
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
fn chip_words(choice: &dm_core::ytdlp::Choice) -> String {
    let (head, ext) = choice
        .quality
        .split_once('/')
        .unwrap_or((&choice.quality, ""));
    let top = if head == "audio" {
        format!("{} {}", t("Sound only"), ext.to_ascii_uppercase())
    } else if head.chars().all(|c| c.is_ascii_digit()) {
        format!("{head}p {}", ext.to_ascii_uppercase())
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

#[cfg(test)]
mod clock_tests {
    #[test]
    fn a_moment_reads_as_a_player_shows_it() {
        assert_eq!(super::clock(34), "0:34");
        assert_eq!(super::clock(725), "12:05");
        assert_eq!(super::clock(3723), "1:02:03");
    }
}
