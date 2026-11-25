// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The list window's pieces, each a piece of the element tree that `render`
//! puts together in this order: the bars above the list, the root's actions
//! and keys, and below the bars the sidebar beside the list's column.

use gpui::{div, prelude::*, AnyElement, Context, Div, Stateful, Window};

use super::{scrollbar_mode, Downloads};
use crate::window::screen::Screen;
use crate::window::theme::Theme;

/// What the list's column shows: the bars, how many more wait, and whether
/// the list has no row.
pub(crate) struct Shown {
    /// The bars shown: the first (with how many more wait), or all of them once asked for.
    pub(crate) bars: Vec<AnyElement>,
    /// No row is shown.
    pub(crate) empty: bool,
}

impl Downloads {
    /// The bars above the list, `update_bar` and `browser_bar` among them,
    /// one at a time unless all were asked for.
    pub(super) fn shown_bars(
        &mut self,
        update_bar: Option<Div>,
        browser_bar: Option<Div>,
        theme: Theme,
        screen: &Screen,
        cx: &mut Context<Self>,
    ) -> Shown {
        // In the order they matter: what the person just did (a link awaited from the
        // browser, one copied), dm's update, a download stalled for room, problems, failures,
        // and the way to add dm to a browser last (gaps row U26).
        use crate::window::bars::Tone;
        let mut bars: Vec<(Tone, gpui::Div)> = Vec::new();
        bars.extend(
            self.awaiting_bar(theme, screen, cx)
                .map(|b| (Tone::Note, b)),
        );
        bars.extend(
            self.clipboard_bar(theme, screen, cx)
                .map(|b| (Tone::Note, b)),
        );
        bars.extend(update_bar.map(|b| (Tone::Note, b)));
        bars.extend(
            self.stalled_bar(theme, screen, cx)
                .map(|b| (Tone::Failure, b)),
        );
        // Problems from outside the list, each until dismissed.
        bars.extend(self.status.problems.iter().enumerate().map(|(i, problem)| {
            let body = crate::window::bars::body()
                .child(
                    gpui::div()
                        .flex_1()
                        .child(screen.label(dm_desktop_base::format::as_shown(problem))),
                )
                .child(
                    crate::widgets::native::button(
                        ("dismiss", i),
                        dm_desktop_base::i18n::t("Dismiss").into(),
                        false,
                        theme,
                        screen,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if i < this.status.problems.len() {
                            this.status.problems.remove(i);
                        }
                        cx.notify();
                    })),
                );
            (Tone::Failure, body)
        }));
        bars.extend(
            self.failed_bar(theme, screen, cx)
                .map(|b| (Tone::Failure, b)),
        );
        bars.extend(browser_bar.map(|b| (Tone::Note, b)));
        if bars.len() <= 1 {
            // Asked for all, they stand together until one or none is left.
            self.layout.bars.all = false;
        }
        // One bar at a time, "+N more" at its end (gaps rows U26, E5); all once asked for.
        let more = if self.layout.bars.all {
            0
        } else {
            bars.len().saturating_sub(1)
        };
        let kept = if self.layout.bars.all { bars.len() } else { 1 };
        let shown_bars: Vec<gpui::AnyElement> = bars
            .into_iter()
            .take(kept)
            .enumerate()
            .map(|(n, (tone, body))| {
                let more = if n == 0 { more } else { 0 };
                crate::window::bars::framed(tone, body, more, theme, screen, cx)
            })
            .collect();
        let empty = self.list.shown.is_empty();
        Shown {
            bars: shown_bars,
            empty,
        }
    }

    /// The window's actions and keys, on its root.
    pub(super) fn list_actions(root: Stateful<Div>, cx: &mut Context<Self>) -> Stateful<Div> {
        root.on_action(
            cx.listener(|this, run: &crate::window::menus::RunOnQueue, _, cx| {
                this.run_on_queue(run.command, &run.queue, cx)
            }),
        )
        // A place sheet's item (a phone's long press on a place).
        .on_action(
            cx.listener(|this, on: &crate::window::placemenu::OnPlace, window, cx| {
                this.on_place(on, window, cx)
            }),
        )
        // A phone card menu's Select: choosing several starts with the card it was opened on.
        .on_action(
            cx.listener(|this, _: &crate::window::rowmenu::ChooseSeveral, _, cx| {
                this.layout.narrow.choosing = true;
                cx.notify();
            }),
        )
        // A phone card menu's Share.
        .on_action(
            cx.listener(|this, _: &crate::window::rowmenu::ShareFile, _, cx| this.share_chosen(cx)),
        )
        // The row menu's "Delete file after" (gaps row U8).
        .on_action(
            cx.listener(|this, it: &crate::window::rowmenu::DeleteAfter, _, cx| {
                this.delete_after_chosen(it.seconds, cx)
            }),
        )
        // Help › Copy a report (gaps row W8).
        .on_action(
            cx.listener(|this, _: &crate::window::menus::CopyReport, _, cx| {
                let settings = this.backend.settings();
                let rows: Vec<dm_core::Entry> = this.rows.iter().map(|r| (**r).clone()).collect();
                let text = dm_desktop_base::details::report(
                    &rows,
                    settings.downloads.connections,
                    &settings.folders.download_dir,
                );
                crate::platform::services::copy(&*this.backend.platform, &text, cx);
                this.say(
                    dm_desktop_base::i18n::t(
                        "Copied a report to the clipboard, for an issue: nothing was sent.",
                    )
                    .into(),
                );
                cx.notify();
            }),
        )
        .on_action(
            cx.listener(|this, _: &crate::window::menus::TipOfTheDay, _, cx| {
                this.tip_of_the_day(cx)
            }),
        )
        // View › Statistics (gaps row W15).
        .on_action(cx.listener(
            |this, _: &crate::dialogs::statistics::ShowStatistics, _, cx| this.open_statistics(cx),
        ))
        // View › Columns, Customize toolbar and Language.
        .on_action(
            cx.listener(|this, it: &crate::window::viewmenu::ToggleColumn, _, cx| {
                this.toggle_column(&it.key, cx)
            }),
        )
        .on_action(
            cx.listener(|this, it: &crate::window::viewmenu::ToolbarStyle, _, cx| {
                this.set_toolbar_style(&it.style, cx)
            }),
        )
        .on_action(cx.listener(
            |this, it: &crate::window::viewmenu::ToggleToolbarButton, _, cx| {
                this.toggle_toolbar_button(&it.id, cx)
            },
        ))
        .on_action(
            cx.listener(|this, it: &crate::window::viewmenu::SetLanguage, _, cx| {
                this.set_language(&it.code, cx)
            }),
        )
        // Files dropped on the window: the links they hold added.
        .on_drop(cx.listener(|this, paths: &gpui::ExternalPaths, _, cx| {
            let paths = paths.paths().to_vec();
            this.dropped_files(&paths, cx);
        }))
        .on_action(
            cx.listener(|this, _: &crate::window::menus::CloseWindow, window, cx| {
                if this.close_asked(window, cx) {
                    window.remove_window();
                }
            }),
        )
        .on_action(
            cx.listener(|this, run: &crate::window::menus::Run, window, cx| {
                use dm_shell::app::commands::Command;
                // Held by Options: its window comes forward instead (B42).
                if this.bring_holder(cx) {
                    return;
                }
                match run.0 {
                    // The search box is the window's: these need it.
                    Command::Find => this.find(window, cx),
                    Command::FindNext => {
                        this.find_step(true);
                        cx.notify();
                    }
                    Command::FindPrevious => {
                        this.find_step(false);
                        cx.notify();
                    }
                    command => this.run(command, cx),
                }
            }),
        )
        // Edit › Undo, Copy and Paste where no text field has the keys (a field's own come
        // first, in its context): Undo puts back what Remove took off, Copy copies the chosen
        // downloads' links, and Paste takes the links on the clipboard (gaps row U9).
        .on_action(cx.listener(|this, _: &gpui_base::input::Undo, _, cx| {
            this.undo_remove(cx);
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &gpui_base::input::Copy, _, cx| {
            use dm_shell::app::commands::Command;
            if !this.bring_holder(cx) {
                this.run(Command::CopyLink, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &gpui_base::input::Paste, _, cx| {
            use dm_shell::app::commands::Command;
            if !this.bring_holder(cx) {
                this.run(Command::AddFromClipboard, cx);
            }
        }))
    }

    /// The window's keys for the list and its menus, on its root after its
    /// actions.
    pub(super) fn list_keys(root: Stateful<Div>, cx: &mut Context<Self>) -> Stateful<Div> {
        root.on_action(
            cx.listener(|this, _: &crate::window::menus::SelectPrevious, _, cx| {
                this.step(-1);
                cx.notify();
            }),
        )
        .on_action(
            cx.listener(|this, _: &crate::window::menus::SelectNext, _, cx| {
                // The row menu is the system's, which takes the arrows while it is open.
                this.step(1);
                cx.notify();
            }),
        )
        .on_action(
            cx.listener(|this, _: &crate::window::menus::Confirm, window, cx| {
                // Return in the search box is the box's own (Find next): passed on to its field,
                // whose binding comes after this one. Taken here, it opened the chosen row's
                // progress window and never found the next.
                if gpui::Focusable::focus_handle(this.list.filter.read(cx), cx)
                    .contains_focused(window, cx)
                {
                    cx.propagate();
                    return;
                }
                this.run(dm_shell::app::commands::Command::ShowProgress, cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &crate::window::menus::Dismiss, window, cx| {
                // The phone's layout: its sheet or drawer closes first (gaps row N16: a
                // phone's Back is Escape given to the window on top).
                if crate::window::narrow::is_narrow(window) && this.narrow_back() {
                    cx.notify();
                    return;
                }
                if this.menus.place_menu.is_some() {
                    this.close_place_menu(window, cx);
                    cx.notify();
                } else if cfg!(target_os = "android") {
                    // Nothing to close: passed on, so the phone's platform, told no one used
                    // it, sends dm to the background as Back does on a first screen.
                    cx.propagate();
                }
            }),
        )
        // A letter typed over the list finds the next row whose name starts with what was
        // typed (gaps row U6); not while a field or the search box has the keys.
        .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
            let held = &event.keystroke.modifiers;
            if !this.focus.is_focused(window)
                || held.platform
                || held.control
                || held.alt
                || held.function
            {
                return;
            }
            if let Some(typed) = event.keystroke.key_char.as_deref() {
                if typed.chars().count() == 1 && typed.chars().all(|c| !c.is_control()) {
                    this.type_to_find(typed);
                    cx.notify();
                }
            }
        }))
        .on_action(
            cx.listener(|this, m: &crate::window::menus::MoveInList, _, cx| {
                use crate::window::menus::ListMove as M;
                use dm_desktop_base::selection::Step;
                let step = match m.to {
                    M::Up => Step::Up,
                    M::Down => Step::Down,
                    M::First => Step::First,
                    M::Last => Step::Last,
                    M::PageUp => Step::PageUp(this.page()),
                    M::PageDown => Step::PageDown(this.page()),
                };
                this.step_by(step, m.shift);
                this.refresh_menus(cx);
                cx.notify();
            }),
        )
        .on_action(
            cx.listener(|this, _: &crate::window::menus::MenuBarByKey, window, cx| {
                this.menu_bar_by_key(window, cx);
            }),
        )
        .on_action(cx.listener(
            |this, _: &crate::window::menus::RowMenuByKey, window, cx| {
                this.row_menu_by_key(window, cx);
                cx.notify();
            },
        ))
    }

    /// Under the bars and toolbar: the sidebar, beside the list's column.
    pub(super) fn body(
        &mut self,
        theme: Theme,
        screen: &Screen,
        shown: Shown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .flex()
            // The side panel on the right where the window is mirrored (gaps row U35).
            .map(crate::window::mirror::row)
            .flex_1()
            .min_h_0()
            .when(self.layout.show_sidebar, |row| {
                row.child(crate::window::sidebar::sidebar(
                    self, theme, screen, window, cx,
                ))
            })
            .child(self.list_column(theme, screen, shown, cx))
    }

    /// The list's column: the bars, the table, and what an empty list says.
    fn list_column(
        &mut self,
        theme: Theme,
        screen: &Screen,
        shown: Shown,
        cx: &mut Context<Self>,
    ) -> Div {
        let Shown {
            bars: shown_bars,
            empty,
        } = shown;
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .bg(theme.background)
            // The bars above the list, one at a time with how many more
            // wait (gaps row U26): stacked, a failure, the browser's bar and a
            // problem or two took 135 points of a 500-point window.
            .children(shown_bars)
            // The headings and the rows scroll sideways
            // together where the window is narrower than
            // the columns, as a macOS list does: at 120% in
            // a narrow window the columns after Status were
            // out of reach. The sideways gesture alone moves
            // them; the wheel still moves the rows. With no
            // row, what the list says stands under them in
            // the middle of what is in sight, not of the
            // columns' whole width.
            .child(self.table_frame(theme, screen, empty, cx))
            .when(empty, |list| {
                list.child(crate::window::table::empty(self, theme, cx))
            })
            // The chosen download's detail under the list (View › Details, gaps row E8).
            .children(self.detail_pane(theme, screen, cx))
    }

    /// The headings and the rows, which scroll sideways together, and the
    /// list's bars over them.
    fn table_frame(
        &mut self,
        theme: Theme,
        screen: &Screen,
        empty: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        if crate::window::mirror::rtl() {
            self.keep_sideways_from_right(cx);
        }
        // Where the columns run past the list's edge, the sideways bar stays in sight on every
        // system: at a larger text size whole columns lay past the edge with nothing to say so,
        // macOS showing its bar only under the pointer (`docs/ui-design.md` rule 7).
        let sideways_mode = if self.list.sideways.max_offset().x > gpui::px(0.0) {
            gpui_base::ScrollbarMode::Always
        } else {
            scrollbar_mode()
        };
        div()
            .relative()
            .flex()
            .when(empty, |frame| frame.flex_none())
            .when(!empty, |frame| frame.flex_1().min_h_0())
            .child(
                div()
                    .id("table")
                    .map(|table| crate::window::table::told_table(table, self))
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_x_scroll()
                    .restrict_scroll_to_axis()
                    .track_scroll(&self.list.sideways)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .when(!empty, |list| list.h_full())
                            // The rows end above the sideways bar
                            // where it is always shown: over them, it
                            // hid the last row's foot and took a click
                            // on it (Linux, gaps row Q1).
                            .when(
                                !empty && sideways_mode == gpui_base::ScrollbarMode::Always,
                                |list| list.pb(gpui_base::Scrollbar::width()),
                            )
                            .w_full()
                            .min_w(crate::window::zoom::px(crate::window::table::least_width(
                                self,
                            )))
                            .child(crate::window::table::header(self, theme, screen, cx))
                            .when(!empty, |list| {
                                list.child(crate::window::table::rows(self, theme, cx))
                            }),
                    ),
            )
            // The list's bars, each dragged (gaps row U43): the up-and-down
            // one at the list's visible right edge, however far the columns
            // run past it; the sideways one along its foot.
            .when(!empty, |frame| {
                frame
                    .child(
                        div()
                            .absolute()
                            .top(crate::window::zoom::px(crate::window::table::HEADER_HEIGHT))
                            // At the left edge where the window is mirrored (gaps row U35).
                            .map(|bar| {
                                if crate::window::mirror::rtl() {
                                    bar.left_0()
                                } else {
                                    bar.right_0()
                                }
                            })
                            .bottom_0()
                            .w(gpui_base::Scrollbar::width())
                            .child(
                                gpui_base::Scrollbar::vertical(&self.list.scroll)
                                    .viewport_from_layout()
                                    .mode(scrollbar_mode()),
                            ),
                    )
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .h(gpui_base::Scrollbar::width())
                            .child(
                                gpui_base::Scrollbar::horizontal(&self.list.sideways)
                                    .viewport_from_layout()
                                    .mode(sideways_mode),
                            ),
                    )
            })
    }
}

