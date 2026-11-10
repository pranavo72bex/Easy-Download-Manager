// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
/// The Scheduler's days, Sunday first, as `Schedule::days` numbers them.
pub const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// The Scheduler as it is being edited: every queue's hours, and the queue
/// whose hours show, as reference A's lists its queues beside their
/// schedule.
#[derive(Debug, Clone)]
pub struct SchedulerDraft {
    /// The queue chosen in the list.
    pub queue: String,
    /// The hours shown: the chosen queue's.
    pub schedule: Schedule,
    /// Whether the chosen queue has hours of its own. The Main queue's are
    /// the ones every queue without its own follows, so it always has.
    pub own: bool,
    /// Started once, on `date`, rather than every week on the days ticked.
    pub once: bool,
    /// The date of the once-only start, `YYYY-MM-DD`.
    pub date: String,
    /// The Main queue's hours, which every queue without its own follows.
    pub shared: Schedule,
    /// The queues with hours of their own.
    pub queues: BTreeMap<String, Schedule>,
    /// What is wrong with what was typed, said under it.
    pub says: String,
    /// Whether the queues stand beside the hours, decided once, as it
    /// first opens: a dialog that is a window of its own reads its own size
    /// in its own frames, and decided each frame it flipped between the two
    /// layouts, its buttons moving so that no click took.
    pub beside: Option<bool>,
}

impl SchedulerDraft {
    /// The Scheduler over these settings, showing `queue`'s hours.
    pub fn new(settings: &Settings, queue: &str) -> Self {
        // No days kept means every day, shown as every day ticked.
        let every_day = |mut s: Schedule| {
            if s.days.is_empty() {
                s.days = (0..=6).collect();
            }
            s
        };
        let mut draft = SchedulerDraft {
            queue: String::new(),
            schedule: Schedule::default(),
            own: true,
            once: false,
            date: String::new(),
            shared: every_day(settings.schedule.clone()),
            // The Main queue's hours are the shared ones; an entry of its
            // own (only a settings file written by hand has one) is not
            // shown, so it is not kept.
            queues: settings
                .queue_schedules
                .iter()
                .filter(|(name, _)| name.as_str() != MAIN_QUEUE)
                .map(|(name, s)| (name.clone(), every_day(s.clone())))
                .collect(),
            says: String::new(),
            beside: None,
        };
        draft.load(queue);
        draft
    }

    /// Shows `queue`'s hours.
    pub fn load(&mut self, queue: &str) {
        self.queue = queue.to_string();
        let (schedule, own) = if queue == MAIN_QUEUE {
            (self.shared.clone(), true)
        } else if let Some(own) = self.queues.get(queue) {
            (own.clone(), true)
        } else {
            (self.shared.clone(), false)
        };
        self.schedule = schedule;
        self.own = own;
        // A once-only start is shown as its date, its time in Start at.
        let once = self.schedule.once_at.clone();
        match once.as_deref().and_then(|at| at.trim().split_once(' ')) {
            Some((date, time)) => {
                self.once = true;
                self.date = date.to_string();
                self.schedule.from = time.trim().to_string();
            }
            None => {
                self.once = false;
                self.date = chrono::Local::now().format("%Y-%m-%d").to_string();
            }
        }
    }

    /// Keeps what the form shows as the chosen queue's hours.
    pub fn store(&mut self) {
        let mut schedule = self.schedule.clone();
        schedule.once_at = self
            .once
            .then(|| format!("{} {}", self.date.trim(), schedule.from.trim()));
        if self.queue == MAIN_QUEUE {
            self.shared = schedule;
        } else if self.own {
            self.queues.insert(self.queue.clone(), schedule);
        } else {
            self.queues.remove(&self.queue);
        }
    }

    /// The first queue whose hours cannot be kept, and why; Main first.
    pub fn problem(&self) -> Option<(String, String)> {
        std::iter::once((MAIN_QUEUE, &self.shared))
            .chain(self.queues.iter().map(|(name, s)| (name.as_str(), s)))
            .find_map(|(name, s)| schedule_problem(s).map(|why| (name.to_string(), why)))
    }
}

