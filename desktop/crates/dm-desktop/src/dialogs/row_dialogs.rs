// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
pub(super) fn update_link(app: &mut DmApp, ctx: &egui::Context) {
    let focus = app.dialogs.take_focus("Update download link");
    // Which download, and the address it has now: the dialog named neither.
    let Some(row) = app
        .dialogs
        .update_link
        .as_ref()
        .and_then(|(id, _, _)| app.row(*id).cloned())
    else {
        app.dialogs.update_link = None;
        return;
    };
    let named = app.name_it_will_get(&row, &mut app.applied.categories());
    let Some((id, address, says)) = app.dialogs.update_link.as_mut() else {
        return;
    };
    let mut open = true;
    let mut ok = false;
    let mut cancel = false;
    let mut from_browser = false;
    window(ctx, "Update download link")
        .open(&mut open)
        .show(ctx, |ui| {
            ui.strong(&*crate::bidi::visual(&named));
            ui.add(
                egui::Label::new(RichText::new(format::shown_url(&row.url)).small().weak())
                    .truncate(),
            )
            .on_hover_text(format::shown_url(&row.url));
            ui.label(t("The download's link has expired? Paste a fresh one:"));
            ok = entered(&focused(field(ui, t("New address"), address, 420.0), focus));
            // Or the page it came from opened again in the browser, and the
            // next download clicked there taken as the link.
            if is_web_page(&row.referrer) {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(t("Or:")).weak());
                    if ui.button(t("Get it from the browser")).clicked() {
                        from_browser = true;
                    }
                });
            }
            if !says.is_empty() {
                ui.colored_label(warn(ui), says.as_str());
            }
            // Cancel beside OK, as every dialog has.
            footer(ui, |ui| {
                ok |= primary(ui, "OK").clicked() || enter_free(ui);
                if ui.button(t("Cancel")).clicked() {
                    cancel = true;
                }
            });
        });
    open &= !cancel;
    if from_browser {
        let row_id = *id;
        app.dialogs.update_link = None;
        if is_web_page(&row.referrer) {
            app.open_file(&row.referrer);
        }
        app.await_link(row_id);
        return;
    }
    // A download that failed for want of it goes on, from the parts it
    // has (`row_edits::update_link`, both windows).
    if ok && open {
        match dm_desktop_base::row_edits::update_link(&app.backend, *id, address) {
            Ok(()) => open = false,
            Err(e) => *says = e,
        }
    }
    if !open {
        app.dialogs.update_link = None;
    }
}

pub(super) fn move_to_queue(app: &mut DmApp, ctx: &egui::Context) {
    let focus = app.dialogs.take_focus("Move to queue");
    let known = app.queues();
    let Some((ids, queue, new)) = app.dialogs.move_to_queue.as_mut() else {
        return;
    };
    let mut open = true;
    let mut ok = false;
    let mut cancel = false;
    // The queue is held as shown, so the field reads in the language in
    // force, and taken back to dm's own name on OK: typing "Principal" in
    // Spanish is the Main queue, not a new one beside it. The pick and a
    // new name are kept apart: one field for both started with "Main
    // queue" in it, and typing "Night" made "Main queueNight".
    window(ctx, "Move to queue")
        .open(&mut open)
        .show(ctx, |ui| {
            for q in known.iter() {
                let shown = crate::i18n::name(q);
                let picked = new.trim().is_empty() && *queue == shown;
                if radio_flag(ui, picked, shown.clone()).clicked() {
                    *queue = shown;
                    new.clear();
                }
            }
            ok = entered(&focused(field(ui, t("Or a new queue"), new, 240.0), focus));
            footer(ui, |ui| {
                ok |= primary(ui, "OK").clicked() || enter_free(ui);
                cancel = ui.button(t("Cancel")).clicked();
            });
        });
    open &= !cancel;
    let chosen = dm_desktop_base::row_edits::queue_chosen(queue, new);
    if let (true, true, Some(named)) = (ok, open, chosen) {
        for id in ids.iter() {
            app.backend.set_queue(*id, &named);
        }
        open = false;
    }
    if !open {
        app.dialogs.move_to_queue = None;
    }
}

