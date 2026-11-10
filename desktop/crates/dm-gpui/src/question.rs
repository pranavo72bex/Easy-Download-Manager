// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A question, asked as macOS's own programs ask one: a sheet on the dm
//! window (GPUI's `WindowKind::Dialog`), which holds that window until it
//! is answered (`docs/decisions.md`, 2026-11-10). Its words are dm's
//! (`dm_desktop_base::asks`); the buttons stand as macOS sets an alert's
//! (the first rightmost), and the default is Return's.

use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, FocusHandle, KeyDownEvent, Role, SharedString,
    Window, WindowBounds, WindowKind, WindowOptions,
};

use crate::native;
use crate::theme::Theme;

/// What a question says and offers.
pub struct Asked {
    /// Its title, as the control port and a screen reader find it.
    pub title: SharedString,
    /// Its sentences.
    pub lines: Vec<SharedString>,
    /// Which one, by name, or how many: said in bold.
    pub named: Option<SharedString>,
    /// What a server answered, as it came: in the system's fixed-width
    /// font.
    pub detail: Option<SharedString>,
    /// A choice to tick, where there is one.
    pub toggle: Option<SharedString>,
    /// Its buttons, in order: the first stands rightmost, the place of
    /// the default (`placed`).
    pub buttons: Vec<SharedString>,
    /// Which button is the default, highlighted and Return's: the first,
    /// which loses nothing, but for a question whose answer changes under
    /// it (Exit's), where the highlight moves and the buttons do not.
    pub default: usize,
    /// Which button Escape and the close button press.
    pub escape: usize,
}

/// How it was answered: the button pressed (its place in
/// [`Asked::buttons`]), and whether the choice was ticked.
pub type Answer = Box<dyn FnOnce(usize, bool, &mut App)>;

/// What a question says once its choice is ticked or not: some questions
/// say something else then (Remove's: "Their files are deleted too.").
pub type Reword = Box<dyn Fn(bool) -> Asked>;

/// What a question says as the list changes under it, asked again each time
/// it does (Exit's: a download finishing while it is open); `None` once it
/// is moot (When done's countdown, run out or cancelled elsewhere), and the
/// sheet closes as Escape would answer it.
pub type Live = Box<dyn Fn(&crate::list::Downloads) -> Option<Asked>>;

/// The question's sheet.
pub struct Question {
    screen: crate::screen::Screen,
    asked: Asked,
    reword: Option<Reword>,
    /// Its words follow the list: room is kept for them.
    steady: bool,
    ticked: bool,
    answer: Option<Answer>,
    focus: FocusHandle,
}

impl Question {
    /// Answers, once, and closes the sheet.
    fn answer(&mut self, button: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(answer) = self.answer.take() {
            let ticked = self.ticked;
            cx.defer(move |cx| answer(button, ticked, cx));
        }
        window.remove_window();
    }
}

/// Where each button stands, as macOS lays out an alert's: the default
/// (the first) rightmost, the one Escape presses beside it, the rest at the
/// leading side; with two, both at the end, the other left of the default.
/// Each is its place in [`Asked::buttons`].
fn placed(count: usize, escape: usize) -> (Vec<usize>, Vec<usize>) {
    if count <= 2 {
        return (Vec::new(), (0..count).rev().collect());
    }
    let leading = (1..count).filter(|&i| i != escape).collect();
    let trailing = if escape != 0 && escape < count {
        vec![escape, 0]
    } else {
        vec![0]
    };
    (leading, trailing)
}

