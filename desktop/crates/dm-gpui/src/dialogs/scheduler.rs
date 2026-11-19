// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The Scheduler (B49, B62, B65), a window of its own: the queues at its
//! left, the list's own queue chosen, with Start now and Stop for it; the
//! chosen queue's hours at its right (its own, or the Main queue's), every
//! week on the days ticked or once on a date, what dm does when the
//! scheduled downloads are done, how many times a failed one is tried
//! again, and a program to run then. OK keeps every queue's hours, Cancel
//! none. What it keeps and refuses is dm's
//! (`dm_desktop_base::scheduler::SchedulerDraft`).

use dm_core::MAIN_QUEUE;
use dm_desktop_base::held::DAYS;
use dm_desktop_base::i18n::t;
use dm_desktop_base::power::WhenDone;
use dm_desktop_base::scheduler::SchedulerDraft;
use dm_shell::app::commands::Command;
use gpui::{
    div, prelude::*, size, App, Bounds, Context, Entity, FocusHandle, KeyDownEvent, Role,
    SharedString, WeakEntity, Window, WindowBounds, WindowOptions,
};
use gpui_component::input::{InputEvent, InputState};

use crate::widgets::native::{self, enabled};
use crate::widgets::space;
use crate::window::list::Downloads;
use crate::window::screen::{Screen, Screens};
use crate::window::theme::Theme;
use crate::window::zoom::px;

mod sections;

/// The date of "Once, on this date" as three pop-ups, year, month and day, this year and the
/// next four (gaps row U32): typed as "YYYY-MM-DD", a wrong one was refused only at OK.
fn date_picker(date: &str, theme: Theme, screen: &Screen) -> gpui::Div {
    use dm_desktop_base::scheduler::{date_parts, today};
    let now = today();
    let (y, m, d) = date_parts(date, now);
    let years: Vec<SharedString> = (0..5).map(|n| (now.0 + n).to_string().into()).collect();
    let months: Vec<SharedString> = dm_desktop_base::format::MONTHS
        .iter()
        .map(|w| t(w).into())
        .collect();
    let days: Vec<SharedString> = (1..=31).map(|n: u32| n.to_string().into()).collect();
    let year_at = usize::try_from(y - now.0).ok().filter(|i| *i < years.len());
    div()
        .flex()
        .items_center()
        .gap(px(space::XS))
        .child(crate::widgets::popup::button(
            "date-day",
            days[(d as usize).saturating_sub(1).min(30)].clone(),
            days,
            Some((d as usize).saturating_sub(1)),
            theme,
            screen,
        ))
        .child(crate::widgets::popup::button(
            "date-month",
            months[(m as usize).saturating_sub(1).min(11)].clone(),
            months,
            Some((m as usize).saturating_sub(1)),
            theme,
            screen,
        ))
        .child(crate::widgets::popup::button(
            "date-year",
            y.to_string().into(),
            years,
            year_at,
            theme,
            screen,
        ))
}

/// The Scheduler window's view.
pub struct Scheduler {
    view: WeakEntity<Downloads>,
    pub(crate) draft: SchedulerDraft,
    queues: Vec<String>,
    retries: Entity<InputState>,
    run: Entity<InputState>,
    done: bool,
    focus: FocusHandle,
    screen: Screen,
}

impl Scheduler {
    /// The fields as the draft shows the chosen queue.
    fn fields_from_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let s = self.draft.schedule.clone();
        self.retries
            .update(cx, |f, cx| f.set_value(s.retries.to_string(), window, cx));
        self.run
            .update(cx, |f, cx| f.set_value(s.run_when_done, window, cx));
    }

    /// What the fields hold, into the draft's chosen queue.
    fn draft_from_fields(&mut self, cx: &App) {
        self.draft.schedule.retries = self
            .retries
            .read(cx)
            .value()
            .trim()
            .parse::<u8>()
            .unwrap_or(0)
            .min(5);
        self.draft.schedule.run_when_done = self.run.read(cx).value().to_string();
    }

    /// A time's hour or minute, or the date's year, month or day, chosen from its pop-up: what
    /// it can hold is only what can be (gaps row U32).
    pub(crate) fn pick(&mut self, pick: &crate::widgets::popup::Pick, cx: &mut Context<Self>) {
        use dm_desktop_base::scheduler::{date_of, date_parts, today};
        let i = pick.index as u32;
        let schedule = &mut self.draft.schedule;
        match pick.field {
            "from-hour" => native::time_picked(&mut schedule.from, true, pick.index),
            "from-minute" => native::time_picked(&mut schedule.from, false, pick.index),
            "to-hour" => native::time_picked(&mut schedule.to, true, pick.index),
            "to-minute" => native::time_picked(&mut schedule.to, false, pick.index),
            "date-year" | "date-month" | "date-day" => {
                let (y, m, d) = date_parts(&self.draft.date, today());
                self.draft.date = match pick.field {
                    "date-year" => date_of(today().0 + i as i32, m, d),
                    "date-month" => date_of(y, i + 1, d),
                    _ => date_of(y, m, i + 1),
                };
            }
            _ => return,
        }
        cx.notify();
    }

    /// Another queue chosen in the list: the one shown kept, its hours shown.
    fn choose(&mut self, queue: String, window: &mut Window, cx: &mut Context<Self>) {
        if queue == self.draft.queue {
            return;
        }
        self.draft_from_fields(cx);
        self.draft.store();
        self.draft.load(&queue);
        self.draft.says.clear();
        self.fields_from_draft(window, cx);
        cx.notify();
    }

    /// Start now or Stop, for the queue chosen here.
    fn run_on_queue(&mut self, command: Command, cx: &mut Context<Self>) {
        let queue = self.draft.queue.clone();
        let _ = self
            .view
            .update(cx, |view, cx| view.run_on_queue(command, &queue, cx));
    }

    /// OK: every queue's hours kept, or the first that cannot be shown on
    /// its queue with why.
    fn ok(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.done {
            return;
        }
        self.draft_from_fields(cx);
        self.draft.store();
        if let Some((queue, why)) = self.draft.problem() {
            // Shown on the queue whose hours are wrong.
            if queue != self.draft.queue {
                self.draft.load(&queue);
                self.fields_from_draft(window, cx);
            }
            self.draft.says = why;
            cx.notify();
            return;
        }
        let Some(view) = self.view.upgrade() else {
            return;
        };
        let mut wanted = view.read(cx).backend.settings();
        wanted.schedule = self.draft.shared.clone();
        wanted.queue_schedules = self.draft.queues.clone();
        // Through the window's guarded way in, as Options saves: the program
        // run when the hours end is a person's to set, so a script driving
        // the window a moment ago keeps the one in force, and the window says
        // so (security review).
        match view.update(cx, |view, cx| view.apply_settings(wanted, cx)) {
            Ok(_) => {
                self.done = true;
                let _ = self.view.update(cx, |_, cx| cx.notify());
                window.remove_window();
            }
            Err(e) => {
                self.draft.says = e;
                cx.notify();
            }
        }
    }

    /// Cancel, Escape or the close button: no queue's hours kept.
    fn cancel(&mut self, window: &mut Window) {
        self.done = true;
        window.remove_window();
    }
}

