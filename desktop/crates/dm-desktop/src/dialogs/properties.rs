use super::*;
pub(super) fn properties(app: &mut DmApp, ctx: &egui::Context) {
    let ids: Vec<Id> = app.dialogs.properties.keys().copied().collect();
    let mut nth = 0.0_f32;
    for id in ids {
        let Some(row) = app.row(id).cloned() else {
            app.dialogs.properties.remove(&id);
            continue;
        };
        // What a row not yet placed will have, as the list shows it: its
        // size, kind and folder were dashes here.
        // And the size a dialog learned before it started, as the list
        // shows (a dash beside the list's 300.0 KB).
        let size = crate::table::shown_size(&row)
            .or_else(|| app.sizes_told.get(&id).copied())
            .map(format::bytes)
            .unwrap_or_default();
        // As the tree places it: a person's own category, the kind, or
        // Other for a kind not yet known ("—" for a row the
        // tree had under Other).
        let kind = match (app.custom_of(&row), crate::app::kind_of(&row)) {
            (Some(own), _) => own,
            (None, kind) => crate::i18n::name(&format::titled(&category_word(
                kind.unwrap_or(dm_core::Category::Other),
            ))),
        };
        // And the name the list shows: a waiting copy read "slow.bin"
        // beside the list's "slow (2).bin".
        let shown_name = app.name_it_will_get(&row, &mut app.applied.categories());
        let saved_to = if row.path.is_empty() {
            let mut categories = app.applied.categories();
            let folder = row
                .save_dir
                .clone()
                .filter(|d| !d.trim().is_empty())
                .or_else(|| app.planned_folder(&row, &mut categories));
            folder
                .map(|f| {
                    std::path::Path::new(&f)
                        .join(app.name_it_will_get(&row, &mut categories))
                        .display()
                        .to_string()
                })
                .unwrap_or_default()
        } else {
            row.path.clone()
        };
        let (mut ok, mut cancel, mut open_file) = (false, false, false);
        let focus = app.dialogs.take_focus("File properties");
        let runtime = app.backend.runtime.clone();
        let Some(d) = app.dialogs.properties.get_mut(&id) else {
            continue;
        };
        // A server that wants a sign-in: the keyboard goes to Login.
        let wants_login =
            matches!(&row.state, State::Failed(why) if crate::app::needs_sign_in(why));
        // No wider than the window leaves room for, however long a name,
        // path or referrer: they wrap, rather than pushing the buttons off
        // the screen.
        let room = (ctx.content_rect().width() - 16.0 - sides(ctx)).clamp(320.0, 620.0);
        // A close box in its title bar, as every other window has.
        let mut shut = true;
        // Below the menu bar, as the progress window keeps.
        let below = {
            let mut below = ctx.content_rect();
            below.min.y = app.menu_bottom.max(below.top()).min(below.bottom() - 120.0);
            below
        };
        // Taller than that room, its body scrolls: in a 720×460 window its
        // buttons were under the window's foot.
        let window = egui::Id::unique(("properties", id));
        let step = nth;
        nth += 1.0;
        let tall = (below.height() - 16.0).max(160.0);
        let cramped = ctx
            .memory(|m| m.area_rect(window))
            .is_some_and(|was| was.height() >= tall - 8.0);
        Dialog::new(t("File properties"))
            .id(window)
            .collapsible(false)
            .resizable(false)
            .open(&mut shut)
            .max_width(room)
            .max_height(tall)
            .vscroll(cramped)
            .constrain_to(below)
            // In the middle, as every other dialog opens, each further one
            // a step down and across; it opened in the top left corner.
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(below.center() + egui::vec2(24.0, 24.0) * step)
            .show(ctx, |ui| {
                ui.set_max_width(room);
                // The values' column: what is left beside the headings.
                let value = (room - 130.0).max(160.0);
                // Nothing to say is a plain dash: an empty wrapped label
                // stood a whole line tall and left gaps down the dialog.
                let said = |ui: &mut egui::Ui, text: &str| {
                    if text.is_empty() {
                        ui.label(RichText::new("—").weak());
                        return;
                    }
                    ui.scope(|ui| {
                        ui.set_max_width(value);
                        let drawn = ui.add(
                            egui::Label::new(&*crate::bidi::visual(text))
                                .wrap_mode(egui::TextWrapMode::Wrap),
                        );
                        crate::bidi::said_as(&drawn, text);
                    });
                };
                // The fields scroll inside a window too short for them, so
                // the buttons below stay on the screen.
                let tall = (ctx.content_rect().height() - 150.0).max(120.0);
                egui::ScrollArea::vertical()
                    .id_salt(("props-scroll", id))
                    .max_height(tall)
                    .show(ui, |ui| {
                        egui::Grid::new(("props", id))
                            .num_columns(2)
                            .spacing([12.0, 6.0])
                            .max_col_width(value)
                            .show(ui, |ui| {
                                ui.label(t("File name"));
                                said(ui, &shown_name);
                                ui.end_row();
                                ui.label(t("Status"));
                                // Held to the values' column like the rest:
                                // a long engine error, unwrapped, stretched
                                // every row of the grid.
                                let status = status_of(&row);
                                match crate::app::failure_as_said(&row) {
                                    Some(raw) => {
                                        ui.scope(|ui| said(ui, &status))
                                            .response
                                            .on_hover_text(raw);
                                    }
                                    None => said(ui, &status),
                                }
                                ui.end_row();
                                ui.label(t("Size"));
                                said(ui, &size);
                                ui.end_row();
                                ui.label(t("Category"));
                                // In the window's language, as the category tree
                                // names it.
                                said(ui, &kind);
                                ui.end_row();
                                // Not yet, for one not yet placed.
                                ui.label(if row.path.is_empty() {
                                    t("Will be saved to")
                                } else {
                                    t("Saved to")
                                });
                                said(ui, &saved_to);
                                ui.end_row();
                                ui.label(t("Referrer"));
                                said(ui, &row.referrer);
                                ui.end_row();
                                let name = ui.label(t("Address"));
                                // Also while it waits to try again: a wrong
                                // address is what it most needs then.
                                let editable =
                                    !matches!(row.state, State::Running | State::Completed)
                                        || crate::app::retrying(row.id).is_some();
                                let address = ui
                                    .add_enabled(
                                        editable,
                                        egui::TextEdit::singleline(&mut d.address)
                                            .desired_width(value.min(360.0)),
                                    )
                                    .labelled_by(name.id);
                                ok |= entered(&focused(address, focus && editable && !wants_login));
                                ui.end_row();
                                let name = ui.label(t("Login"));
                                let login = ui
                                    .add_enabled(
                                        editable,
                                        egui::TextEdit::singleline(&mut d.login)
                                            .desired_width(value.min(200.0)),
                                    )
                                    .labelled_by(name.id);
                                ok |= entered(&focused(login, focus && editable && wants_login));
                                ui.end_row();
                                let name = ui.label(t("Password"));
                                let password = ui
                                    .add_enabled(
                                        editable,
                                        egui::TextEdit::singleline(&mut d.password)
                                            .password(true)
                                            .desired_width(value.min(200.0)),
                                    )
                                    .labelled_by(name.id);
                                ok |= entered(&password);
                                ui.end_row();
                                let name = ui.label(t("Note"));
                                let note = ui
                                    .add(
                                        egui::TextEdit::singleline(&mut d.description)
                                            .desired_width(value.min(360.0)),
                                    )
                                    .labelled_by(name.id);
                                ok |= entered(&focused(note, focus && !editable));
                                ui.end_row();
                                // A finished file's digest, worked out beside
                                // the window, and a box to check it against
                                // one the person was given.
                                if row.state == State::Completed && !crate::table::gone(&row) {
                                    digest_rows(ui, ctx, &runtime, &row.path, d, value);
                                }
                            });
                    });
                if !d.says.is_empty() {
                    ui.colored_label(warn(ui), &d.says);
                }
                footer(ui, |ui| {
                    // Properties runs a finished file from here.
                    let can_open = row.state == State::Completed && !crate::table::gone(&row);
                    // The default first, as every dialog has it.
                    ok |= primary(ui, "OK").clicked() || enter_free(ui);
                    open_file = ui
                        .add_enabled(can_open, egui::Button::new(t("Open")))
                        .clicked();
                    cancel = ui.button(t("Cancel")).clicked();
                });
            });
        let cancel = cancel || !shut;
        if open_file {
            if let Err(e) = app.backend.platform.open(&row.path) {
                d.says = e;
            }
        }
        let (description, address) = (
            d.description.clone(),
            join_login(&d.address, &d.login, &d.password),
        );
        if cancel {
            app.dialogs.properties.remove(&id);
        }
        if ok {
            if description != row.description {
                app.backend.set_description(id, &description);
            }
            let changed = address.trim() != row.url;
            let result = if changed {
                app.backend.set_url(id, &address)
            } else {
                Ok(())
            };
            match result {
                Ok(()) => {
                    app.dialogs.properties.remove(&id);
                    // A sign-in given to a download that failed for want
                    // of one is what it was waiting for: tried again with it.
                    if changed && wants_login {
                        app.backend.retry(id);
                        app.said = crate::i18n::Said::new(
                            "Trying {} again with the sign-in.",
                            &[&name_of(&row)],
                        );
                        app.said_about = Some((id, app.said.clone(), false));
                    }
                }
                Err(e) => {
                    if let Some(d) = app.dialogs.properties.get_mut(&id) {
                        d.says = e;
                    }
                }
            }
            app.refresh();
        }
    }
}

