// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The list window's pieces, each a piece of the element tree that `render`
//! puts together in this order: the bars above the list, the root's actions
//! and keys, and below the bars the sidebar beside the list's column.

use gpui::{div, prelude::*, AnyElement, Context, Div, Stateful, Window};

use super::{scrollbar_mode, Downloads};
use crate::widgets::space;
use crate::window::screen::Screen;
use crate::window::theme::Theme;

/// What the list's column shows: the bars, how many more wait, and whether
/// the list has no row.
pub(crate) struct Shown {
    /// The bars shown: the first, or all of them once asked for.
    pub(crate) bars: Vec<AnyElement>,
    /// How many bars wait behind the one shown.
    pub(crate) more: usize,
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
        let mut bars: Vec<gpui::AnyElement> = Vec::new();
        bars.extend(
            self.awaiting_bar(theme, screen, cx)
                .map(IntoElement::into_any_element),
        );
        bars.extend(
            self.clipboard_bar(theme, screen, cx)
                .map(IntoElement::into_any_element),
        );
        bars.extend(update_bar.map(IntoElement::into_any_element));
        bars.extend(
            self.stalled_bar(theme, screen, cx)
                .map(IntoElement::into_any_element),
        );
        // Problems from outside the list, each until dismissed.
        bars.extend(self.status.problems.iter().enumerate().map(|(i, problem)| {
            gpui::div()
                .flex()
                .items_center()
                .gap(crate::window::zoom::px(crate::widgets::native::LABEL_GAP))
                .px(crate::window::zoom::px(space::L))
                .py(crate::window::zoom::px(space::S))
                .border_b_1()
                .border_color(theme.separator)
                .child(
                    gpui::div()
                        .flex_1()
                        .text_color(theme.warning)
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
                )
                .into_any_element()
        }));
        bars.extend(
            self.failed_bar(theme, screen, cx)
                .map(IntoElement::into_any_element),
        );
        bars.extend(browser_bar.map(IntoElement::into_any_element));
        if bars.len() <= 1 {
            // Asked for all, they stand together until one or none is left.
            self.layout.bars.all = false;
        }
        let more_bars = if self.layout.bars.all {
            0
        } else {
            bars.len().saturating_sub(1)
        };
        let shown_bars: Vec<gpui::AnyElement> = if self.layout.bars.all {
            bars
        } else {
            bars.into_iter().take(1).collect()
        };
        let empty = self.list.shown.is_empty();
        Shown {
            bars: shown_bars,
            more: more_bars,
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
        // The row menu's "Delete file after" (gaps row U8).
        .on_action(
            cx.listener(|this, it: &crate::window::rowmenu::DeleteAfter, _, cx| {
                this.delete_after_chosen(it.seconds, cx)
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
            cx.listener(|this, _: &crate::window::menus::Dismiss, _, cx| {
                if this.menus.place_menu.is_some() {
                    this.close_place_menu();
                    cx.notify();
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
            more: more_bars,
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
            .children(self.more_bars_line(more_bars, theme, screen, cx))
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
    }

    /// "+2 more" under the bar shown, where `more` bars wait behind it: a click shows them all.
    pub(crate) fn more_bars_line(
        &self,
        more: usize,
        theme: Theme,
        screen: &Screen,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Stateful<Div>> {
        (more > 0).then(|| {
            div()
                .id("more-bars")
                .px(crate::window::zoom::px(space::L))
                .py(crate::window::zoom::px(space::XXS))
                .border_b_1()
                .border_color(theme.separator)
                .text_size(crate::window::zoom::px(crate::widgets::native::SMALL_TEXT))
                .text_color(theme.selection)
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.layout.bars.all = true;
                    cx.notify();
                }))
                .child(screen.label(dm_desktop_base::i18n::tn(
                    "+{} more||+{} more",
                    more as u64,
                    &[&more],
                )))
        })
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
                                !empty && scrollbar_mode() == gpui_base::ScrollbarMode::Always,
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
                            .right_0()
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
                                    .mode(scrollbar_mode()),
                            ),
                    )
            })
    }
}
