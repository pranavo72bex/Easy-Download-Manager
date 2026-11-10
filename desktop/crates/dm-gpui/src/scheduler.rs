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
    div, prelude::*, px, size, App, Bounds, Context, Entity, FocusHandle, KeyDownEvent, Role,
    SharedString, WeakEntity, Window, WindowBounds, WindowOptions,
};
use gpui_component::input::{InputEvent, InputState};

use crate::list::Downloads;
use crate::native::{self, enabled};
use crate::screen::{Screen, Screens};
use crate::theme::Theme;

/// The Scheduler window's view.
pub struct Scheduler {
    view: WeakEntity<Downloads>,
    draft: SchedulerDraft,
    queues: Vec<String>,
    from: Entity<InputState>,
    to: Entity<InputState>,
    date: Entity<InputState>,
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
        let date = self.draft.date.clone();
        self.from
            .update(cx, |f, cx| f.set_value(s.from, window, cx));
        self.to.update(cx, |f, cx| f.set_value(s.to, window, cx));
        self.date.update(cx, |f, cx| f.set_value(date, window, cx));
        self.retries
            .update(cx, |f, cx| f.set_value(s.retries.to_string(), window, cx));
        self.run
            .update(cx, |f, cx| f.set_value(s.run_when_done, window, cx));
    }

    /// What the fields hold, into the draft's chosen queue.
    fn draft_from_fields(&mut self, cx: &App) {
        self.draft.schedule.from = self.from.read(cx).value().trim().to_string();
        self.draft.schedule.to = self.to.read(cx).value().trim().to_string();
        self.draft.date = self.date.read(cx).value().trim().to_string();
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
        let backend = view.read(cx).backend.clone();
        let mut wanted = backend.settings();
        wanted.schedule = self.draft.shared.clone();
        wanted.queue_schedules = self.draft.queues.clone();
        match backend.set_settings(wanted) {
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
        let main = self.draft.queue == MAIN_QUEUE;
        let own = self.draft.own;
        let on = self.draft.schedule.enabled;
        let once = self.draft.once;
        let days = self.draft.schedule.days.clone();
        let when_done = WhenDone::from_word(&self.draft.schedule.when_done);
        let wake = self.draft.schedule.wake;

        // The queues on the left, as reference A lists them.
        // As wide as its two buttons side by side.
        let queues = div()
            .w(px(2.0 * native::BUTTON_MIN + native::LABEL_GAP + 4.0))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .mb(px(4.0))
                    .child(screen.label(t("Queues"))),
            )
            .children(self.queues.iter().enumerate().map(|(i, queue)| {
                let chosen = *queue == self.draft.queue;
                let q = queue.clone();
                div()
                    .id(("queue", i))
                    .role(Role::ListItem)
                    .aria_label(dm_desktop_base::i18n::name(queue))
                    .h(px(24.0))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .rounded(px(native::RADIUS))
                    .when(chosen, |d| d.bg(theme.accent).text_color(gpui::white()))
                    .when(!chosen, |d| d.hover(|s| s.bg(theme.stripe)))
                    .child(screen.label(dm_desktop_base::i18n::name(queue)))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.choose(q.clone(), window, cx)),
                    )
            }))
            .child(
                div()
                    .mt(px(8.0))
                    .flex()
                    .gap(px(native::LABEL_GAP))
                    .child(
                        native::button("queue-start", t("Start now").into(), false, theme, &screen)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.run_on_queue(Command::StartQueue, cx)
                            })),
                    )
                    .child(
                        native::button("queue-stop", t("Stop").into(), false, theme, &screen)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.run_on_queue(Command::StopQueue, cx)
                            })),
                    ),
            );

        // The chosen queue's hours on the right.
        let field = |input: &Entity<InputState>, name: &'static str, w: f32| {
            div().w(px(w)).child(native::field(input, name, theme))
        };
        let hours = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(native::ROW_GAP))
            .when(main, |d| {
                d.child(screen.label(t(
                    "The hours when downloads may run, in every queue without hours of its own.",
                )))
            })
            .when(!main, |d| {
                d.child(
                    native::checkbox(
                        "own",
                        t("Hours of its own for this queue").into(),
                        own,
                        theme,
                        &screen,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.draft.own = !this.draft.own;
                        cx.notify();
                    })),
                )
                .when(!own, |d| {
                    d.child(native::note(
                        t("This queue follows the Main queue's hours."),
                        theme.muted,
                        &screen,
                    ))
                })
            })
            .child(enabled(
                own,
                div()
                    .flex()
                    .flex_col()
                    .gap(px(native::ROW_GAP))
                    .child(
                        native::checkbox(
                            "enabled",
                            t("Only download between set hours").into(),
                            on,
                            theme,
                            &screen,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.draft.schedule.enabled = !this.draft.schedule.enabled;
                            cx.notify();
                        })),
                    )
                    .child(enabled(
                        on,
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(native::ROW_GAP))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(native::LABEL_GAP))
                                    .child(screen.label(t("Start at")))
                                    .child(field(&self.from, "Start at", 70.0))
                                    .child(div().w(px(8.0)))
                                    .child(screen.label(t("Stop at")))
                                    .child(field(&self.to, "Stop at", 70.0)),
                            )
                            // Every week on the days ticked, or once on a
                            // date, as reference A's "Daily" and "Once at".
                            .child(
                                native::radio(
                                    "weekly",
                                    t("Every week, on the days ticked").into(),
                                    !once,
                                    theme,
                                    &screen,
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.draft.once = false;
                                    cx.notify();
                                })),
                            )
                            .child(enabled(
                                !once,
                                div().pl(px(20.0)).flex().flex_wrap().gap(px(10.0)).children(
                                    DAYS.iter().enumerate().map(|(day, word)| {
                                        let day = day as u8;
                                        let ticked = days.contains(&day);
                                        native::checkbox(
                                            ("day", day as usize),
                                            t(word).into(),
                                            ticked,
                                            theme,
                                            &screen,
                                        )
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            let days = &mut this.draft.schedule.days;
                                            if days.contains(&day) {
                                                days.retain(|d| *d != day);
                                            } else {
                                                days.push(day);
                                                days.sort_unstable();
                                            }
                                            cx.notify();
                                        }))
                                    }),
                                ),
                            ))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(native::LABEL_GAP))
                                    .child(
                                        native::radio(
                                            "once",
                                            t("Once, on this date").into(),
                                            once,
                                            theme,
                                            &screen,
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.draft.once = true;
                                            cx.notify();
                                        })),
                                    )
                                    .child(enabled(once, field(&self.date, "Date (YYYY-MM-DD)", 110.0))),
                            )
                            // Where the system lets dm wake the computer.
                            .when(dm_desktop_base::wake::possible(), |d| {
                                d.child(
                                    native::checkbox(
                                        "wake",
                                        t("Wake the computer from sleep to start").into(),
                                        wake,
                                        theme,
                                        &screen,
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.draft.schedule.wake = !this.draft.schedule.wake;
                                        cx.notify();
                                    })),
                                )
                            })
                            // What dm does when the scheduled downloads are
                            // done: through the countdown that can be
                            // cancelled, as Downloads › When done is.
                            .child(screen.label(t("When the scheduled downloads are done")))
                            .child(
                                div().flex().flex_wrap().gap(px(12.0)).children(
                                    [
                                        (WhenDone::Nothing, Command::WhenDoneNothing),
                                        (WhenDone::Quit, Command::WhenDoneQuit),
                                        (WhenDone::Sleep, Command::WhenDoneSleep),
                                        (WhenDone::Hibernate, Command::WhenDoneHibernate),
                                        (WhenDone::ShutDown, Command::WhenDoneShutDown),
                                    ]
                                    .into_iter()
                                    .map(|(w, command)| {
                                        // As the Downloads menu greys them:
                                        // only what this computer can do.
                                        let can = w.possible(dm_desktop_base::power::available);
                                        enabled(
                                            can,
                                            native::radio(
                                                command.id(),
                                                t(command.label()).into(),
                                                when_done == w,
                                                theme,
                                                &screen,
                                            )
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.draft.schedule.when_done = w.word().to_string();
                                                cx.notify();
                                            })),
                                        )
                                    }),
                                ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(native::LABEL_GAP))
                                    .child(screen.label(t(
                                        "Try a failed download again this many times (0 for none)",
                                    )))
                                    .child(field(&self.retries, "Try a failed download again this many times (0 for none)", 50.0)),
                            )
                            // A program run once when they are done, as
                            // Options' command is after each file: no shell.
                            .child(screen.label(t("Run this program when they are done (optional)")))
                            .child(native::field(
                                &self.run,
                                "Run this program when they are done (optional)",
                                theme,
                            ))
                            .when(cfg!(windows), |d| {
                                let hang = self.draft.schedule.hang_up;
                                d.child(
                                    native::checkbox(
                                        "hang-up",
                                        t("Hang up modem when done").into(),
                                        hang,
                                        theme,
                                        &screen,
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.draft.schedule.hang_up = !this.draft.schedule.hang_up;
                                        cx.notify();
                                    })),
                                )
                            })
                            .child(native::note(
                                t("HH:MM, local time. A window ending before it starts runs overnight."),
                                theme.muted,
                                &screen,
                            )),
                    )),
            ));

        div()
            .id("scheduler")
            .role(Role::Dialog)
            .aria_label(t("Scheduler"))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => this.cancel(window),
                    "enter" => this.ok(window, cx),
                    _ => {}
                }
            }))
            .size_full()
            .bg(theme.window)
            .child(crate::fit::fitted(
                native::body(theme)
                    .child(
                        div()
                            .flex()
                            .gap(px(16.0))
                            .child(queues)
                            .child(div().w(px(1.0)).flex_none().bg(theme.separator))
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
    let Some(downloads) = view.upgrade() else {
        return;
    };
    let backend = downloads.read(cx).backend.clone();
    let settings = backend.settings();
    let queues = backend.queues();
    let bounds = Bounds::centered(None, size(px(720.0), px(520.0)), cx);
    let opened = cx.open_window(
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
            let from = input(window, cx);
            let to = input(window, cx);
            let date = input(window, cx);
            let retries = input(window, cx);
            let run = input(window, cx);
            let scheduler = cx.new(|cx| {
                // Return in a field is OK.
                for input in [&from, &to, &date, &retries, &run] {
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
                    from,
                    to,
                    date,
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
            crate::native::close_is_cancel(&scheduler, window, cx);
            cx.new(|cx| gpui_component::Root::new(scheduler, window, cx))
        },
    );
    if let Err(e) = opened {
        eprintln!("dm-gpui: the Scheduler could not be opened: {e}");
    }
}

impl crate::native::CloseButton for Scheduler {
    fn close_button(&mut self, window: &mut Window, _: &mut Context<Self>) {
        self.cancel(window);
    }
}
