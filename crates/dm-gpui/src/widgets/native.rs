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

use crate::widgets::space;
use crate::window::a11y::{Greyed, Told};
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// The measure each system's own dialogs use: macOS's (AppKit's, measured
/// on the system's own dialog), Windows 11's (WinUI's) or GNOME's
/// (libadwaita's), so a dm dialog is laid out as its neighbours are.
const fn pick(macos: f32, windows: f32, gnome: f32) -> f32 {
    Each {
        macos,
        windows,
        gnome,
    }
    .here()
}

/// `phone` on a phone, where a finger is the pointer and the text is read at arm's length
/// (Android's 48 dp touch target, 14 to 16 sp body text, 16 dp margins), else `desktop`.
const fn on_a_phone(phone: f32, desktop: f32) -> f32 {
    if crate::platform::ON_A_PHONE {
        phone
    } else {
        desktop
    }
}

/// One measure for each system, each by its system's name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Each<T> {
    pub(crate) macos: T,
    pub(crate) windows: T,
    pub(crate) gnome: T,
}

impl<T: Copy> Each<T> {
    /// The same measure on every system.
    pub(crate) const fn all(same: T) -> Self {
        Each {
            macos: same,
            windows: same,
            gnome: same,
        }
    }

    /// This system's measure (GNOME's on any system but macOS and Windows).
    pub(crate) const fn here(&self) -> T {
        if cfg!(target_os = "macos") {
            self.macos
        } else if cfg!(windows) {
            self.windows
        } else {
            self.gnome
        }
    }
}

/// The windows whose size is fixed: each asks the table below for its row
/// by name. A window whose size follows what it holds (About, a question,
/// Details, the limiter, a row's dialog) works its size out where it opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowSize {
    /// The main window, the downloads list, on its first start.
    Main,
    /// Options.
    Options,
    /// The site grabber.
    Grabber,
    /// Links found on a page.
    Links,
    /// Links on the clipboard.
    ClipboardLinks,
    /// Add URL.
    AddUrl,
    /// Add numbered series.
    Series,
    /// A playlist's videos.
    Playlist,
    /// "Download complete".
    Complete,
    /// A download's progress.
    Progress,
    /// A download's properties.
    Properties,
    /// The scheduler.
    Scheduler,
    /// The first start's welcome.
    Welcome,
    /// View › Statistics (gaps row W15).
    Statistics,
}

impl WindowSize {
    /// The table: each window's size as it opens, width by height in
    /// points, on each system. Smaller on Windows where a laptop's screen
    /// asked for it (the maintainer); Windows' sizes are to be
    /// set again.
    pub(crate) const fn each(self) -> Each<(f32, f32)> {
        match self {
            Self::Main => Each {
                macos: (900.0, 600.0),
                windows: (840.0, 520.0),
                gnome: (900.0, 600.0),
            },
            // The list of pages beside the page (gaps row C21): 200 wider than the page was.
            Self::Options => Each {
                macos: (880.0, 600.0),
                windows: (820.0, 520.0),
                gnome: (880.0, 600.0),
            },
            Self::Grabber => Each {
                macos: (640.0, 560.0),
                windows: (600.0, 480.0),
                gnome: (640.0, 560.0),
            },
            Self::Links => Each {
                macos: (760.0, 560.0),
                windows: (680.0, 480.0),
                gnome: (760.0, 560.0),
            },
            Self::ClipboardLinks => Each::all((520.0, 300.0)),
            Self::AddUrl => Each::all((520.0, 220.0)),
            Self::Series => Each::all((520.0, 300.0)),
            Self::Playlist => Each::all((560.0, 420.0)),
            Self::Complete => Each::all((440.0, 220.0)),
            Self::Progress => Each::all((480.0, 320.0)),
            Self::Properties => Each::all((560.0, 420.0)),
            Self::Scheduler => Each::all((720.0, 520.0)),
            Self::Welcome => Each::all((480.0, 300.0)),
            Self::Statistics => Each::all((560.0, 560.0)),
        }
    }

    /// This window's size on this system: width, height.
    pub(crate) const fn here(self) -> (f32, f32) {
        self.each().here()
    }
}

