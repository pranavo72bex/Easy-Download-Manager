use super::*;
/// An address without its login, for telling two rows apart.
pub(crate) fn bare(url: &str) -> String {
    dm_shell::lists::bare(url)
}

/// Asked when a list being imported holds addresses already in the list,
/// as Add URL asks of one address: once, for all of them.
pub(super) fn import_ask(app: &mut DmApp, ctx: &egui::Context) {
    let Some(ask) = app.dialogs.import_ask.as_ref() else {
        return;
    };
    let (already, total) = (ask.already, ask.total);
    let (mut skip, mut again, mut cancel) = (false, false, false);
    window(ctx, "Some are already in the list").show(ctx, |ui| {
        ui.label(crate::i18n::tn(
            "{} of the {} addresses in this list is already in dm's list.||{} of the {} addresses in this list are already in dm's list.",
            already as u64,
            &[&already, &total],
        ));
        footer(ui, |ui| {
            skip = primary(ui, "Skip them").clicked() || enter_free(ui);
            again = ui.button(t("Add them again")).clicked();
            cancel = ui.button(t("Cancel")).clicked();
        });
    });
    if skip || again {
        if let Some(ask) = app.dialogs.import_ask.take() {
            app.import_text(&ask.text, skip);
        }
    } else if cancel {
        app.dialogs.import_ask = None;
    }
}

/// Import or Export's file, typed where the desktop has no file chooser:
/// both did nothing there, without a word.
pub(super) fn list_file(app: &mut DmApp, ctx: &egui::Context) {
    let focus = app.dialogs.take_focus("List file");
    let Some(d) = app.dialogs.list_file.as_mut() else {
        return;
    };
    let mut open = true;
    let (mut ok, mut cancel) = (false, false);
    let dialog = if d.save {
        window(ctx, "Export the list")
    } else {
        window(ctx, "Import a list")
    };
    dialog.open(&mut open).show(ctx, |ui| {
        ui.label(t("No file picker here: type the path."));
        let path = focused(field(ui, t("File's path"), &mut d.path, 420.0), focus);
        ok |= entered(&path);
        if !d.says.is_empty() {
            ui.colored_label(warn(ui), &d.says);
        }
        footer(ui, |ui| {
            ok |= primary(ui, "OK").clicked() || enter_free(ui);
            cancel = ui.button(t("Cancel")).clicked();
        });
    });
    if cancel || !open {
        app.dialogs.list_file = None;
        return;
    }
    if !ok {
        return;
    }
    let typed = d.path.trim();
    // The home folder as the settings find it, on Windows too, which has
    // no HOME (CI, 0.2.0).
    let path = dm_shell::settings::full_path(typed).unwrap_or_else(|| typed.into());
    if typed.is_empty() || !path.is_absolute() {
        d.says = tf(
            "Type the file's full path, such as {}.",
            &[&default_list_path(&app.applied).display()],
        );
        return;
    }
    let save = d.save;
    let done = if save {
        app.export_to(&path)
    } else {
        app.import_from(&path)
    };
    // Kept open with what went wrong, and the path as typed: closed, the
    // path was lost and further typing went to the list.
    match done {
        Ok(()) => app.dialogs.list_file = None,
        Err(why) => {
            if let Some(d) = app.dialogs.list_file.as_mut() {
                d.says = why;
            }
            // The path field keeps the keyboard, to be put right (typing went nowhere).
            app.dialogs.focus_on("List file");
        }
    }
}

/// Sites as the rules match them, by host name: an address pasted whole,
/// scheme and path and all, was kept as it was and matched nothing.
/// A bare name, or one with a leading dot, is kept.
pub fn sites(words: &[String]) -> Vec<String> {
    words
        .iter()
        .map(|w| {
            let host = if w.contains("://") {
                url::Url::parse(w)
                    .ok()
                    .and_then(|u| u.host_str().map(str::to_string))
            } else {
                // "example.com/path" or "example.com:8080": the name alone.
                w.split(['/', ':']).next().map(str::to_string)
            };
            host.filter(|h| !h.is_empty())
                .unwrap_or_else(|| w.clone())
                .to_ascii_lowercase()
        })
        .collect()
}

