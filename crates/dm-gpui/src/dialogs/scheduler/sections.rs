// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The Scheduler's sections, each a piece of the window that `render`
//! puts together in its order.

use super::*;

impl Scheduler {
    /// The queues on the left, as reference A lists them, as wide as its two
    /// buttons side by side.
    pub(super) fn queue_list(
        &self,
        theme: Theme,
        screen: &Screen,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        div()
            .map(|list| {
                if crate::platform::ON_A_PHONE {
                    list.w_full()
                } else {
                    list.w(px(2.0 * native::BUTTON_MIN + native::LABEL_GAP + 4.0))
                }
            })
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(space::XXS))
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .mb(px(space::XS))
                    .child(screen.label(t("Queues"))),
            )
            .children(self.queues.iter().enumerate().map(|(i, queue)| {
                let chosen = *queue == self.draft.queue;
                let q = queue.clone();
                div()
                    .id(("queue", i))
                    .map(|c| crate::widgets::native::ringed(c, theme))
                    .aria_selected(chosen)
                    .role(Role::ListItem)
                    .aria_label(dm_desktop_base::i18n::name(queue))
                    .h(px(24.0))
                    .px(px(space::M))
                    .flex()
                    .items_center()
                    .rounded(px(native::RADIUS))
                    .when(chosen, |d| d.bg(theme.accent).text_color(theme.on_accent))
                    .when(!chosen, |d| d.hover(|s| s.bg(theme.stripe)))
                    .child(screen.label(dm_desktop_base::i18n::name(queue)))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.choose(q.clone(), window, cx)),
                    )
            }))
            .child(
                div()
                    .mt(px(space::M))
                    .flex()
                    .gap(px(native::LABEL_GAP))
                    .child(
                        native::button("queue-start", t("Start now").into(), false, theme, screen)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.run_on_queue(Command::StartQueue, cx)
                            })),
                    )
                    .child(
                        native::button("queue-stop", t("Stop").into(), false, theme, screen)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.run_on_queue(Command::StopQueue, cx)
                            })),
                    ),
            )
    }

    /// The chosen queue's hours, on the right.
    pub(super) fn hours(&self, theme: Theme, screen: &Screen, cx: &mut Context<Self>) -> gpui::Div {
        let main = self.draft.queue == MAIN_QUEUE;
        let own = self.draft.own;
        let on = self.draft.schedule.enabled;
        div()
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
                    native::switch_inline(
                        "own",
                        t("Hours of its own for this queue").into(),
                        own,
                        theme,
                        screen,
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
                        screen,
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
                        native::switch_inline(
                            "enabled",
                            t("Only download between set hours").into(),
                            on,
                            theme,
                            screen,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.draft.schedule.enabled = !this.draft.schedule.enabled;
                            cx.notify();
                        })),
                    )
                    .child(enabled(on, self.timed(theme, screen, cx))),
            ))
    }

    /// The hours, the days or the date, and what is done after: all greyed
    /// while downloads are not held to set hours.
    pub(super) fn timed(&self, theme: Theme, screen: &Screen, cx: &mut Context<Self>) -> gpui::Div {
        let wake = self.draft.schedule.wake;
        let d = div().flex().flex_col().gap(px(native::ROW_GAP)).child(
            // Each time with its words, Stop at under Start at where a line has no room for
            // both (a phone, gaps row N31).
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(native::LABEL_GAP))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(native::LABEL_GAP))
                        .child(screen.label(t("Start at")))
                        .child(native::time_picker(
                            ("from-hour", "from-minute"),
                            &self.draft.schedule.from,
                            theme,
                            screen,
                        )),
                )
                .child(div().w(px(space::M)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(native::LABEL_GAP))
                        .child(screen.label(t("Stop at")))
                        .child(native::time_picker(
                            ("to-hour", "to-minute"),
                            &self.draft.schedule.to,
                            theme,
                            screen,
                        )),
                ),
        );
        // The hint directly under the hours it explains (gaps row E10), not at the foot.
        let d = d.child(native::note(
            t("HH:MM, local time. A window ending before it starts runs overnight."),
            theme.muted,
            screen,
        ));
        let d = self
            .repeat(theme, screen, d, cx)
            // Where the system lets dm wake the computer.
            .when(dm_desktop_base::wake::possible(), |d| {
                d.child(
                    native::switch_inline(
                        "wake",
                        t("Wake the computer from sleep to start").into(),
                        wake,
                        theme,
                        screen,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.draft.schedule.wake = !this.draft.schedule.wake;
                        cx.notify();
                    })),
                )
            });
        self.when_done(theme, screen, d, cx)
    }

    /// Every week on the days ticked, or once on a date, as reference A's "Daily" and
    /// "Once at".
    pub(super) fn repeat(
        &self,
        theme: Theme,
        screen: &Screen,
        d: gpui::Div,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let once = self.draft.once;
        let days = self.draft.schedule.days.clone();
        d.child(
            native::radio(
                "weekly",
                t("Every week, on the days ticked").into(),
                !once,
                theme,
                screen,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.draft.once = false;
                cx.notify();
            })),
        )
        .child(enabled(
            !once,
            div()
                .pl(px(space::XXL))
                .flex()
                .flex_wrap()
                .gap(px(space::M_L))
                .children(DAYS.iter().enumerate().map(|(day, word)| {
                    let day = day as u8;
                    let ticked = days.contains(&day);
                    native::checkbox(("day", day as usize), t(word).into(), ticked, theme, screen)
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
                })),
        ))
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(native::LABEL_GAP))
                .child(
                    native::radio("once", t("Once, on this date").into(), once, theme, screen)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.draft.once = true;
                            cx.notify();
                        })),
                )
                .child(enabled(once, date_picker(&self.draft.date, theme, screen))),
        )
    }

    /// What dm does when the scheduled downloads are done: through the countdown that can be
    /// cancelled, as Downloads › When done is. Then how often a failed one is tried again, and
    /// the program run then.
    pub(super) fn when_done(
        &self,
        theme: Theme,
        screen: &Screen,
        d: gpui::Div,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let when_done = WhenDone::from_word(&self.draft.schedule.when_done);
        let field = |input: &Entity<InputState>, name: &'static str, w: f32| {
            div().w(px(w)).child(native::field(input, name, theme))
        };
        d.child(screen.label(t("When the scheduled downloads are done")))
            .child(
                div().flex().flex_wrap().gap(px(space::L)).children(
                    [
                        (WhenDone::Nothing, Command::WhenDoneNothing),
                        (WhenDone::Quit, Command::WhenDoneQuit),
                        (WhenDone::Sleep, Command::WhenDoneSleep),
                        (WhenDone::Hibernate, Command::WhenDoneHibernate),
                        (WhenDone::ShutDown, Command::WhenDoneShutDown),
                    ]
                    .into_iter()
                    // A phone cannot sleep, hibernate or turn off for dm: not shown there.
                    .filter(|(w, _)| {
                        !crate::platform::ON_A_PHONE
                            || w.possible(dm_desktop_base::power::available)
                    })
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
                                screen,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.draft.schedule.when_done = w.word().to_string();
                                    cx.notify();
                                },
                            )),
                        )
                    }),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(native::LABEL_GAP))
                    // Its words wrap within the window: as an item of their own width they ran
                    // past its edge in Windows' wider type (gaps row E2).
                    .child(div().flex_1().min_w_0().child(screen.label(t(
                        "Try a failed download again this many times (0 for none)",
                    ))))
                    .child(field(
                        &self.retries,
                        "Try a failed download again this many times (0 for none)",
                        50.0,
                    )),
            )
            // A program run once when they are done, as
            // Options' command is after each file: no shell. A phone runs no programs.
            .when(!crate::platform::ON_A_PHONE, |d| {
                d.child(screen.label(t("Run this program when they are done (optional)")))
                    .child(native::field(
                        &self.run,
                        "Run this program when they are done (optional)",
                        theme,
                    ))
            })
            .when(cfg!(windows), |d| {
                let hang = self.draft.schedule.hang_up;
                d.child(
                    native::switch_inline(
                        "hang-up",
                        t("Hang up modem when done").into(),
                        hang,
                        theme,
                        screen,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.draft.schedule.hang_up = !this.draft.schedule.hang_up;
                        cx.notify();
                    })),
                )
            })
    }
}