pub(super) fn rename(app: &mut DmApp, ctx: &egui::Context) {
    let focus = app.dialogs.take_focus("Move/Rename");
    let platform = std::sync::Arc::clone(&app.backend.platform);
    let Some(d) = app.dialogs.rename.as_mut() else {
        return;
    };
    let mut open = true;
    let mut ok = false;
    let mut cancel = false;
    window(ctx, "Move/Rename").open(&mut open).show(ctx, |ui| {
        // A new name for one; for several, only where they go.
        if d.ids.len() == 1 {
            ok = entered(&focused(
                field(ui, t("New name"), &mut d.name, 320.0),
                focus,
            ));
        } else {
            ui.label(crate::i18n::tn(
                "Moves {} finished file.||Moves {} finished files.",
                d.ids.len() as u64,
                &[&d.ids.len()],
            ));
            let name = ui.label(t("Name pattern"));
            let pattern = ui
                .add(
                    egui::TextEdit::singleline(&mut d.pattern)
                        .hint_text(t(
                            "Empty keeps the names. {n} counts from 1, {name} is the old name",
                        ))
                        .desired_width(320.0),
                )
                .labelled_by(name.id);
            ok |= entered(&pattern);
        }
        let name = ui.label(t("Folder"));
        ui.horizontal(|ui| {
            let folder = ui
                .add(egui::TextEdit::singleline(&mut d.folder).desired_width(280.0))
                .labelled_by(name.id);
            ok |= entered(&folder);
            browse_button(ui, &*platform, &mut d.folder);
        });
        footer(ui, |ui| {
            ok |= primary(ui, "OK").clicked() || enter_free(ui);
            cancel = ui.button(t("Cancel")).clicked();
        });
    });
    open &= !cancel;
    // Moved first where the folder changed, then renamed in it
    // (`MoveRename::carry`, both windows).
    if ok {
        let said = d.carry(&app.backend);
        app.refresh();
        if let Some(said) = said {
            app.said = said;
        }
        open = false;
    }
    if !open {
        app.dialogs.rename = None;
    }
}

pub(super) fn find(app: &mut DmApp, ctx: &egui::Context) {
    if !app.dialogs.find_open {
        return;
    }
    let focus = app.dialogs.take_focus("Find");
    let mut open = true;
    let mut next = false;
    // Never over the row it found: moved above or below that row where
    // it would cover it, else in the middle.
    let screen = super::room(ctx);
    let found = app.cursor.and_then(|id| app.row_rects.get(&id)).copied();
    let size = ctx
        .memory(|m| m.area_rect(window_id("Find")))
        .map(|r| r.size())
        .unwrap_or(egui::vec2(440.0, 170.0));
    let mut place = window(ctx, "Find");
    if let Some(row) = found {
        let middle = egui::Rect::from_center_size(screen.center(), size);
        if middle.expand(24.0).intersects(row) {
            let below = row.bottom() + 12.0;
            let top = if below + size.y <= screen.bottom() - 8.0 {
                below
            } else {
                (row.top() - 12.0 - size.y).max(screen.top())
            };
            place = place.anchor(egui::Align2::CENTER_TOP, [0.0, top - screen.top()]);
        }
    }
    let mut close = false;
    place.open(&mut open).show(ctx, |ui| {
        let r = focused(
            field(ui, t("Find what"), &mut app.dialogs.find_text, 260.0),
            focus,
        );
        // Enter finds the next and the field keeps the keyboard, so the
        // next Enter finds the one after and typing goes on into it.
        let typed = entered(&r);
        if typed {
            r.request_focus();
        }
        // A Close beside it, as every other dialog has.
        // A plain row: a band made the small window tall enough to cover
        // the row it had found.
        ui.horizontal(|ui| {
            next = ui.button(t("Find next")).clicked() || typed;
            if ui.button(t("Close")).clicked() {
                close = true;
            }
        });
    });
    if close {
        open = false;
    }
    if next {
        app.find_next();
    }
    app.dialogs.find_open = open;
}

/// Whether a row's referrer is a web page, to open as the page it came from
/// (`dm_desktop_base::row_edits`, both windows).
use dm_desktop_base::row_edits::is_web_page;
