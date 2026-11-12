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

use gpui::{div, prelude::*, Div, Entity, FontWeight, Role, SharedString, Stateful};
use gpui_component::input::{Input, InputState};

use crate::a11y::{Greyed, Told};
use crate::screen::Screen;
use crate::theme::Theme;
use crate::zoom::px;

/// The measure each system's own dialogs use: macOS's (AppKit's, measured
/// on the system's own dialog), Windows 11's (WinUI's) or GNOME's
/// (libadwaita's), so a dm dialog is laid out as its neighbours are.
pub(crate) const fn pick(macos: f32, windows: f32, gnome: f32) -> f32 {
    if cfg!(target_os = "macos") {
        macos
    } else if cfg!(windows) {
        windows
    } else {
        gnome
    }
}

/// The system font's size for controls and labels (macOS's
/// `NSFont.systemFontSize`, WinUI's body text, GNOME's).
pub const TEXT: f32 = pick(13.0, 14.0, 14.0);
/// The small system font's size: what is said under a control, a dialog's
/// informative text (macOS's `smallSystemFontSize`, WinUI's caption).
pub const SMALL_TEXT: f32 = pick(11.0, 12.0, 12.0);
/// A regular control's height: a text field, a push button, a pop-up.
pub const CONTROL: f32 = pick(24.0, 32.0, 34.0);
/// From the window's edges to what it holds.
pub const MARGIN: f32 = pick(20.0, 24.0, 18.0);
/// Between one row of a form and the next.
pub const ROW_GAP: f32 = pick(8.0, 8.0, 10.0);
/// Between a label and its control.
pub const LABEL_GAP: f32 = 8.0;
/// Between two buttons in a row.
pub const BUTTON_GAP: f32 = pick(12.0, 8.0, 12.0);
/// Above the row of buttons.
pub const FOOTER_GAP: f32 = pick(20.0, 24.0, 18.0);
/// A push button's narrowest, so OK is not a sliver beside Cancel.
pub const BUTTON_MIN: f32 = pick(80.0, 120.0, 80.0);
/// A control's corners.
pub const RADIUS: f32 = pick(5.0, 4.0, 6.0);

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
        // Told off to a screen reader where it is greyed (gaps row U23).
        .off_where_greyed()
        // Reached by Tab, pressed by Space or Return, its place ringed (gaps row U2).
        .tab_index(0)
        .focus(move |s| s.border_2().border_color(theme.ring(default)))
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
        .when(default, |b| b.bg(theme.accent).text_color(theme.on_accent))
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
    let pressed = input.clone();
    // `Styled`'s, not the multi-line height `Input::h` sets.
    div()
        .relative()
        .w_full()
        // A press anywhere in the box takes the keyboard, as the system's
        // fields do: the field itself answers only inside its padding, and
        // a click near its left edge typed nowhere (an Options number, on
        // Linux with real input, 2026-11-11).
        .when(on, |d| {
            d.on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                let focus = gpui::Focusable::focus_handle(pressed.read(cx), cx);
                window.focus(&focus, cx);
            })
        })
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
        .off_where_greyed()
        // Reached by Tab, ticked by Space, its place ringed (gaps row U2).
        .tab_index(0)
        .focus(move |s| s.bg(theme.stripe))
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
                .text_color(theme.on_accent)
                .child("✓")
        })
}

/// A row of buttons as the system lays one out: `leading` at the start (an
/// action beside the question's answer, such as Add for later), and
/// `trailing` at the end, given in macOS's order, the default last
/// (rightmost), which Windows reverses.
pub fn buttons(leading: Vec<gpui::AnyElement>, mut trailing: Vec<gpui::AnyElement>) -> Div {
    // Given as macOS and GNOME order them, the default rightmost; Windows
    // puts the default first and Cancel last.
    if cfg!(windows) {
        trailing.reverse();
    }
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
        .off_where_greyed()
        // Reached by Tab, chosen by Space, its place ringed (gaps row U2).
        .tab_index(0)
        .focus(move |s| s.bg(theme.stripe))
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
                        .child(div().size(px(6.0)).rounded_full().bg(theme.on_accent))
                }),
        )
        .child(screen.label(words))
}

/// A control that takes no clicks while `on` is false, drawn faded, as the
/// system greys a control it has disabled: a layer over it, made after it
/// (GPUI's hit test lets an element block only what was made before it, so
/// blocking on the wrapper itself left its own controls clickable: a greyed
/// check box still turned). The wheel still scrolls the page it is on:
/// occluded whole, a greyed control stopped Options' Advanced page short of
/// its end for a pointer resting on it. Each of dm's controls in it is told
/// off to a screen reader too (`a11y::Greyed`, gaps row U23).
pub fn enabled<E: IntoElement>(on: bool, element: E) -> Div {
    div()
        .relative()
        .child(Greyed::new(!on, element))
        .when(!on, |d| {
            d.opacity(0.45).child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .block_mouse_except_scroll(),
            )
        })
}

/// A dialog's own way out, which its window's close button takes too, as
/// Cancel and Escape do (B42: "its close button does what Cancel does"): a
/// hand-off answered, an early download removed, a tick kept.
pub trait CloseButton: 'static + Sized {
    /// What the close button does: the dialog's Cancel, which closes it.
    fn close_button(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<Self>);
}

/// A dialog window's root, as every dm dialog's is: a dialog to a screen
/// reader, under its name, in the window's colour, filling its window; and
/// Escape doing what its close button does (its Cancel, B42). A dialog with
/// a default button adds Return's own; one with nothing to type in gives
/// its root a focus of its own (`track_focus`), focused as it opens, as
/// GPUI hands a key to the window's root view alone while nothing in it is
/// focused, and Escape missed a Download complete window on Linux.
pub fn dialog<V: CloseButton>(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    theme: Theme,
    cx: &gpui::Context<V>,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Dialog)
        .aria_label(label.into())
        .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" {
                this.close_button(window, cx);
            }
        }))
        .size_full()
        .bg(theme.window)
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

/// The system's own monospaced font, for a digest, an address or a list of
/// links: gpui-component's choice for each system (Menlo on macOS, Consolas
/// on Windows, DejaVu Sans Mono on Linux, the next installed one where that
/// is not). dm brings no font of its own: every word on screen is in one the
/// system has, the rest in its interface font (`.SystemUIFont`).
pub fn mono(cx: &gpui::App) -> gpui::SharedString {
    gpui_component::ActiveTheme::theme(cx)
        .mono_font_family
        .clone()
}