/// The system font's size for controls and labels (macOS's
/// `NSFont.systemFontSize`, WinUI's body text, GNOME's).
pub const TEXT: f32 = on_a_phone(15.0, pick(13.0, 14.0, 14.0));
/// The small system font's size: what is said under a control, a dialog's
/// informative text (macOS's `smallSystemFontSize`, WinUI's caption).
pub const SMALL_TEXT: f32 = on_a_phone(13.0, pick(11.0, 12.0, 12.0));
/// A figure that leads its window (the progress window's), and a phone's dialog's and sheet's
/// title: 1.4 times the body's (rule 4 asks 1.3 at least), the one size above the ranks (gaps
/// row E34); made by sums at each call, they were three.
pub const FIGURE_TEXT: f32 = TEXT * 1.4;
/// A check box's tick.
pub const TICK_TEXT: f32 = on_a_phone(15.0, MARK_TEXT);
/// The type ramp beyond the body and small sizes (gaps row V13), named once so a size changed
/// here moves every place that uses it: a letter or mark inside a small round badge,
pub const MARK_TEXT: f32 = 10.0;
/// a label under a phone's navigation icon (Material's label size),
pub const CAPTION_TEXT: f32 = 12.0;
/// a heading of its own over a few lines (an empty list's),
pub const SUBTITLE_TEXT: f32 = 16.0;
/// and a window's one title (About's name).
pub const TITLE_TEXT: f32 = 20.0;
/// A number's box: one width for every count, size and limit in the dialogs and Options (gaps
/// row E31); they were 48 to 110 points, one at each call.
pub const NUMBER_FIELD: f32 = 90.0;
/// A time's box (minutes and seconds, "4:45"): narrower, so a part's From and to stay on one
/// line in Windows' wider type, where at a number's width they wrapped.
pub const TIME_FIELD: f32 = 64.0;
/// A regular control's height: a text field, a push button, a pop-up.
pub const CONTROL: f32 = on_a_phone(space::TOUCH, pick(24.0, 32.0, 34.0));
// A phone's control a finger's least (gaps row E26), held where the phone's build is made.
const _: () = assert!(!crate::platform::ON_A_PHONE || CONTROL >= space::TOUCH);
/// From the window's edges to what it holds.
pub const MARGIN: f32 = on_a_phone(16.0, pick(20.0, 24.0, 18.0));
/// Between one row of a form and the next.
pub const ROW_GAP: f32 = on_a_phone(14.0, pick(8.0, 8.0, 10.0));
/// Between a label and its control.
pub const LABEL_GAP: f32 = 8.0;
/// Between two buttons in a row.
pub const BUTTON_GAP: f32 = pick(12.0, 8.0, 12.0);
/// Above the row of buttons.
pub const FOOTER_GAP: f32 = pick(20.0, 24.0, 18.0);
/// A push button's narrowest, so OK is not a sliver beside Cancel.
pub const BUTTON_MIN: f32 = on_a_phone(64.0, pick(80.0, 120.0, 80.0));
/// A control's corners.
pub const RADIUS: f32 = on_a_phone(10.0, pick(5.0, 4.0, 6.0));

/// A picture's size, in points: a mark beside small words (a heading's arrow, a status mark).
pub const ICON_SMALL: f32 = 12.0;
/// A picture beside body text: a row's, a place's (gaps row E23: each size was a bare number).
pub const ICON: f32 = 16.0;
/// A toolbar button's picture over its word.
pub const ICON_TOOL: f32 = 18.0;
/// A phone's picture: a card's button, a tab of the bottom bar.
pub const ICON_LARGE: f32 = 20.0;

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
        // A phone's: never wider than the screen, a long name cut with "…".
        .when(crate::platform::ON_A_PHONE, |b| {
            b.max_w_full().overflow_hidden().text_ellipsis()
        })
        .when(default, |b| b.bg(theme.accent).text_color(theme.on_accent))
        .when(!default, |b| {
            b.bg(theme.control)
                .border_1()
                .border_color(theme.control_border)
        })
        // Answering the pointer: lighter or darker under it, more so pressed (gaps row U2).
        .hover(move |s| {
            let face = if default { theme.accent } else { theme.control };
            s.bg(theme.answering(face, crate::window::theme::HOVERED))
        })
        .active(move |s| {
            let face = if default { theme.accent } else { theme.control };
            s.bg(theme.answering(face, crate::window::theme::PRESSED))
        })
        .child(screen.label(words))
}

/// A one-line text field, as tall as the other controls, its text 6 points
/// in, as the system's.
pub fn field(input: &Entity<InputState>, name: &'static str, theme: Theme) -> Div {
    field_if(input, name, true, theme)
}

