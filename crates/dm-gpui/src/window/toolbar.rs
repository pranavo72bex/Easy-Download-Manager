// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The toolbar, a row of its own under the top bar (`titlebar.rs`), as a
//! code editor keeps its bars under its title bar: a picture with its word
//! under it (Finder's "Icon and Text"). The buttons are the application's
//! (`commands::toolbar_shown`: the person's order, less what they hid).

use dm_desktop_base::i18n::t;
use dm_shell::app::commands::Command;
use gpui::{div, prelude::*, svg, Context, Role, Window};

use crate::widgets::space;
use crate::window::a11y::Told;
use crate::window::list::Downloads;
use crate::window::menus::Run;
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

mod more;

/// The toolbar's height.
pub const HEIGHT: f32 = 50.0;

/// The picture on a toolbar command's button.
pub fn icon(command: Command) -> &'static str {
    match command {
        Command::AddUrl => "icons/add.svg",
        Command::DownloadNow => "icons/resume.svg",
        Command::StopDownload => "icons/stop.svg",
        Command::StopAll => "icons/stop-all.svg",
        Command::Remove => "icons/delete.svg",
        Command::DeleteCompleted => "icons/delete-completed.svg",
        Command::Options => "icons/options.svg",
        Command::Scheduler => "icons/scheduler.svg",
        Command::StartQueue => "icons/start-queue.svg",
        Command::StopQueue => "icons/stop-queue.svg",
        _ => "icons/grabber.svg",
    }
}

/// The toolbar.
/// A button's face under the pointer: the main action's accent a shade toward the text, any
/// other the stripe.
fn hovered(main: bool, theme: Theme) -> gpui::Rgba {
    if main {
        theme.answering(theme.accent, crate::window::theme::HOVERED)
    } else {
        theme.stripe
    }
}