impl Render for Scheduler {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin();
        let screen = self.screen.clone();

        // The queues on the left, as reference A lists them.
        let queues = self.queue_list(theme, &screen, cx);

        // The chosen queue's hours on the right.
        let hours = self.hours(theme, &screen, cx);

        crate::widgets::native::dialog("scheduler", t("Scheduler"), theme, cx)
            .on_action(
                cx.listener(|this, pick: &crate::widgets::popup::Pick, _, cx| this.pick(pick, cx)),
            )
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "enter" {
                    this.ok(window, cx);
                }
            }))
            .child(crate::widgets::fit::fitted(
                native::body(theme)
                    .child(
                        div()
                            .flex()
                            .gap(px(space::XL))
                            .child(queues)
                            .child(div().w(px(space::LINE)).flex_none().bg(theme.separator))
                            .child(hours),
                    )
                    .when(!self.draft.says.is_empty(), |d| {
                        d.child(native::note(
                            SharedString::from(self.draft.says.clone()),
                            theme.warning,
                            &screen,
                        ))
                    })
                    .child(native::buttons(
                        Vec::new(),
                        vec![
                            native::button("cancel", t("Cancel").into(), false, theme, &screen)
                                .on_click(cx.listener(|this, _, window, _| this.cancel(window)))
                                .into_any_element(),
                            native::button("ok", t("OK").into(), true, theme, &screen)
                                .on_click(cx.listener(|this, _, window, cx| this.ok(window, cx)))
                                .into_any_element(),
                        ],
                    )),
            ))
    }
}

/// Opens the Scheduler, the list's own queue (`queue`) chosen.
pub fn open(view: WeakEntity<Downloads>, queue: String, cx: &mut App) {
    // After the update that asked: the main window's view is read.
    cx.defer(move |cx| open_now(view, queue, cx));
}

fn open_now(view: WeakEntity<Downloads>, queue: String, cx: &mut App) {
    // Asked for again while open: the one open, as it stands.
    if crate::window::screen::bring_named(t("Scheduler"), cx) {
        return;
    }
    let Some(downloads) = view.upgrade() else {
        return;
    };
    let backend = downloads.read(cx).backend.clone();
    let settings = backend.settings();
    let queues = backend.queues();
    let (width, height) = crate::widgets::native::WindowSize::Scheduler.here();
    let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
    let opened = crate::window::screen::open_window(
        cx,
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(t("Scheduler").into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            let screen = Screen::default();
            Screens::register(window.window_handle(), screen.clone(), true, cx);
            let input =
                |window: &mut Window, cx: &mut App| cx.new(|cx| InputState::new(window, cx));
            let retries = input(window, cx);
            let run = input(window, cx);
            let scheduler = cx.new(|cx| {
                // Return in a field is OK.
                for input in [&retries, &run] {
                    cx.subscribe_in(
                        input,
                        window,
                        |this: &mut Scheduler, _, event, window, cx| {
                            if let InputEvent::PressEnter { .. } = event {
                                this.ok(window, cx);
                            }
                        },
                    )
                    .detach();
                }
                Scheduler {
                    view,
                    draft: SchedulerDraft::new(&settings, &queue),
                    queues,
                    retries,
                    run,
                    done: false,
                    focus: cx.focus_handle(),
                    screen,
                }
            });
            scheduler.update(cx, |s, cx| {
                s.fields_from_draft(window, cx);
                window.focus(&s.focus, cx);
            });
            crate::widgets::native::close_is_cancel(&scheduler, window, cx);
            cx.new(|cx| gpui_component::Root::new(scheduler, window, cx))
        },
    );
    if let Err(e) = opened {
        dm_core::note!(error, "dm-gpui: the Scheduler could not be opened: {e}");
    }
}

impl crate::widgets::native::CloseButton for Scheduler {
    fn close_button(&mut self, window: &mut Window, _: &mut Context<Self>) {
        self.cancel(window);
    }
}
