// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A question, asked as macOS's own programs ask one: a sheet on the dm
//! window (GPUI's `WindowKind::Dialog`), which holds that window until it
//! is answered (`docs/decisions.md`, 2026-11-10). Its words are dm's
//! (`dm_desktop_base::asks`); the button that loses nothing is the default,
//! first, and Return's and Escape's.

use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, FocusHandle, KeyDownEvent, Role, SharedString,
    Window, WindowBounds, WindowKind, WindowOptions,
};

use crate::theme::Theme;

/// What a question says and offers.
pub struct Asked {
    /// Its title, as the control port and a screen reader find it.
    pub title: SharedString,
    /// Its sentences.
    pub lines: Vec<SharedString>,
    /// Which one, by name, or how many: said in bold.
    pub named: Option<SharedString>,
    /// A choice to tick, where there is one.
    pub toggle: Option<SharedString>,
    /// The button that does it.
    pub go: SharedString,
    /// The button that loses nothing: the default.
    pub keep: SharedString,
}

/// How it was answered: whether to go ahead, and whether the choice was
/// ticked.
pub type Answer = Box<dyn FnOnce(bool, bool, &mut App)>;

/// What a question says once its choice is ticked or not: some questions
/// say something else then (Remove's: "Their files are deleted too.").
pub type Reword = Box<dyn Fn(bool) -> Asked>;

/// The question's sheet.
pub struct Question {
    screen: crate::screen::Screen,
    asked: Asked,
    reword: Option<Reword>,
    ticked: bool,
    answer: Option<Answer>,
    focus: FocusHandle,
}

impl Question {
    /// Answers, once, and closes the sheet.
    fn answer(&mut self, go: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(answer) = self.answer.take() {
            let ticked = self.ticked;
            cx.defer(move |cx| answer(go, ticked, cx));
        }
        window.remove_window();
    }
}

/// A button on the sheet; `default` is drawn as macOS draws the default
/// button, in the selection colour.
fn button(
    id: &'static str,
    words: SharedString,
    default: bool,
    theme: Theme,
    screen: &crate::screen::Screen,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(words.clone())
        .px_4()
        .h(px(24.0))
        .min_w(px(80.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .border_1()
        .border_color(theme.separator)
        .when(default, |b| {
            b.bg(theme.selection).text_color(theme.on_selection)
        })
        .when(!default, |b| b.bg(theme.stripe))
        .child(screen.label(words))
}

impl Render for Question {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        let ticked = self.ticked;
        self.screen.begin();
        let screen = self.screen.clone();
        div()
            .id("question")
            .role(Role::AlertDialog)
            .aria_label(self.asked.title.clone())
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                // Return and Escape both answer with the button that loses
                // nothing: a slip of either keeps everything.
                if matches!(event.keystroke.key.as_str(), "enter" | "escape") {
                    this.answer(false, window, cx);
                }
            }))
            .size_full()
            .p_5()
            .flex()
            .flex_col()
            .gap_3()
            .bg(theme.background)
            .text_color(theme.text)
            .text_sm()
            .child(
                div()
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(screen.label(self.asked.title.clone())),
            )
            .children(self.asked.lines.iter().cloned().map(|l| screen.label(l)))
            .when_some(self.asked.named.clone(), |d, named| {
                d.child(
                    div()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(screen.label(named)),
                )
            })
            .when_some(self.asked.toggle.clone(), |d, toggle| {
                d.child(
                    div()
                        .id("toggle")
                        .role(Role::CheckBox)
                        .aria_label(toggle.clone())
                        .aria_toggled(if ticked {
                            gpui::Toggled::True
                        } else {
                            gpui::Toggled::False
                        })
                        .flex()
                        .items_center()
                        .gap_2()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.ticked = !this.ticked;
                            if let Some(reword) = &this.reword {
                                this.asked = reword(this.ticked);
                            }
                            cx.notify();
                        }))
                        .child(
                            div()
                                .size(px(14.0))
                                .rounded(px(3.0))
                                .border_1()
                                .border_color(theme.muted)
                                .when(ticked, |b| b.bg(theme.selection)),
                        )
                        .child(screen.label(toggle)),
                )
            })
            .child(
                div()
                    .mt_2()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        button("keep", self.asked.keep.clone(), true, theme, &screen).on_click(
                            cx.listener(|this, _, window, cx| this.answer(false, window, cx)),
                        ),
                    )
                    .child(
                        button("go", self.asked.go.clone(), false, theme, &screen).on_click(
                            cx.listener(|this, _, window, cx| this.answer(true, window, cx)),
                        ),
                    ),
            )
    }
}

/// Asks `asked` as a sheet on the dm window; `answer` is told how it was
/// answered. No answer if the sheet could not open.
pub fn ask(asked: Asked, reword: Option<Reword>, answer: Answer, cx: &mut App) -> bool {
    let height = 150.0
        + 20.0 * asked.lines.len() as f32
        + if asked.named.is_some() { 20.0 } else { 0.0 }
        + if asked.toggle.is_some() { 26.0 } else { 0.0 };
    let bounds = Bounds::centered(None, size(px(440.0), px(height)), cx);
    let title = asked.title.clone();
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            kind: WindowKind::Dialog,
            ..Default::default()
        },
        move |window, cx| {
            window.set_window_title(&title);
            cx.new(|cx| {
                let focus = cx.focus_handle();
                window.focus(&focus, cx);
                let screen = crate::screen::Screen::default();
                crate::screen::Screens::register(window.window_handle(), screen.clone(), true, cx);
                Question {
                    screen,
                    asked,
                    reword,
                    ticked: false,
                    answer: Some(answer),
                    focus,
                }
            })
        },
    )
    .is_ok()
}
