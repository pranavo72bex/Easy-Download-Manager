// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A question, asked as macOS's own programs ask one: a sheet on the dm
//! window, which holds that window until it is answered (`docs/decisions.md`,
//! 2026-11-10), begun and ended by dm so that dm works on while it slides
//! (`sheets`); GPUI's `WindowKind::Dialog` on Windows. Its words are dm's
//! (`dm_desktop_base::asks`); the buttons stand as macOS sets an alert's
//! (the first rightmost), and the default is Return's.

use gpui::{
    div, prelude::*, size, App, Bounds, Context, FocusHandle, KeyDownEvent, Role, SharedString,
    Window, WindowBounds, WindowKind, WindowOptions,
};

use crate::native;
use crate::theme::Theme;
use crate::zoom::px;

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
    /// The widest each button's words have been, in points at 100%: a steady question's
    /// buttons keep that width, so one whose words grow shorter (Keep downloading, then
    /// Stay) moves none beside it under the pointer.
    widest: Vec<f32>,
    ticked: bool,
    answer: Option<Answer>,
    focus: FocusHandle,
    /// Answered and going: a second press (Return again as the sheet slides
    /// off) does nothing.
    closing: bool,
}

impl Question {
    /// Answers, once, and closes the sheet.
    fn answer(&mut self, button: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(answer) = self.answer.take() {
            let ticked = self.ticked;
            cx.defer(move |cx| answer(button, ticked, cx));
        }
        self.close(window, cx);
    }

    /// Closes the sheet: on macOS it slides off its window first, dm working
    /// on meanwhile (`sheets`), out of the port's sight at once, as a closed
    /// window is.
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.closing, true) {
            return;
        }
        #[cfg(target_os = "macos")]
        if let Some(sheet) = crate::syswindow::ns_window(window) {
            let handle = window.window_handle();
            crate::screen::Screens::forget(handle, cx);
            let (gone, slid) = async_channel::bounded::<()>(1);
            crate::sheets::end(sheet, move || {
                let _ = gone.try_send(());
            });
            cx.spawn(async move |_, cx| {
                let _ = slid.recv().await;
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            })
            .detach();
            return;
        }
        let _ = cx;
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
        if steady {
            self.widest.resize(self.asked.buttons.len(), 0.0);
            for (i, words) in self.asked.buttons.iter().enumerate() {
                let wide = crate::sidebar::drawn_width(words, native::TEXT, window, cx);
                self.widest[i] = self.widest[i].max(wide);
            }
        }
        let widest = self.widest.clone();
        let button = |i: usize, cx: &mut Context<Self>| {
            native::button(
                ("button", i),
                self.asked.buttons[i].clone(),
                i == default,
                theme,
                &screen,
            )
            // Its padding and border beside its widest words.
            .when_some(widest.get(i), |b, wide| {
                b.min_w(px((wide + 30.0).max(native::BUTTON_MIN)))
            })
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
                                d.child(
                                    div()
                                        .font_family(crate::native::mono(cx))
                                        .child(screen.label(detail)),
                                )
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

/// The kind of window a question is. On Linux not GPUI's `Dialog`: there it
/// is made the child of whichever window has the keys and closed with it, so
/// "Already in the list", asked as Add URL closed, closed at once, unanswered,
/// and every question after it waited for its answer (B17, B39, B40).
/// `Floating` keeps it over that window without closing with it.
pub const KIND: WindowKind = if cfg!(any(target_os = "macos", windows)) {
    WindowKind::Dialog
} else {
    WindowKind::Floating
};

/// The kind of window [`ask`] makes. On macOS a `Floating` one, unshown,
/// which dm makes a sheet itself (`sheets`, gaps row 194): GPUI's `Dialog`
/// begins its sheet as it makes the window, inside GPUI, where AppKit's slide
/// held every window and the control port about 0.26 s, opening and again
/// closing. `sheets::begin` gives it a `Dialog`'s level back.
const ASKED_KIND: WindowKind = if cfg!(target_os = "macos") {
    WindowKind::Floating
} else {
    KIND
};

thread_local! {
    /// dm is quitting: the questions it closes as it goes are not answered.
    static QUITTING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// dm is quitting (`Downloads::quit`): a question it closes on the way out
/// is left unanswered, as one closed by the system before. Answered as
/// Escape would answer it, "Keep running in the tray?" saved Keep running
/// for a person who had chosen Exit.
pub fn quitting() {
    QUITTING.with(|q| q.set(true));
}

/// Asks `asked` as a sheet on the dm window; `answer` is told how it was
/// answered. The question's window, or `None` (and no answer) if it could
/// not open.
pub fn ask(
    asked: Asked,
    reword: Option<Reword>,
    live: Option<(Live, gpui::Entity<crate::list::Downloads>)>,
    answer: Answer,
    cx: &mut App,
) -> Option<gpui::AnyWindowHandle> {
    let height = 150.0
        + 20.0 * asked.lines.len() as f32
        + if asked.named.is_some() { 20.0 } else { 0.0 }
        + if asked.detail.is_some() { 20.0 } else { 0.0 }
        + if asked.toggle.is_some() { 26.0 } else { 0.0 };
    // Wide enough for its buttons side by side, at this system's measures
    // (each at least `BUTTON_MIN` and its words' room, `BUTTON_GAP` apart,
    // inside the margins).
    let n = asked.buttons.len() as f32;
    let width = 440f32.max(
        2.0 * native::MARGIN
            + n * (native::BUTTON_MIN + 40.0)
            + (n - 1.0).max(0.0) * native::BUTTON_GAP,
    );
    let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
    let title = asked.title.clone();
    crate::screen::open_window(
        cx,
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            kind: ASKED_KIND,
            // Shown on macOS by `sheets::begin`, as a sheet, once drawn.
            show: !cfg!(target_os = "macos"),
            // Named by its title (`screen::open_window`).
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(title),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            cx.new(|cx| {
                // Gone unanswered, however its window went (closed with
                // another, or by the system): answered as Escape answers
                // it, so the questions after it are asked; not while dm
                // quits, which closes it and asks nothing more.
                cx.on_release(|this: &mut Question, cx| {
                    if QUITTING.with(std::cell::Cell::get) {
                        return;
                    }
                    if let Some(answer) = this.answer.take() {
                        let (escape, ticked) = (this.asked.escape, this.ticked);
                        cx.defer(move |cx| answer(escape, ticked, cx));
                    }
                })
                .detach();
                let focus = cx.focus_handle();
                window.focus(&focus, cx);
                #[cfg(target_os = "macos")]
                if let Some(sheet) = crate::syswindow::ns_window(window) {
                    crate::sheets::begin(sheet);
                }
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
                    widest: Vec::new(),
                    ticked: false,
                    answer: Some(answer),
                    focus,
                    closing: false,
                }
            })
        },
    )
    .ok()
    .map(Into::into)
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