/// A field that takes no typing while `on` is false, greyed as the
/// system greys one.
pub fn field_if(input: &Entity<InputState>, name: &'static str, on: bool, theme: Theme) -> Div {
    field_drawn(input, name, on, true, theme)
}

/// `field_if`, with the field itself not drawn where `drawn` is false: it still takes the
/// keyboard and the pointer, and something laid over it shows in its place. Linux's renderer
/// painted the field's text over a box laid on it, so both were read at once (`path_field`).
pub(super) fn field_drawn(
    input: &Entity<InputState>,
    name: &'static str,
    on: bool,
    drawn: bool,
    theme: Theme,
) -> Div {
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
        // a click near its left edge would type nowhere (an Options number,
        // on Linux with real input).
        .when(on, |d| {
            d.on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                let focus = gpui::Focusable::focus_handle(pressed.read(cx), cx);
                window.focus(&focus, cx);
            })
        })
        .child(
            gpui::Styled::h(field, px(CONTROL))
                .text_size(px(TEXT))
                .px(px(space::S))
                .py(px(0.0))
                .rounded(px(RADIUS))
                .bg(theme.field)
                .border_color(theme.separator)
                .when(!drawn, |f| f.opacity(0.0)),
        )
        // What the field holds is on screen as a person reads it, for the
        // control port (`ui.texts`); a password's is not.
        .child(
            gpui::canvas(
                move |bounds, window, cx| {
                    let state = held.read(cx);
                    if !state.presentation().is_masked() {
                        crate::window::screen::Screens::record(
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
        .map(|c| ringed(c, theme))
        .aria_toggled(if ticked {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .flex()
        .items_center()
        // A finger's least on a phone (gaps row E26).
        .min_h(px(on_a_phone(space::TOUCH, 0.0)))
        .gap(px(space::S))
        .child(tick_box(ticked, theme))
        // On a phone long words wrap under themselves rather than running off the screen.
        .map(|row| {
            if crate::platform::ON_A_PHONE {
                row.w_full()
                    .child(div().flex_1().min_w_0().child(screen.label(words)))
            } else {
                row.child(screen.label(words))
            }
        })
}

/// The box of a check box.
pub fn tick_box(ticked: bool, theme: Theme) -> Div {
    div()
        .size(px(on_a_phone(22.0, 14.0)))
        .flex_none()
        .rounded(px(space::CORNER_S))
        .border_1()
        .border_color(theme.muted)
        .bg(theme.field)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(TICK_TEXT))
        .font_weight(FontWeight::SEMIBOLD)
        .when(ticked, |b| {
            b.bg(theme.accent)
                .border_color(theme.accent)
                .text_color(theme.on_accent)
                .child(icon("icons/check.svg", TICK_TEXT, theme.on_accent))
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
    // A phone's: the buttons wrap onto another line at the end, as a phone's dialogs stack
    // theirs, where the screen has no room for them side by side; one row ran off its edge.
    if crate::platform::ON_A_PHONE {
        return div()
            .mt(px(FOOTER_GAP - ROW_GAP))
            .flex()
            .map(crate::window::mirror::row)
            .flex_wrap()
            .items_center()
            .justify_end()
            .gap(px(BUTTON_GAP))
            .children(leading)
            .children(trailing);
    }
    div()
        .mt(px(FOOTER_GAP - ROW_GAP))
        .flex()
        .map(crate::window::mirror::row)
        .items_center()
        .gap(px(BUTTON_GAP))
        .child(
            div()
                .flex_1()
                .flex()
                .map(crate::window::mirror::row)
                .items_center()
                .gap(px(LABEL_GAP))
                .children(leading),
        )
        .children(trailing)
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
        // Reached by Tab, chosen by Space, its place ringed (gaps row U2); the arrows move
        // to the radio before or after it, as in the system's groups of them.
        .map(|c| ringed(c, theme))
        .on_key_down(|event: &gpui::KeyDownEvent, window, cx| {
            if event.keystroke.modifiers.modified() {
                return;
            }
            match event.keystroke.key.as_str() {
                "up" | "left" => window.focus_prev(cx),
                "down" | "right" => window.focus_next(cx),
                _ => return,
            }
            cx.stop_propagation();
        })
        .aria_toggled(if on {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .flex()
        .items_center()
        .min_h(px(on_a_phone(space::TOUCH, 0.0)))
        .gap(px(space::S))
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
    let label = label.into();
    div()
        .id(id)
        .role(Role::Dialog)
        .aria_label(label.clone())
        .map(crate::window::mirror::text)
        .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" {
                // A pop-up's sheet open closes first; the dialog with the next Escape.
                if crate::widgets::popup::close_picking_in(window, cx) {
                    return;
                }
                this.close_button(window, cx);
            }
        }))
        .size_full()
        .bg(theme.window)
        // A phone's dialog fills the screen with no title bar of the system's: its own top
        // bar names it, and its Back does what the close button does, as the phone's Back.
        // A column there, so what is under the bar takes the rest of the screen and scrolls.
        .when(crate::platform::ON_A_PHONE, |root| {
            root.flex().flex_col().child(phone_bar(
                label,
                theme,
                cx.listener(|this, _: &gpui::ClickEvent, window, cx| this.close_button(window, cx)),
            ))
        })
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

/// The window keys are going to: the control port's ([`keys_to`]), else the system's key
/// window.
pub fn keys_window(cx: &gpui::App) -> Option<gpui::AnyWindowHandle> {
    KEYS_TO.with(|k| k.get()).or_else(|| cx.active_window())
}

/// File › Close (⌘W) on the window in front that is not the main window's
/// (which keeps its own rule): a dialog's Cancel, else the window closed.
pub fn close_front(cx: &mut gpui::App) {
    let Some(front) = keys_window(cx) else {
        return;
    };
    // Only a dialog's window: the main window keeps its own rule, and is
    // never simply closed.
    let dialog = crate::window::screen::Screens::all(cx)
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

/// A time as two pop-ups, hour and minute, in the system's clock (gaps row U32), named by
/// `fields` (the hour's, the minute's) for the view's `popup::Pick` to set: typed, a wrong one
/// was refused only at OK.
pub fn time_picker(
    fields: (&'static str, &'static str),
    time: &str,
    theme: Theme,
    screen: &Screen,
) -> Div {
    time_picker_of(fields, 0, time, theme, screen)
}

/// [`time_picker`] for row `item` of a list of them (Options' queue start times).
pub fn time_picker_of(
    fields: (&'static str, &'static str),
    item: usize,
    time: &str,
    theme: Theme,
    screen: &Screen,
) -> Div {
    use dm_desktop_base::scheduler::{hour_choices, minute_choices, time_parts};
    let hour12 = dm_desktop_base::region::system().is_some_and(|r| r.hour12);
    let (h, m) = time_parts(time);
    let hours: Vec<SharedString> = hour_choices(hour12).into_iter().map(Into::into).collect();
    let minutes: Vec<SharedString> = minute_choices().into_iter().map(Into::into).collect();
    div()
        .flex()
        .items_center()
        .gap(px(space::XS))
        .child(crate::widgets::popup::button_of(
            fields.0,
            item,
            hours[h as usize].clone(),
            hours,
            Some(h as usize),
            theme,
            screen,
        ))
        .child(screen.label(":"))
        .child(crate::widgets::popup::button_of(
            fields.1,
            item,
            minutes[m as usize].clone(),
            minutes,
            Some(m as usize),
            theme,
            screen,
        ))
}

/// The time `time` ("HH:MM") with its hour (`hour`) or minute set to `to`: what a time pop-up's
/// choice does.
pub fn time_picked(time: &mut String, hour: bool, to: usize) {
    use dm_desktop_base::scheduler::{time_of, time_parts};
    let (h, m) = time_parts(time);
    let to = to as u32;
    *time = if hour { time_of(to, m) } else { time_of(h, to) };
}

mod destroy;
mod help;
mod keys;
mod measure;
pub use help::help_button;
mod bar;
pub use bar::bar;
mod note;
mod row;
pub use row::row;
mod path;
mod switch;
pub use destroy::{destructive, undo_line};
pub use keys::{faced, icon, return_is_default, ringed};
pub use measure::{figures, label_width, measure_with, small_width, text_width};
pub use note::{cut, heading, note, says};
pub use switch::{switch, switch_inline, SWITCH_INLINE_BEFORE};
mod phone;
pub use path::path_field;
pub use phone::{phone_bar, phone_framed, row_short, row_wide};

#[cfg(test)]
mod tests;