impl Render for Question {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        let ticked = self.ticked;
        let steady = self.steady;
        self.screen.begin();
        let screen = self.screen.clone();
        let default = self.asked.default;
        let button = |i: usize, cx: &mut Context<Self>| {
            native::button(
                ("button", i),
                self.asked.buttons[i].clone(),
                i == default,
                theme,
                &screen,
            )
            .on_click(cx.listener(move |this, _, window, cx| this.answer(i, window, cx)))
            .into_any_element()
        };
        let (leading, trailing) = placed(self.asked.buttons.len(), self.asked.escape);
        let leading: Vec<_> = leading.into_iter().map(|i| button(i, cx)).collect();
        let trailing: Vec<_> = trailing.into_iter().map(|i| button(i, cx)).collect();
        div()
            .id("question")
            .role(Role::AlertDialog)
            .aria_label(self.asked.title.clone())
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                // Return presses the default, which loses nothing; Escape
                // the question's own (Remove's: the same, so a slip of
                // either keeps everything).
                match event.keystroke.key.as_str() {
                    "enter" => {
                        let default = this.asked.default;
                        this.answer(default, window, cx)
                    }
                    "escape" => {
                        let escape = this.asked.escape;
                        this.answer(escape, window, cx)
                    }
                    _ => {}
                }
            }))
            .size_full()
            .bg(theme.window)
            .child(crate::fit::fitted(
                // As an alert says it: its title in bold at the system's
                // size, what it explains in the small size under it.
                native::body(theme)
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::BOLD)
                            .child(screen.label(self.asked.title.clone())),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .text_size(px(native::SMALL_TEXT))
                            // A question whose words change as it stands
                            // keeps room for three lines, so its buttons do
                            // not move under a pointer on its way to one.
                            .when(steady, |d| d.min_h(px(3.0 * native::SMALL_TEXT * 1.4)))
                            .children(self.asked.lines.iter().cloned().map(|l| screen.label(l)))
                            .when_some(self.asked.named.clone(), |d, named| {
                                d.child(
                                    div()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .child(screen.label(named)),
                                )
                            })
                            .when_some(self.asked.detail.clone(), |d, detail| {
                                d.child(div().font_family("Menlo").child(screen.label(detail)))
                            }),
                    )
                    .when_some(self.asked.toggle.clone(), |d, toggle| {
                        d.child(
                            native::checkbox("toggle", toggle, ticked, theme, &screen).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.ticked = !this.ticked;
                                    if let Some(reword) = &this.reword {
                                        this.asked = reword(this.ticked);
                                    }
                                    cx.notify();
                                }),
                            ),
                        )
                    })
                    .child(native::buttons(leading, trailing)),
            ))
    }
}

/// Asks `asked` as a sheet on the dm window; `answer` is told how it was
/// answered. No answer if the sheet could not open.
pub fn ask(
    asked: Asked,
    reword: Option<Reword>,
    live: Option<(Live, gpui::Entity<crate::list::Downloads>)>,
    answer: Answer,
    cx: &mut App,
) -> bool {
    let height = 150.0
        + 20.0 * asked.lines.len() as f32
        + if asked.named.is_some() { 20.0 } else { 0.0 }
        + if asked.detail.is_some() { 20.0 } else { 0.0 }
        + if asked.toggle.is_some() { 26.0 } else { 0.0 };
    // Wide enough for its buttons side by side.
    let width = 440f32.max(40.0 + 120.0 * asked.buttons.len() as f32);
    let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
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
                // Said again each time the list changes, where it follows
                // the list: after the list's update, never inside it (the
                // first frame is drawn while the list is still updating).
                let steady = live.is_some();
                if let Some((live, view)) = live {
                    cx.observe_in(
                        &view,
                        window,
                        move |this: &mut Question, view, window, cx| match live(view.read(cx)) {
                            Some(asked) => {
                                this.asked = asked;
                                cx.notify();
                            }
                            None => {
                                let escape = this.asked.escape;
                                this.answer(escape, window, cx);
                            }
                        },
                    )
                    .detach();
                }
                Question {
                    screen,
                    asked,
                    reword,
                    steady,
                    ticked: false,
                    answer: Some(answer),
                    focus,
                }
            })
        },
    )
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::placed;

    #[test]
    fn the_default_stands_rightmost_and_cancel_beside_it_as_on_a_mac() {
        // Yes / No: No, then Yes at the end.
        assert_eq!(placed(2, 0), (vec![], vec![1, 0]));
        assert_eq!(placed(1, 0), (vec![], vec![0]));
        // Show it, Download again, Replace the old file, Cancel (Escape's):
        // the other two at the leading side, Cancel beside Show it.
        assert_eq!(placed(4, 3), (vec![1, 2], vec![3, 0]));
        // Escape the default itself: the rest leading.
        assert_eq!(placed(3, 0), (vec![1, 2], vec![0]));
    }
}