/// Starts hashing `path` with `algorithm` unless that is already under way
/// or done, on the app's runtime: a large file takes a while, and the window
/// must not.
fn start_sum(
    runtime: &tokio::runtime::Handle,
    sums: &Sums,
    path: &str,
    algorithm: dm_core::checksum::Algorithm,
) {
    let name = algorithm.name();
    {
        let mut held = dm_core::lock(sums);
        if held.contains_key(name) {
            return;
        }
        held.insert(name, None);
    }
    let (sums, path) = (Arc::clone(sums), std::path::PathBuf::from(path));
    runtime.spawn(async move {
        let found = dm_core::checksum::digest(&path, algorithm)
            .await
            .map_err(|e| e.to_string());
        dm_core::lock(&sums).insert(name, Some(found));
    });
}

/// What has been found for `algorithm`: still working, the digest, or why not.
fn sum_of(sums: &Sums, algorithm: dm_core::checksum::Algorithm) -> Option<Result<String, String>> {
    dm_core::lock(sums).get(algorithm.name()).cloned().flatten()
}

/// The finished file's SHA-256, and the box that checks it against a digest
/// the person was given (MD5, SHA-256 or SHA-512, told by its length).
fn digest_rows(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    runtime: &tokio::runtime::Handle,
    path: &str,
    d: &mut Properties,
    value: f32,
) {
    use dm_core::checksum::{Algorithm, Checksum};
    let waiting = |ui: &mut egui::Ui| {
        ui.label(RichText::new(t("Working it out…")).weak());
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    };
    start_sum(runtime, &d.sums, path, Algorithm::Sha256);
    ui.label("SHA-256");
    match sum_of(&d.sums, Algorithm::Sha256) {
        None => waiting(ui),
        Some(Ok(hex)) => {
            ui.scope(|ui| {
                ui.set_max_width(value);
                ui.add(
                    egui::Label::new(RichText::new(hex).monospace())
                        .wrap_mode(egui::TextWrapMode::Wrap)
                        .selectable(true),
                );
            });
        }
        Some(Err(why)) => {
            ui.colored_label(warn(ui), why);
        }
    }
    ui.end_row();
    let name = ui.label(t("Check against"));
    ui.add(
        egui::TextEdit::singleline(&mut d.check)
            .hint_text(t("A digest you were given"))
            .desired_width(value.min(360.0)),
    )
    .labelled_by(name.id);
    ui.end_row();
    if let Some(given) = Checksum::parse(&d.check) {
        start_sum(runtime, &d.sums, path, given.algorithm);
        ui.label("");
        match sum_of(&d.sums, given.algorithm) {
            None => waiting(ui),
            Some(Ok(found)) if found.eq_ignore_ascii_case(&given.expected) => {
                ui.label(RichText::new(t("It matches.")).strong());
            }
            Some(Ok(_)) => {
                ui.colored_label(warn(ui), t("It does not match."));
            }
            Some(Err(why)) => {
                ui.colored_label(warn(ui), why);
            }
        }
        ui.end_row();
    }
}
