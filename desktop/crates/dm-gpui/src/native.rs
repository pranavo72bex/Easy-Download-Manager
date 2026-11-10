// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The system's own measures for a dialog, in one place: AppKit's regular
//! controls (13-point text, 11 for what is said under it; a text field, a
//! push button and a pop-up all the same height), its window margins and
//! spacing, and how macOS lays out a form (labels right-aligned in a column
//! of their own, the controls in a second) and a row of buttons (the
//! default rightmost, Cancel beside it, any other action at the leading
//! side). Every dialog is built from these, so they agree with one another
//! and with the system's.

use gpui::{div, prelude::*, px, Div, Entity, FontWeight, Role, SharedString, Stateful};
use gpui_component::input::{Input, InputState};

use crate::screen::Screen;
use crate::theme::Theme;

/// The system font's size for controls and labels (`NSFont.systemFontSize`).
pub const TEXT: f32 = 13.0;
/// The small system font's size: what is said under a control, a dialog's
/// informative text (`NSFont.smallSystemFontSize`).
pub const SMALL_TEXT: f32 = 11.0;
/// A regular control's height: a text field, a push button, a pop-up (24
/// points, measured on the system's own dialog).
pub const CONTROL: f32 = 24.0;
/// From the window's edges to what it holds.
pub const MARGIN: f32 = 20.0;
/// Between one row of a form and the next.
pub const ROW_GAP: f32 = 8.0;
/// Between a label and its control.
pub const LABEL_GAP: f32 = 8.0;
/// Between two buttons in a row.
pub const BUTTON_GAP: f32 = 12.0;
/// Above the row of buttons.
pub const FOOTER_GAP: f32 = 20.0;
/// A push button's narrowest, so OK is not a sliver beside Cancel.
pub const BUTTON_MIN: f32 = 80.0;
/// A control's corners.
pub const RADIUS: f32 = 5.0;

/// A dialog's body: the margins, the system's text, its rows apart.
pub fn body(theme: Theme) -> Div {
    div()
        .p(px(MARGIN))
        .flex()
        .flex_col()
        .gap(px(ROW_GAP))
        .text_size(px(TEXT))
        .text_color(theme.text)
}

/// A push button; `default` drawn as the system draws the default button
/// (Return's), in the accent colour.
pub fn button(
    id: impl Into<gpui::ElementId>,
    words: SharedString,
    default: bool,
    theme: Theme,
    screen: &Screen,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(words.clone())
        .h(px(CONTROL))
        .min_w(px(BUTTON_MIN))
        .px(px(14.0))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(RADIUS))
        .text_size(px(TEXT))
        .whitespace_nowrap()
        .when(default, |b| b.bg(theme.accent).text_color(gpui::white()))
        .when(!default, |b| {
            b.bg(theme.control)
                .border_1()
                .border_color(theme.control_border)
        })
        .child(screen.label(words))
}

/// A one-line text field, as tall as the other controls, its text 6 points
/// in, as the system's.
pub fn field(input: &Entity<InputState>, name: &'static str, theme: Theme) -> impl IntoElement {
    field_if(input, name, true, theme)
}