/// Why these hours cannot be kept, where they cannot.
fn schedule_problem(schedule: &Schedule) -> Option<String> {
    if !schedule.enabled {
        return None;
    }
    if !(dm_core::is_time(&schedule.from) && dm_core::is_time(&schedule.to)) {
        // Refused here, where it was typed: saved, "25:00" or "1am" turned
        // the schedule off without a word.
        return Some(tf(
            "\"{}\" to \"{}\": times are HH:MM in 24 hours, such as 01:00 or 23:30.",
            &[&schedule.from, &schedule.to],
        ));
    }
    match schedule.once_at.as_deref() {
        Some(at) => {
            let date = at.trim().split(' ').next().unwrap_or_default();
            chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .is_err()
                .then(|| {
                    tf(
                        "\"{}\" is not a date: write it as YYYY-MM-DD, such as 2026-12-31.",
                        &[&date],
                    )
                })
        }
        None if schedule.days.is_empty() => Some(t("Tick at least one day.").to_string()),
        None => None,
    }
}

pub(super) fn scheduler(app: &mut DmApp, ctx: &egui::Context) {
    let queues = app.queues();
    let Some(draft) = app.dialogs.scheduler.as_mut() else {
        return;
    };
    let mut open = true;
    let mut apply = false;
    let mut cancel = false;
    let mut chosen: Option<String> = None;
    let mut run: Option<crate::commands::Command> = None;
    // Side by side where the window has the room; at large text or in a
    // small window the queues above the hours, which ran past the edge.
    // The width the two need beside each other: a scrolled body keeps the
    // dialog's least width, and the days ran past its edge.
    const BESIDE: f32 = 660.0;
    let beside = *draft
        .beside
        .get_or_insert_with(|| super::room(ctx).width() - sides(ctx) >= BESIDE + 40.0);
    let mut dialog = window(ctx, "Scheduler");
    if beside {
        dialog = dialog.min_width(BESIDE);
    }
    dialog.open(&mut open).show(ctx, |ui| {
        if beside {
            ui.horizontal_top(|ui| {
                // The queues on the left, as reference A lists them; their
                // hours on the right.
                let left = ui
                    .vertical(|ui| {
                        ui.set_width(140.0);
                        queue_list(ui, &queues, &draft.queue, &mut chosen, &mut run);
                    })
                    .response
                    .rect;
                ui.add_space(16.0);
                let right = ui
                    .vertical(|ui| schedule_form(ui, draft, &mut apply))
                    .response
                    .rect;
                // A line between them as tall as the two: a separator takes
                // all the height it is given, which in a window of its own
                // is the window's, and the window grew to it frame after
                // frame until it scrolled, then shrank, and over again.
                let x = (left.right() + right.left()) / 2.0;
                ui.painter().vline(
                    x,
                    left.top()..=left.bottom().max(right.bottom()),
                    ui.visuals().widgets.noninteractive.bg_stroke,
                );
            });
        } else {
            queue_list(ui, &queues, &draft.queue, &mut chosen, &mut run);
            ui.separator();
            schedule_form(ui, draft, &mut apply);
        }
        if !draft.says.is_empty() {
            let said = ui.colored_label(warn(ui), draft.says.as_str());
            // Brought into view when it is new: under a body that scrolls
            // it was below the fold, and OK seemed to do nothing.
            let seen_id = egui::Id::unique("scheduler-said");
            if ui.data(|d| d.get_temp::<String>(seen_id)).as_deref() != Some(draft.says.as_str()) {
                said.scroll_to_me(Some(egui::Align::Center));
                ui.data_mut(|d| d.insert_temp(seen_id, draft.says.clone()));
            }
        }
        footer(ui, |ui| {
            apply |= primary(ui, "OK").clicked() || enter_free(ui);
            cancel = ui.button(t("Cancel")).clicked();
        });
    });
    open &= !cancel;
    if let Some(queue) = chosen {
        draft.store();
        draft.load(&queue);
        draft.says.clear();
    }
    if let Some(command) = run {
        app.queue_chosen = Some(draft.queue.clone());
        app.queued.push(command);
    }
    if apply && open {
        draft.store();
        if let Some((queue, why)) = draft.problem() {
            // Shown on the queue whose hours are wrong.
            if queue != draft.queue {
                draft.load(&queue);
            }
            draft.says = why;
        } else {
            let mut wanted = app.applied.clone();
            wanted.schedule = draft.shared.clone();
            wanted.queue_schedules = draft.queues.clone();
            match app.apply_settings(wanted) {
                Ok(()) => open = false,
                Err(e) => {
                    if let Some(draft) = app.dialogs.scheduler.as_mut() {
                        draft.says = e;
                    }
                }
            }
        }
    }
    if !open {
        app.dialogs.scheduler = None;
    }
}