/// Where the room before the rest of the toolbar goes (gaps row E3): after the leading run of
/// Add URL and what acts on the chosen rows, then the rest stand at the end. Where a person
/// moved the buttons, the run ends where they put another command.
fn rest_after(buttons: &[(Command, &'static str)]) -> Option<usize> {
    let leading = buttons
        .iter()
        .take_while(|(c, _)| {
            matches!(
                c,
                Command::AddUrl | Command::DownloadNow | Command::StopDownload | Command::Remove
            )
        })
        .count();
    (leading > 0 && leading < buttons.len()).then_some(leading)
}

pub fn toolbar(
    view: &Downloads,
    theme: Theme,
    screen: &Screen,
    window: &mut Window,
    cx: &mut Context<Downloads>,
) -> impl IntoElement {
    let settings = &view.known.drawn;
    let buttons = dm_shell::app::commands::toolbar_shown(settings);
    // View › Customize toolbar: a picture with its word, the picture alone
    // (its word on hover), or the word alone.
    let style = dm_desktop_base::toolbar::ToolStyle::named(&settings.toolbar_style);
    let picture = style != dm_desktop_base::toolbar::ToolStyle::Text;
    let worded = style != dm_desktop_base::toolbar::ToolStyle::Icons;
    // Resume says what it will do for the rows chosen (Retry, Start now), as
    // the menus do.
    let resume = dm_desktop_base::rows::resume_word(&view.chosen());
    // As many as fit across the window; the rest under More (gaps row E3).
    let widths: Vec<f32> = buttons
        .iter()
        .map(|&(command, words)| more::button_width(command, t(words), picture, worded))
        .collect();
    let room = f32::from(window.viewport_size().width) / crate::window::zoom::factor();
    let mut buttons = buttons;
    let rest = buttons.split_off(more::fitting(
        &widths,
        room,
        more::more_width(picture, worded),
    ));
    let rest_after = rest_after(&buttons);
    // More stands at the end, after the room, where the room is not already before it.
    let more = (!rest.is_empty()).then(|| {
        div()
            .flex()
            .when(rest_after.is_none(), |d| d.ml_auto())
            .child(more::more_button(
                rest,
                view,
                theme,
                (picture, worded),
                screen,
            ))
    });
    div()
        .id("toolbar")
        .role(Role::Toolbar)
        .aria_label(t("Toolbar"))
        // A row as tall as a button; where the buttons do not fit across the window (a narrow
        // one, a larger text size), those at the end go under More (`more.rs`). Wrapped to a
        // second row only where the measure falls short.
        .min_h(px(HEIGHT))
        .flex_none()
        .flex()
        // The buttons from the right where the window is mirrored (gaps row U35).
        .map(crate::window::mirror::row)
        .flex_wrap()
        .items_center()
        .gap_1()
        .px_2()
        .border_b_1()
        .border_color(theme.separator)
        .children(
            buttons
                .into_iter()
                .enumerate()
                .map(|(at, (command, words))| {
                    button(
                        view,
                        (at, command, words),
                        rest_after,
                        resume,
                        (picture, worded),
                        theme,
                        screen,
                        cx,
                    )
                }),
        )
        .children(more)
}

/// One toolbar button, `at` its place among those drawn: its picture and word as the style has
/// them, greyed where it cannot act, dragged to a new place, and the room before it where the
/// rest begin (`rest_after`).
#[allow(clippy::too_many_arguments)]
fn button(
    view: &Downloads,
    (at, command, words): (usize, Command, &'static str),
    rest_after: Option<usize>,
    resume: &'static str,
    (picture, worded): (bool, bool),
    theme: Theme,
    screen: &Screen,
    cx: &mut Context<Downloads>,
) -> gpui::AnyElement {
    // The window's main action, filled with the accent, the one filled button there
    // (`docs/ui-design.md`, rule 1).
    let main = command == Command::AddUrl;
    let enabled = view.enabled(command);
    let why_off = view.why_off(command);
    let words = if command == dm_shell::app::commands::Command::DownloadNow
        && matches!(resume, "Retry" | "Start now")
    {
        t(resume)
    } else {
        t(words)
    };
    // Start Queue and Stop Queue have an arrow beside them that
    // lists the queues; the one chosen is started or stopped,
    // whatever the list shows (B64).
    let queues = matches!(command, Command::StartQueue | Command::StopQueue)
        .then(|| view.known.queue_list.clone());
    let arrow = queues.map(|queues| queue_arrow(command, queues, theme));
    let button = div()
        .id(command.id())
        .role(Role::Button)
        .aria_label(words)
        // Greyed, told off to a screen reader too (gaps row U23).
        .off_if(!enabled)
        // Dragged onto another button, it takes that one's place, as
        // a column's heading does (View › Customize toolbar shows or
        // hides each).
        .on_drag(
            DraggedButton {
                command,
                words,
                theme,
            },
            |dragged, _, _, cx| cx.new(|_| dragged.clone()),
        )
        .drag_over::<DraggedButton>(move |style, _, _, _| style.bg(theme.stripe))
        .on_drop(cx.listener(move |view, dragged: &DraggedButton, _, cx| {
            view.move_toolbar_button(dragged.command, command, cx)
        }))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(space::LINE))
        .px_2()
        .py_1()
        .rounded(px(crate::widgets::native::RADIUS))
        // A toolbar item's label: the system's small size.
        .text_size(px(crate::widgets::native::SMALL_TEXT))
        .when(main, |b| b.bg(theme.accent).text_color(theme.on_accent))
        .when(!enabled, |b| b.opacity(0.4))
        .when(enabled, |b| {
            b.hover(move |s| s.bg(hovered(main, theme)))
                .on_click(cx.listener(move |_, _, window, cx| {
                    window.dispatch_action(Box::new(Run(command)), cx);
                }))
        })
        .when(picture, |b| {
            b.child(
                svg()
                    .path(icon(command))
                    .size(px(crate::widgets::native::ICON_TOOL))
                    .text_color(if main { theme.on_accent } else { theme.text }),
            )
        })
        .when(worded, |b| b.child(screen.label(words)))
        // As wide as the widest of its words, so the buttons after it stay put when
        // Resume becomes Retry or Start now: the others
        // take room under it, unseen and unread.
        .when(
            worded && command == dm_shell::app::commands::Command::DownloadNow,
            |b| {
                b.child(
                    div().h(px(0.0)).overflow_hidden().invisible().children(
                        ["Resume", "Retry", "Start now"]
                            .into_iter()
                            .map(|word| div().whitespace_nowrap().child(t(word))),
                    ),
                )
            },
        )
        // Its words on hover where the button shows none, and why it is greyed where
        // it is (gaps row U5).
        .when(!worded || why_off.is_some(), |b| {
            let said = match why_off {
                Some(why) => format!("{words}: {why}"),
                None => words.to_string(),
            };
            b.tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(gpui::SharedString::from(said.clone()))
                    .build(window, cx)
            })
        });
    div()
        .flex()
        .map(crate::window::mirror::row)
        .items_center()
        // Room before the rest, which stand at the end (gaps row E3).
        .when(rest_after == Some(at), |d| d.ml_auto())
        .child(button)
        .children(arrow)
        .into_any_element()
}

