// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// The server sent another name than the address's: what reference A asks
/// about, and so dm does (maintainer, 2026-11-01).
pub struct NameAsk {
    /// Which dialog this is.
    pub id: Id,
    /// The name the address gave.
    pub address_name: String,
    /// The name the server sent.
    pub server_name: String,
}

/// Asked once the server's name is known and is not the address's: save under
/// the server's name (the default, as dm has always done) or the address's.
pub(super) fn name_ask(app: &mut DmApp, ctx: &egui::Context) {
    let Some(ask) = app.dialogs.name_ask.as_ref() else {
        return;
    };
    let (id, address_name, server_name) =
        (ask.id, ask.address_name.clone(), ask.server_name.clone());
    if app.row(id).is_none() {
        app.dialogs.name_ask = None;
        return;
    }
    let (mut yes, mut no) = (false, false);
    window(ctx, "Name sent by the server").show(ctx, |ui| {
        ui.label(tf(
            "The name to save the file was {}, but the server sent {}. Save the file with the name received from the server?",
            &[&address_name, &server_name],
        ));
        ui.add_space(8.0);
        footer(ui, |ui| {
            yes = primary(ui, "Yes").clicked() || enter_free(ui);
            no = ui.button(t("No")).clicked();
        });
    });
    if yes {
        app.dialogs.name_ask = None;
    } else if no {
        app.dialogs.name_ask = None;
        app.use_address_name(id, &address_name);
    }
}

/// Asked when a web page answered for a file: save it as an HTML page, or
/// leave nothing behind.
pub(super) fn page_ask(app: &mut DmApp, ctx: &egui::Context) {
    let Some(id) = app.dialogs.page_ask else {
        return;
    };
    if app.row(id).is_none() {
        app.dialogs.page_ask = None;
        return;
    }
    let (mut yes, mut no) = (false, false);
    window(ctx, "Web page instead of the file").show(ctx, |ui| {
        ui.label(t(
            "The requested file looks like an HTML web page. Would you like to continue downloading it and save it as an HTML page?",
        ));
        ui.add_space(8.0);
        footer(ui, |ui| {
            yes = primary(ui, "Yes").clicked() || enter_free(ui);
            no = ui.button(t("No")).clicked();
        });
    });
    if yes {
        app.dialogs.page_ask = None;
        app.backend.accept_page(id);
    } else if no {
        app.dialogs.page_ask = None;
        app.backend.remove(id);
    }
}

/// Told once when a server says the file is not there, beside the row being
/// marked failed.
pub(super) fn not_found(app: &mut DmApp, ctx: &egui::Context) {
    if app.dialogs.not_found.is_none() {
        return;
    }
    let mut ok = false;
    window(ctx, "The file doesn't exist").show(ctx, |ui| {
        ui.label(t("The requested file doesn't exist."));
        ui.add_space(6.0);
        ui.label(t("Details:"));
        ui.monospace("HTTP/1.1 404 Not Found");
        ui.add_space(8.0);
        footer(ui, |ui| {
            ok = primary(ui, "OK").clicked() || enter_free(ui);
        });
    });
    if ok {
        app.dialogs.not_found = None;
    }
}