/// The `file://` address of a `.torrent` on this computer named by `text`,
/// its path (`~` for the home folder) or that address itself, if the file
/// is there.
pub fn local_torrent(text: &str) -> Option<String> {
    let text = text.trim();
    let path = match dm_core::torrent::local_file(text) {
        Some(path) => path,
        None if !text.contains("://") => dm_shell::settings::full_path(text)?,
        None => return None,
    };
    if !dm_core::is_torrent(&path.to_string_lossy()) || !path.is_file() {
        return None;
    }
    dm_core::torrent::file_url(&path)
}

/// Where Export offers to write the list when its path is typed.
pub fn default_list_path(settings: &Settings) -> std::path::PathBuf {
    settings.download_dir.join("downloads.dmlist")
}

/// The row already holding `address`, if one does and was not cancelled.
pub(crate) fn already_listed(app: &DmApp, address: &str) -> Option<Id> {
    let wanted = bare(address);
    app.rows()
        .iter()
        .find(|r| r.state != State::Cancelled && bare(&r.url) == wanted)
        .map(|r| r.id)
}

/// The row a caught download of `address` would duplicate: one listed and
/// still worth keeping. A failed row, or a finished one whose file has gone
/// from the disk, is not: the person clicking the link again wants it.
pub(crate) fn worth_keeping(app: &DmApp, address: &str) -> Option<Id> {
    let wanted = bare(address);
    app.rows()
        .iter()
        .find(|r| {
            r.state != State::Cancelled
                && !matches!(r.state, State::Failed(_))
                && bare(&r.url) == wanted
                && (r.state != State::Completed
                    || (!r.path.is_empty() && std::path::Path::new(&r.path).exists()))
        })
        .map(|r| r.id)
}

/// Asked when an address typed in is already a row: download it again,
/// show the row, or leave it.
pub(super) fn duplicate(app: &mut DmApp, ctx: &egui::Context) {
    let Some((address, id)) = app.dialogs.duplicate.clone() else {
        return;
    };
    let status = app.row(id).map(crate::app::status_of).unwrap_or_default();
    let (mut again, mut show, mut cancel) = (false, false, false);
    window(ctx, "Already in the list").show(ctx, |ui| {
        ui.label(RichText::new(format::shown_url(&address)).weak());
        ui.label(tf("This address is already in the list ({}).", &[&status]));
        footer(ui, |ui| {
            // Enter presses it, as in every other dialog.
            // Enter shows the row, which changes nothing: an Enter pressed
            // for Download details, a moment before this asked, fetched the
            // file a second time.
            show = primary(ui, "Show it").clicked() || enter_free(ui);
            again = ui.button(t("Download again")).clicked();
            cancel = ui.button(t("Cancel")).clicked();
        });
    });
    if again {
        app.dialogs.duplicate = None;
        app.dialogs.open_file_info(vec![address]);
    } else if show {
        app.dialogs.duplicate = None;
        app.selected.clear();
        app.selected.insert(id);
    } else if cancel {
        app.dialogs.duplicate = None;
    }
}