/// The Scheduler's queues, the one shown marked, with Start now and Stop
/// for it, as the queue list's Start Queue and Stop Queue.
fn queue_list(
    ui: &mut egui::Ui,
    queues: &[String],
    shown: &str,
    chosen: &mut Option<String>,
    run: &mut Option<crate::commands::Command>,
) {
    ui.label(RichText::new(t("Queues")).strong());
    for queue in queues {
        let name = crate::i18n::name(queue);
        if option(ui, shown == queue, name).clicked() && shown != queue {
            *chosen = Some(queue.clone());
        }
    }
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        if ui.button(t("Start now")).clicked() {
            *run = Some(crate::commands::Command::StartQueue);
        }
        if ui.button(t("Stop")).clicked() {
            *run = Some(crate::commands::Command::StopQueue);
        }
    });
}

/// The chosen queue's hours: whether it has its own, the hours, the days or
/// a date, and what is done when they end.
fn schedule_form(ui: &mut egui::Ui, draft: &mut SchedulerDraft, apply: &mut bool) {
    if draft.queue == MAIN_QUEUE {
        ui.label(t(
            "The hours when downloads may run, in every queue without hours of its own.",
        ));
    } else {
        toggle_row(ui, &mut draft.own, t("Hours of its own for this queue"));
        if !draft.own {
            ui.label(RichText::new(t("This queue follows the Main queue's hours.")).weak());
        }
    }
    let SchedulerDraft {
        schedule,
        own,
        once,
        date,
        ..
    } = draft;
    ui.add_enabled_ui(*own, |ui| {
        toggle_row(
            ui,
            &mut schedule.enabled,
            t("Only download between set hours"),
        );
        ui.add_enabled_ui(schedule.enabled, |ui| {
            ui.horizontal(|ui| {
                *apply |= entered(&field(ui, t("Start at"), &mut schedule.from, 60.0));
                *apply |= entered(&field(ui, t("Stop at"), &mut schedule.to, 60.0));
            });
            // Every week on the days ticked, or once on a date, as reference
            // A's "Daily" and "Once at".
            if radio_flag(ui, !*once, t("Every week, on the days ticked")).clicked() {
                *once = false;
            }
            ui.add_enabled_ui(!*once, |ui| {
                ui.horizontal_wrapped(|ui| {
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
            });
            ui.horizontal(|ui| {
                if radio_flag(ui, *once, t("Once, on this date")).clicked() {
                    *once = true;
                }
                ui.add_enabled_ui(*once, |ui| {
                    *apply |= entered(&field(ui, t("Date (YYYY-MM-DD)"), date, 90.0));
                });
            });
            // Where the system lets dm wake the computer for it.
            if crate::wake::possible() {
                toggle_row(
                    ui,
                    &mut schedule.wake,
                    t("Wake the computer from sleep to start"),
                );
            }
            // What dm does when the scheduled downloads are done: through the
            // countdown that can be cancelled, as Downloads › When done is.
            ui.label(t("When the scheduled downloads are done"));
            ui.horizontal_wrapped(|ui| {
                use crate::commands::Command;
                use crate::power::WhenDone;
                let now = WhenDone::from_word(&schedule.when_done);
                for (w, command) in [
                    (WhenDone::Nothing, Command::WhenDoneNothing),
                    (WhenDone::Quit, Command::WhenDoneQuit),
                    (WhenDone::Sleep, Command::WhenDoneSleep),
                    (WhenDone::Hibernate, Command::WhenDoneHibernate),
                    (WhenDone::ShutDown, Command::WhenDoneShutDown),
                ] {
                    // As the Downloads menu greys them: only what this
                    // computer can do.
                    let can = w.possible(crate::power::available);
                    ui.add_enabled_ui(can, |ui| {
                        if radio_flag(ui, now == w, command.title()).clicked() {
                            schedule.when_done = w.word().to_string();
                        }
                    });
                }
            });
            ui.horizontal(|ui| {
                let mut tries = u64::from(schedule.retries);
                let name = ui.label(t(
                    "Try a failed download again this many times (0 for none)",
                ));
                if ui
                    .add(number_box(&mut tries).range(0..=5))
                    .labelled_by(name.id)
                    .changed()
                {
                    schedule.retries = tries.min(5) as u8;
                }
            });
            // A program run once when they are done, as Downloads > Options'
            // command is after each file: no shell, its words split.
            field(
                ui,
                t("Run this program when they are done (optional)"),
                &mut schedule.run_when_done,
                320.0,
            );
            // Reference A's, after the file it opens: the connection set in
            // Options › Advanced › Dial Up / VPN, which is Windows' alone.
            if cfg!(windows) {
                toggle_row(ui, &mut schedule.hang_up, t("Hang up modem when done"));
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
    });
}

pub(super) fn limiter(app: &mut DmApp, ctx: &egui::Context) {
    let focus = app.dialogs.take_focus("Speed limiter");
    let Some((limit, says, mb, hours, slow)) = app.dialogs.limiter.as_mut() else {
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
                    let name = ui.label(t("Speed limit (KB/s, 0 for none)"));
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
            // A lower limit for set hours of every day.
            toggle_row(ui, &mut slow.enabled, t("Slower between set hours"));
            ui.add_enabled_ui(slow.enabled, |ui| {
                ui.horizontal(|ui| {
                    apply |= entered(&field(ui, t("Start at"), &mut slow.from, 60.0));
                    apply |= entered(&field(ui, t("Stop at"), &mut slow.to, 60.0));
                });
                ui.horizontal(|ui| {
                    let name = ui.label(t("Speed limit in those hours (KB/s)"));
                    let field = ui
                        .add(number_box(&mut slow.limit).range(1..=10_000_000))
                        .labelled_by(name.id);
                    apply |= entered(&field);
                });
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
    if apply
        && open
        && slow.enabled
        && !(dm_core::is_time(&slow.from) && dm_core::is_time(&slow.to))
    {
        *says = tf(
            "\"{}\" to \"{}\": times are HH:MM in 24 hours, such as 01:00 or 23:30.",
            &[&slow.from, &slow.to],
        );
    } else if apply && open {
        let mut wanted = app.applied.clone();
        wanted.speed_limit = *limit;
        wanted.limit_mb = *mb;
        wanted.limit_hours = *hours;
        wanted.slow_hours = slow.clone();
        match app.apply_settings(wanted) {
            Ok(()) => {
                open = false;
                // Put in force at once, by the hours as they are now.
                app.session.forget_limit();
            }
            Err(e) => {
                if let Some((_, says, _, _, _)) = app.dialogs.limiter.as_mut() {
                    *says = e;
                }
            }
        }
    }
    if !open {
        app.dialogs.limiter = None;
    }
}