/// A field that takes no typing while `on` is false, greyed as the
/// system greys one.
pub fn field_if(
    input: &Entity<InputState>,
    name: &'static str,
    on: bool,
    theme: Theme,
) -> impl IntoElement {
    let field = Input::new(input)
        .aria_label(dm_desktop_base::i18n::t(name))
        .disabled(!on);
    let held = input.clone();
    // `Styled`'s, not the multi-line height `Input::h` sets.
    div()
        .relative()
        .w_full()
        .child(
            gpui::Styled::h(field, px(CONTROL))
                .text_size(px(TEXT))
                .px(px(6.0))
                .py(px(0.0))
                .rounded(px(RADIUS))
                .bg(theme.field)
                .border_color(theme.separator),
        )
        // What the field holds is on screen as a person reads it, for the
        // control port (`ui.texts`); a password's is not.
        .child(
            gpui::canvas(
                move |bounds, window, cx| {
                    let state = held.read(cx);
                    if !state.presentation().is_masked() {
                        crate::screen::Screens::record(
                            window,
                            cx,
                            state.value().to_string(),
                            bounds,
                        );
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
}

/// A form's row: `label` right-aligned in a column `label_width` wide, its
/// control beside it.
pub fn row(
    label: Option<&str>,
    label_width: f32,
    screen: &Screen,
    control: impl IntoElement,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(LABEL_GAP))
        .child(
            div()
                .w(px(label_width))
                .flex_none()
                .flex()
                .justify_end()
                .whitespace_nowrap()
                .when_some(label.map(str::to_string), |d, label| {
                    d.child(screen.label(label))
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(LABEL_GAP))
                .child(control),
        )
}

/// The width a form's label column needs for `labels` at the system's
/// size: the widest, measured as the system font sets these words (about
/// 0.55 of the size a letter), never under 60 points.
pub fn label_width(labels: &[&str]) -> f32 {
    labels
        .iter()
        .map(|l| l.chars().count() as f32 * TEXT * 0.55)
        .fold(60.0, f32::max)
        .ceil()
}

/// A check box and its words, as the system draws one: ticked, in the
/// accent colour with a white check mark.
pub fn checkbox(
    id: impl Into<gpui::ElementId>,
    words: SharedString,
    ticked: bool,
    theme: Theme,
    screen: &Screen,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::CheckBox)
        .aria_label(words.clone())
        .aria_toggled(if ticked {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(tick_box(ticked, theme))
        .child(screen.label(words))
}

/// The box of a check box.
pub fn tick_box(ticked: bool, theme: Theme) -> Div {
    div()
        .size(px(14.0))
        .flex_none()
        .rounded(px(3.5))
        .border_1()
        .border_color(theme.muted)
        .bg(theme.field)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(10.0))
        .font_weight(FontWeight::BOLD)
        .when(ticked, |b| {
            b.bg(theme.accent)
                .border_color(theme.accent)
                .text_color(gpui::white())
                .child("✓")
        })
}

/// A row of buttons as macOS lays one out: `leading` at the start (an
/// action beside the question's answer, such as Add for later), and
/// `trailing` at the end in the order given, the default last (rightmost).
pub fn buttons(leading: Vec<gpui::AnyElement>, trailing: Vec<gpui::AnyElement>) -> Div {
    div()
        .mt(px(FOOTER_GAP - ROW_GAP))
        .flex()
        .items_center()
        .gap(px(BUTTON_GAP))
        .child(
            div()
                .flex_1()
                .flex()
                .items_center()
                .gap(px(LABEL_GAP))
                .children(leading),
        )
        .children(trailing)
}

/// Words said under a control, or of a warning: the small system size.
pub fn note(words: impl Into<SharedString>, color: gpui::Rgba, screen: &Screen) -> Div {
    div()
        .text_size(px(SMALL_TEXT))
        .text_color(color)
        .child(screen.label(words))
}

/// A radio button and its words, as the system draws one: chosen, in the
/// accent colour with a white dot.
pub fn radio(
    id: impl Into<gpui::ElementId>,
    words: SharedString,
    on: bool,
    theme: Theme,
    screen: &Screen,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::RadioButton)
        .aria_label(words.clone())
        .aria_toggled(if on {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(
            div()
                .size(px(14.0))
                .flex_none()
                .rounded_full()
                .border_1()
                .border_color(theme.muted)
                .bg(theme.field)
                .flex()
                .items_center()
                .justify_center()
                .when(on, |b| {
                    b.bg(theme.accent)
                        .border_color(theme.accent)
                        .child(div().size(px(6.0)).rounded_full().bg(gpui::white()))
                }),
        )
        .child(screen.label(words))
}

/// A control that takes no clicks while `on` is false, drawn faded, as the
/// system greys a control it has disabled.
pub fn enabled<E: IntoElement>(on: bool, element: E) -> Div {
    div()
        .when(!on, |d| d.opacity(0.45).occlude())
        .child(element)
}

/// A dialog's own way out, which its window's close button takes too, as
/// Cancel and Escape do (B42: "its close button does what Cancel does"): a
/// hand-off answered, an early download removed, a tick kept.
pub trait CloseButton: 'static + Sized {
    /// What the close button does: the dialog's Cancel, which closes it.
    fn close_button(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<Self>);
}

/// What closes a window as its close button does: true where the window
/// is to go as it is (its dialog gone already).
type Closer = std::rc::Rc<dyn Fn(&mut gpui::Window, &mut gpui::App) -> bool>;

thread_local! {
    /// Each dialog window's closer, for File › Close (⌘W) as for its button.
    static CLOSERS: std::cell::RefCell<Vec<(gpui::AnyWindowHandle, Closer)>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// The window keys are being given to by the control port, which is not
    /// the system's key window when dm is not in front.
    static KEYS_TO: std::cell::Cell<Option<gpui::AnyWindowHandle>> =
        const { std::cell::Cell::new(None) };
}

/// Gives the control port's keys to `window`: what they ask of "the window
/// in front" is asked of it.
pub fn keys_to<R>(window: gpui::AnyWindowHandle, give: impl FnOnce() -> R) -> R {
    KEYS_TO.with(|k| k.set(Some(window)));
    let given = give();
    KEYS_TO.with(|k| k.set(None));
    given
}

/// `view`'s window closes by the dialog's own Cancel, not by the window
/// simply going: from its close button, and from File › Close (⌘W).
pub fn close_is_cancel<V: CloseButton>(
    view: &Entity<V>,
    window: &mut gpui::Window,
    cx: &gpui::App,
) {
    let view = view.downgrade();
    let closer: Closer = std::rc::Rc::new(move |window, cx| {
        // The dialog closes itself, as its Cancel does; gone already, the
        // window closes.
        view.update(cx, |this, cx| this.close_button(window, cx))
            .is_err()
    });
    let open = cx.windows();
    let handle = window.window_handle();
    CLOSERS.with(|all| {
        let mut all = all.borrow_mut();
        all.retain(|(w, _)| open.contains(w) && *w != handle);
        all.push((handle, closer.clone()));
    });
    window.on_window_should_close(cx, move |window, cx| closer(window, cx));
}

/// File › Close (⌘W) on the window in front that is not the main window's
/// (which keeps its own rule): a dialog's Cancel, else the window closed.
pub fn close_front(cx: &mut gpui::App) {
    let Some(front) = KEYS_TO.with(|k| k.get()).or_else(|| cx.active_window()) else {
        return;
    };
    // Only a dialog's window: the main window keeps its own rule, and is
    // never simply closed.
    let dialog = crate::screen::Screens::all(cx)
        .iter()
        .any(|(w, _, dialog)| *w == front && *dialog);
    if !dialog {
        return;
    }
    let closer = CLOSERS.with(|all| {
        all.borrow()
            .iter()
            .find(|(w, _)| *w == front)
            .map(|(_, c)| c.clone())
    });
    // After the dispatch that asked: the window is busy with it until then.
    cx.defer(move |cx| {
        let _ = front.update(cx, |_, window, cx| match closer {
            Some(closer) => {
                if closer(window, cx) {
                    window.remove_window();
                }
            }
            None => window.remove_window(),
        });
    });
}