impl Downloads {
    /// Where the window is mirrored, the columns run from the right and the file name's is at
    /// the right end of what scrolls sideways, which starts at its left (gaps row U35): the list
    /// is kept as far from its right end as a person moved it, none at first, so the name is in
    /// sight. The room to move is the last layout's; where it changed, the list is drawn again.
    fn keep_sideways_from_right(&mut self, cx: &mut Context<Self>) {
        let handle = &self.list.sideways;
        let room = handle.max_offset().x;
        let at = handle.offset().x;
        let (now, before) = self.list.drawn_width;
        let (mut from_right, last_set) = self.list.sideways_from_right;
        // A person's move (the wheel, the sideways bar): the offset is not the one last set,
        // while the window's width and the room are as they were. A resize or a new layout
        // can leave the offset anywhere, and is not read as one (`at` runs from 0 to -room).
        if now == before && self.list.sideways_room == Some(room) && at != last_set {
            from_right = (room + at).max(gpui::px(0.0));
        }
        let set = -(room - from_right).max(gpui::px(0.0));
        handle.set_offset(gpui::point(set, handle.offset().y));
        self.list.sideways_from_right = (from_right, set);
        // The room is the last layout's: where it or the window's width changed, the list is
        // drawn once more with the room measured anew.
        if self.list.sideways_room != Some(room) || now != before {
            self.list.sideways_room = Some(room);
            cx.notify();
        }
    }
}