/// The arrow beside Start Queue or Stop Queue (`command`) that lists the `queues`; the one
/// chosen is started or stopped.
fn queue_arrow(command: Command, queues: Vec<String>, theme: Theme) -> impl IntoElement {
    div()
        .id((command.id(), 1usize))
        .role(Role::Button)
        .aria_label(dm_desktop_base::i18n::t("Queues"))
        .h(px(28.0))
        .w(px(14.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(crate::widgets::native::RADIUS))
        .hover(|s| s.bg(theme.stripe))
        .child(
            svg()
                .path("icons/chevron-up-down.svg")
                .size(px(10.0))
                .text_color(theme.muted),
        )
        .on_click(move |event, window, cx| {
            let menu = queues.iter().fold(
                gpui_component::native_menu::NativeMenu::new(),
                |menu, queue| {
                    menu.menu(
                        dm_desktop_base::i18n::name(queue),
                        Box::new(crate::window::menus::RunOnQueue {
                            command,
                            queue: queue.clone(),
                        }),
                    )
                },
            );
            menu.show(event.position(), window, cx);
        })
}

/// A toolbar button being dragged to a new place: its word, under the
/// pointer meanwhile.
#[derive(Clone)]
pub struct DraggedButton {
    command: Command,
    words: &'static str,
    theme: Theme,
}

impl Render for DraggedButton {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py(px(space::XXS))
            .rounded(px(space::CORNER_S))
            .border_1()
            .border_color(self.theme.separator)
            .bg(self.theme.window)
            .text_size(px(crate::widgets::native::SMALL_TEXT))
            .text_color(self.theme.text)
            .child(self.words)
    }
}

impl Downloads {
    /// A toolbar button dropped on another: it takes that one's place in the
    /// person's order (hidden buttons keep theirs), kept in the settings.
    pub(crate) fn move_toolbar_button(
        &mut self,
        moved: Command,
        onto: Command,
        cx: &mut Context<Self>,
    ) {
        if moved == onto {
            return;
        }
        let now = self.backend.settings();
        let order: Vec<Command> = dm_shell::app::commands::toolbar_in(&now.toolbar_order)
            .into_iter()
            .map(|(c, _)| c)
            .collect();
        let Some(order) = crate::window::table::moved_onto(order, moved, onto) else {
            return;
        };
        // The window's look alone: the file is written off its thread.
        self.backend.keep_look(|settings| {
            settings.toolbar_order = order.iter().map(|c| c.id().to_string()).collect();
        });
        self.read();
        cx.notify();
    }
}