pub(super) fn add_url(app: &mut DmApp, ctx: &egui::Context) {
    let focus = app.dialogs.take_focus("Add URL");
    let recent = app.applied.recent_urls.clone();
    let Some(d) = app.dialogs.add_url.as_mut() else {
        return;
    };
    let mut open = true;
    let (mut ok, mut cancel, mut forget) = (false, false, false);
    window(ctx, "Add URL").open(&mut open).show(ctx, |ui| {
        // As wide as the window allows: at 200% text a fixed 420 ran the
        // dialog past the window's edge.
        let wide = (ctx.content_rect().width() - 160.0).clamp(200.0, 420.0);
        let address = focused(field(ui, t("Address"), &mut d.address, wide), focus);
        // What was said of the last address goes when it is changed: the
        // error stayed under a good one.
        if address.changed() {
            d.says.clear();
        }
        // The link it was opened with is selected, so a link pasted or
        // typed replaces it rather than joining it.
        select_on_focus(ui, &address, &d.address);
        ok |= entered(&address);
        // Addresses added here before, to pick again, and a way to forget
        // them.
        if !recent.is_empty() {
            ui.horizontal(|ui| {
                let label = ui.label(t("Recent"));
                // As wide as the address beside it allows: a fixed 320 with
                // Clear history beside it ran past a narrow window.
                combo("add-url-recent")
                    .selected_text("")
                    .width((wide - 160.0).max(120.0))
                    .show_ui(ui, |ui| {
                        for url in &recent {
                            if ui.selectable_label(false, format::shown_url(url)).clicked() {
                                d.address = url.clone();
                            }
                        }
                    })
                    .response
                    .labelled_by(label.id);
                forget = ui.button(t("Clear history")).clicked();
            });
        }
        toggle_row(ui, &mut d.authorize, t("Needs a sign-in"));
        ui.add_enabled_ui(d.authorize, |ui| {
            ui.horizontal(|ui| {
                ok |= entered(&field(ui, t("Login"), &mut d.login, 140.0));
                let name = ui.label(t("Password"));
                let password = ui
                    .add(
                        egui::TextEdit::singleline(&mut d.password)
                            .password(true)
                            .desired_width(140.0),
                    )
                    .labelled_by(name.id);
                ok |= entered(&password);
            });
        });
        warning_line(ui, &d.says);
        footer(ui, |ui| {
            ok |= primary(ui, "OK").clicked() || enter_free(ui);
            cancel = ui.button(t("Cancel")).clicked();
        });
    });
    if forget {
        match app.backend.remember_urls(Vec::new()) {
            Ok(now) => app.applied.recent_urls = now.recent_urls,
            Err(e) => app.said = e.into(),
        }
        return;
    }
    if cancel || !open {
        app.dialogs.add_url = None;
        return;
    }
    if !ok {
        return;
    }
    let mut address = d.address.trim().to_string();
    // A .torrent on this computer, by its path or its file:// address: it
    // could only be dragged in, and Add URL called it no link.
    if let Some(local) = local_torrent(&address) {
        address = local;
    }
    if crate::app::looks_downloadable(&address).is_none() && local_torrent(&address).is_none() {
        // Nothing typed is asked for, not called a bad link.
        d.says = if address.is_empty() {
            t("Type or paste the address of what to download.").into()
        } else {
            t("That isn't a link dm can download. Paste a web address like https://example.com/file.zip.").into()
        };
        // The address keeps the keyboard, to be put right: after the
        // warning, typing went nowhere.
        app.dialogs.focus_on("Add URL");
        return;
    }
    // Several links pasted one after another (a list copied from a page or a
    // chat, a line each): a batch, in one Download details. Run together with
    // nothing between them, they are still asked about, since where one ends
    // is a guess.
    if crate::app::two_addresses(&address) {
        let found = crate::app::links_in(&address);
        if found.len() > 1 && found.len() == address.split_whitespace().count() {
            app.dialogs.add_url = None;
            app.dialogs.open_file_info(found);
            return;
        }
    }
    if crate::app::two_addresses(&address) {
        d.says =
            t("That looks like two links. Add one at a time, or use Add links from clipboard.")
                .into();
        // The address keeps the keyboard, to be put right: after the
        // warning, typing went nowhere.
        app.dialogs.focus_on("Add URL");
        return;
    }
    if d.authorize && !d.login.is_empty() {
        if let Ok(mut url) = url::Url::parse(&address) {
            let _ = url.set_username(&d.login);
            let _ = url.set_password(Some(&d.password));
            address = url.to_string();
        }
    }
    app.dialogs.add_url = None;
    // Remembered for Add URL's list, without any login.
    let remembered = bare(&address);
    if app.applied.recent_urls.first() != Some(&remembered) {
        let mut recent = app.applied.recent_urls.clone();
        recent.retain(|u| *u != remembered);
        recent.insert(0, remembered);
        match app.backend.remember_urls(recent) {
            Ok(now) => app.applied.recent_urls = now.recent_urls,
            Err(e) => app.said = e.into(),
        }
    }
    // Already a row: asked, rather than a second row nobody meant.
    if let Some(id) = already_listed(app, &address) {
        app.dialogs.duplicate = Some((address, id));
        return;
    }
    // A playlist is its videos, each a download: listed to be ticked.
    if dm_core::youtube::playlist_id(&address).is_some() {
        let quality = app.applied.stream_quality.clone();
        app.dialogs.open_playlist(address, &app.backend, quality);
        return;
    }
    app.dialogs.open_file_info(vec![address]);
}

#[cfg(test)]
mod sites_tests {
    /// A site pasted as an address is kept by its host name,
    /// which is what the rules match.
    #[test]
    fn sites_are_kept_by_their_host_name() {
        let given: Vec<String> = [
            "https://ads.example.com/path",
            "tracker.example:8080",
            "example.org/files",
            "plain.example",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            super::sites(&given),
            [
                "ads.example.com",
                "tracker.example",
                "example.org",
                "plain.example"
            ]
        );
    }
}
