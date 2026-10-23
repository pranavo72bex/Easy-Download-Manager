use super::*;
/// The Scheduler's days, Sunday first, as `Schedule::days` numbers them.
pub const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

pub(super) fn scheduler(app: &mut DmApp, ctx: &egui::Context) {
    let Some((schedule, says)) = app.dialogs.scheduler.as_mut() else {
        return;
    };
    let mut open = true;
    let mut apply = false;
    let mut cancel = false;
    window(ctx, "Scheduler").open(&mut open).show(ctx, |ui| {
        // Plainly, for every queue: "When Main queue and every other
        // download may run." read oddly.
        ui.label(t("The hours when downloads may run, in every queue."));
        check(
            ui,
            &mut schedule.enabled,
            t("Only download between set hours"),
        );
        ui.add_enabled_ui(schedule.enabled, |ui| {
            ui.horizontal(|ui| {
                apply |= entered(&field(ui, t("Start at"), &mut schedule.from, 60.0));
                apply |= entered(&field(ui, t("Stop at"), &mut schedule.to, 60.0));
            });
            ui.horizontal(|ui| {
                for (day, word) in DAYS.iter().enumerate() {
                    let day = day as u8;
                    let mut on = schedule.days.contains(&day);
                    if check(ui, &mut on, t(word)).changed() {
                        if on {
                            schedule.days.push(day);
                            schedule.days.sort_unstable();
                        } else {
                            schedule.days.retain(|d| *d != day);
                        }
                    }
                }
            });
            // Where the system lets dm wake the computer for it.
            if crate::wake::possible() {
                check(
                    ui,
                    &mut schedule.wake,
                    t("Wake the computer from sleep to start"),
                );
            }
            // The days are shown as they are kept, all ticked for every
            // day; the hint said no tick meant every day beside seven ticks.
            ui.label(
                RichText::new(t(
                    "HH:MM, local time. A window ending before it starts runs overnight.",
                ))
                .small()
                .weak(),
            );
        });
        if !says.is_empty() {
            ui.colored_label(warn(ui), says.as_str());
        }
        footer(ui, |ui| {
            apply |= primary(ui, "OK").clicked() || enter_free(ui);
            cancel = ui.button(t("Cancel")).clicked();
        });
    });
    open &= !cancel;
    if apply && schedule.enabled && schedule.days.is_empty() {
        *says = t("Tick at least one day.").to_string();
    } else if apply
        && schedule.enabled
        && !(dm_core::schedule::is_time(&schedule.from) && dm_core::schedule::is_time(&schedule.to))
    {
        // Refused here, where it was typed: saved, "25:00" or "1am" turned
        // the schedule off without a word.
        *says = tf(
            "\"{}\" to \"{}\": times are HH:MM in 24 hours, such as 01:00 or 23:30.",
            &[&schedule.from, &schedule.to],
        );
    } else if apply {
        let mut wanted = app.applied.clone();
        wanted.schedule = schedule.clone();
        match app.apply_settings(wanted) {
            Ok(()) => open = false,
            Err(e) => {
                if let Some((_, says)) = app.dialogs.scheduler.as_mut() {
                    *says = e;
                }
            }
        }
    }
    if !open {
        app.dialogs.scheduler = None;
    }
}

pub(super) fn limiter(app: &mut DmApp, ctx: &egui::Context) {
    let focus = app.dialogs.take_focus("Speed limiter");
    let Some((limit, says, mb, hours)) = app.dialogs.limiter.as_mut() else {
        return;
    };
    let mut open = true;
    let mut apply = false;
    let mut cancel = false;
    window(ctx, "Speed limiter")
        .open(&mut open)
        .show(ctx, |ui| {
            // The fields in one column, the buttons apart below them: each
            // field sat where its label happened to end.
            egui::Grid::new("limiter")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    let name = ui.label(t("Maximum, KB/s across every download (0 for none)"));
                    // The limit takes the keyboard as the dialog opens, and
                    // Enter in it applies, as in any other dialog's field.
                    let field = focused(
                        ui.add(number_box(limit).range(0..=1024 * 1024))
                            .labelled_by(name.id),
                        focus,
                    );
                    select_on_focus(ui, &field, &limit.to_string());
                    apply |= entered(&field);
                    ui.end_row();
                    // Download limits, for a link with a fair-use
                    // allowance: downloads are held once it is used and go
                    // on after.
                    let name = ui.label(t("Download at most this many MB (0 for no limit)"));
                    let mb_field = ui
                        .add(number_box(mb).range(0..=1024 * 1024 * 1024))
                        .labelled_by(name.id);
                    select_on_focus(ui, &mb_field, &mb.to_string());
                    apply |= entered(&mb_field);
                    ui.end_row();
                    let name = ui.label(t("in each period of this many hours"));
                    let hours_field = ui
                        .add(number_box(hours).range(1..=24 * 31))
                        .labelled_by(name.id);
                    select_on_focus(ui, &hours_field, &hours.to_string());
                    apply |= entered(&hours_field);
                    ui.end_row();
                });
            // The modern footer draws its own line.
            if !crate::theme::Tokens::get(ui.ctx()).modern {
                ui.separator();
            }
            if !says.is_empty() {
                ui.colored_label(warn(ui), says.as_str());
            }
            footer(ui, |ui| {
                apply |= primary(ui, "OK").clicked() || enter_free(ui);
                cancel = ui.button(t("Cancel")).clicked();
            });
        });
    open &= !cancel;
    if apply && open {
        let mut wanted = app.applied.clone();
        wanted.speed_limit = *limit;
        wanted.limit_mb = *mb;
        wanted.limit_hours = *hours;
        match app.apply_settings(wanted) {
            Ok(()) => open = false,
            Err(e) => {
                if let Some((_, says, _, _)) = app.dialogs.limiter.as_mut() {
                    *says = e;
                }
            }
        }
    }
    if !open {
        app.dialogs.limiter = None;
    }
}
