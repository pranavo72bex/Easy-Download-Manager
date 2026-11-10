// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use dm_shell::app::rules::GoOn;
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

/// Sites as the rules match them, by host name (`dm_desktop_base::options`,
/// both windows).
pub use dm_desktop_base::options::sites;

/// A `.torrent` on this computer named by a path or `file://` address
/// (`dm_desktop_base::add_url`).
pub use dm_desktop_base::add_url::local_torrent;

/// Where Export offers to write the list when its path is typed.
pub fn default_list_path(settings: &Settings) -> std::path::PathBuf {
    settings.download_dir.join("downloads.dmlist")
}

/// How an address already in the list is answered (`dm_shell::app::rules::Dup`).
pub(crate) use dm_shell::app::rules::Dup;

/// Carries out an answer: asked, or remembered (what it does is dm's,
/// `dm_desktop_base::add_url::carry`, as every window's).
pub(crate) fn answer_duplicate(app: &mut DmApp, address: String, id: Id, how: Dup) {
    use dm_desktop_base::add_url::Carried;
    match dm_desktop_base::add_url::carry(app.rows(), address, id, how) {
        Carried::Show { id, go_on } => {
            app.selected.clear();
            app.selected.insert(id);
            match go_on {
                Some(GoOn::Retry) => app.backend.retry(id),
                Some(GoOn::Resume) => app.backend.resume(id),
                _ => {}
            }
        }
        Carried::Details(address) => app.dialogs.open_file_info(vec![address]),
        Carried::Replace { address, with } => {
            if let Err(e) = app.add(&[address], *with) {
                app.said = e.into();
            }
        }
    }
}

/// Asked when an address typed in is already a row: show it (or go on with
/// it), download it again, replace the old file, or leave it.
pub(super) fn duplicate(app: &mut DmApp, ctx: &egui::Context) {
    let Some((address, id)) = app.dialogs.duplicate.clone() else {
        return;
    };
    let status = app.row(id).map(crate::app::status_of).unwrap_or_default();
    let (go_on, can_replace) = dm_desktop_base::add_url::duplicate_offers(app.rows(), id);
    let tick = egui::Id::unique("duplicate-remember");
    let mut remember = ctx.data(|d| d.get_temp::<bool>(tick)).unwrap_or(false);
    let (mut again, mut show, mut replace, mut cancel) = (false, false, false, false);
    window(ctx, "Already in the list").show(ctx, |ui| {
        ui.label(RichText::new(format::shown_url(&address)).weak());
        ui.label(tf("This address is already in the list ({}).", &[&status]));
        toggle_row(ui, &mut remember, t("Remember my choice"));
        footer(ui, |ui| {
            // Enter presses it, as in every other dialog.
            // Enter shows the row, which changes nothing: an Enter pressed
            // for Download details, a moment before this asked, fetched the
            // file a second time.
            show = if go_on {
                primary(ui, "Resume it").clicked()
            } else {
                primary(ui, "Show it").clicked()
            } || enter_free(ui);
            again = ui.button(t("Download again")).clicked();
            if can_replace {
                replace = ui.button(t("Replace the old file")).clicked();
            }
            cancel = ui.button(t("Cancel")).clicked();
        });
    });
    ctx.data_mut(|d| d.insert_temp(tick, remember));
    let how = if replace {
        Some(Dup::Replace)
    } else if again {
        Some(Dup::Again)
    } else if show {
        Some(Dup::Show)
    } else {
        None
    };
    if let Some(how) = how {
        app.dialogs.duplicate = None;
        ctx.data_mut(|d| d.insert_temp(tick, false));
        if remember {
            let mut wanted = app.applied.clone();
            wanted.duplicate_choice = how.word().to_string();
            if let Err(e) = app.apply_settings(wanted) {
                app.said = e.into();
            }
        }
        answer_duplicate(app, address, id, how);
    } else if cancel {
        app.dialogs.duplicate = None;
        ctx.data_mut(|d| d.insert_temp(tick, false));
    }
}

pub(super) fn add_url(app: &mut DmApp, ctx: &egui::Context) {
    let focus = app.dialogs.take_focus("Add URL");
    let recent: Vec<String> = dm_desktop_base::add_url::recent(&app.applied);
    let Some(d) = app.dialogs.add_url.as_mut() else {
        return;
    };
    let mut open = true;
    let (mut ok, mut cancel, mut forget) = (false, false, false);
    window(ctx, "Add URL").open(&mut open).show(ctx, |ui| {
        // As wide as the window allows: at 200% text a fixed 420 ran the
        // dialog past the window's edge.
        let wide = (super::room(ctx).width() - 160.0).clamp(200.0, 420.0);
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
                                d.address.clone_from(url);
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
    // What OK does with it is dm's (`dm_desktop_base::add_url::decide`),
    // as every window's Add URL does it.
    use dm_desktop_base::add_url::Next;
    let (typed, authorize, login, password) = (
        d.address.clone(),
        d.authorize,
        d.login.clone(),
        d.password.clone(),
    );
    let decided = dm_desktop_base::add_url::decide(
        &typed,
        authorize,
        &login,
        &password,
        &app.applied,
        app.rows(),
    );
    if let Next::Says(words) = decided.next {
        if let Some(d) = app.dialogs.add_url.as_mut() {
            d.says = words.into();
        }
        // The address keeps the keyboard, to be put right: after the
        // warning, typing went nowhere.
        app.dialogs.focus_on("Add URL");
        return;
    }
    app.dialogs.add_url = None;
    if let Some(recent) = decided.remember {
        match app.backend.remember_urls(recent) {
            Ok(now) => app.applied.recent_urls = now.recent_urls,
            Err(e) => app.said = e.into(),
        }
    }
    match decided.next {
        Next::Says(_) => {}
        Next::Details(addresses) => app.dialogs.open_file_info(addresses),
        Next::Duplicate { address, id } => app.dialogs.duplicate = Some((address, id)),
        Next::Answered { address, id, how } => answer_duplicate(app, address, id, how),
        Next::Playlist(address) => {
            let quality = app.applied.stream_quality.clone();
            app.dialogs.open_playlist(address, &app.backend, quality);
        }
    }
}

#[cfg(test)]
mod recent_tests {
    use dm_desktop_base::add_url::query_carries_a_secret as secret;

    /// A signed or token-bearing address is not kept in the settings file for
    /// Add URL's list; a plain one is.
    #[test]
    fn an_address_with_a_secret_in_its_query_is_not_remembered() {
        assert!(secret("https://cdn.example/f.zip?token=abc"));
        assert!(secret(
            "https://cdn.example/f.zip?X-Amz-Signature=1&X-Amz-Credential=2"
        ));
        assert!(secret("https://example.com/get?id=7&api_key=9"));
        assert!(secret("https://example.com/get?Password=x"));
        assert!(secret("https://example.com/get?sessionid=x"));
        assert!(!secret("https://example.com/files/a.zip"));
        assert!(!secret("https://example.com/get?id=7&name=report"));
        assert!(!secret("not an address"));
    }
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
