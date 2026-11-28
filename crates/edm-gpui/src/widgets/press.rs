// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A finger's press shown the moment it lands (gaps row V25). GPUI makes a tap a mouse press
//! and its release together, when the finger lifts, so `.active()` never drew on a phone: a
//! tapped button, card or tab gave no sign it was touched. [`layer`], the first child of
//! something tapped, paints its pressed face from the touch's start (GPUI's `TouchDragEvent`,
//! offered as each touch begins) until it ends as a tap, a scroll or a long press.
//!
//! Only the innermost thing pressed shows it (a card's round button, not the card behind it),
//! and nothing hidden under a sheet does.

use std::cell::Cell;
use std::time::Duration;

use gpui::{
    canvas, Bounds, HitboxBehavior, IntoElement, LongPressEvent, MouseUpEvent, ParentElement,
    Pixels, Point, Rgba, ScrollEvent, Styled, TouchDragEvent, TouchPhase, Window, WindowId,
};

use crate::window::theme::Theme;

/// Whether the layer is put in at all: on a phone, and in the tests that tap as one does.
pub(crate) const SHOWN: bool = crate::platform::ON_A_PHONE || cfg!(test);

/// A press that never ended as a tap, a scroll or a long press (Android took the touch for
/// its own gesture): its face goes after this.
const AT_MOST: Duration = Duration::from_millis(1500);

/// The press under way: in which window, what it pressed, and where the finger went down.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Press {
    window: WindowId,
    bounds: Bounds<Pixels>,
    at: Point<Pixels>,
    /// Which press it is, so an old one's time limit leaves a newer one alone.
    n: u64,
    /// The name it was claimed under, where its face is drawn elsewhere ([`claims`], [`face`]).
    named: Option<Name>,
}

/// What a press is claimed under where its face is drawn by another element: a tab's pill.
pub(crate) type Name = (&'static str, usize);

thread_local! {
    static PRESS: Cell<Option<Press>> = const { Cell::new(None) };
    static PRESSES: Cell<u64> = const { Cell::new(0) };
}

/// Where a finger is pressing in `window`, if one is: the bounds of what it pressed.
pub(crate) fn pressed_in(window: &Window) -> Option<Bounds<Pixels>> {
    PRESS
        .get()
        .filter(|press| press.window == window.window_handle().window_id())
        .map(|press| press.bounds)
}

/// The press in `window` over: its face drawn away.
fn lifted(window: &mut Window) {
    if pressed_in(window).is_some() {
        PRESS.set(None);
        window.refresh();
    }
}

/// [`layer`] put in `parent`, where [`SHOWN`], before its other children: drawn under its words,
/// and before anything pressable inside it, which then takes a press on itself first.
pub(crate) fn first<P: ParentElement>(parent: P, radius: f32, theme: Theme) -> P {
    if SHOWN {
        parent.child(layer(Some((radius, theme)), None))
    } else {
        parent
    }
}

/// As [`first`], but the press is claimed under `name` and drawn by [`face`] where that is put:
/// a bottom tab is pressed anywhere in its quarter, and only its pill shows it, as a phone's own
/// tab bars do (the whole quarter lit was a hard grey box, seen on the emulator).
pub(crate) fn claims<P: ParentElement>(parent: P, name: Name) -> P {
    if SHOWN {
        parent.child(layer(None, Some(name)))
    } else {
        parent
    }
}

/// The pressed face of the press claimed under `name` ([`claims`]), over all of what it is put
/// in, `radius` round at its corners.
pub(crate) fn face(name: Name, radius: f32, theme: Theme) -> impl IntoElement {
    let colour = pressed_colour(theme);
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let claimed = PRESS.get().filter(|press| {
                press.window == window.window_handle().window_id() && press.named == Some(name)
            });
            if claimed.is_some() {
                window.paint_quad(
                    gpui::fill(bounds, colour).corner_radii(crate::window::zoom::px(radius)),
                );
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// The text's colour at `theme::PRESSED`: over any face, what `theme.answering(face, PRESSED)`
/// makes of it, as a mouse's press shows.
fn pressed_colour(theme: Theme) -> Rgba {
    Rgba {
        a: crate::window::theme::PRESSED,
        ..theme.text
    }
}

/// What takes a finger's press on what it is put in: with `drawn` (corners, theme) it paints the
/// pressed face there itself ([`pressed_colour`]); with `named` the press is claimed under that
/// name for a [`face`] elsewhere.
fn layer(drawn: Option<(f32, Theme)>, named: Option<Name>) -> impl IntoElement {
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |bounds, hitbox, window, _| {
            if let Some((radius, theme)) = drawn.filter(|_| pressed_in(window) == Some(bounds)) {
                window.paint_quad(
                    gpui::fill(bounds, pressed_colour(theme))
                        .corner_radii(crate::window::zoom::px(radius)),
                );
            }
            window.on_mouse_event(move |event: &TouchDragEvent, phase, window, cx| {
                // Bubbling, the innermost first: what is pressed inside another (a card's
                // button) takes the press before the card can.
                if phase != gpui::DispatchPhase::Bubble
                    || event.phase != TouchPhase::Started
                    || !hitbox.is_hovered_at(event.start_position, window)
                    || PRESS
                        .get()
                        .is_some_and(|press| press.at == event.start_position)
                {
                    return;
                }
                let n = PRESSES.get() + 1;
                PRESSES.set(n);
                PRESS.set(Some(Press {
                    window: window.window_handle().window_id(),
                    bounds,
                    at: event.start_position,
                    n,
                    named,
                }));
                window.refresh();
                window
                    .spawn(cx, async move |cx| {
                        cx.background_executor().timer(AT_MOST).await;
                        let _ = cx.update(|window, _| {
                            if PRESS.get().is_some_and(|press| press.n == n) {
                                lifted(window);
                            }
                        });
                    })
                    .detach();
            });
            // The tap's release (its click runs then), a scroll, or a long press: over.
            window.on_mouse_event(|_: &MouseUpEvent, phase, window, _| {
                if phase == gpui::DispatchPhase::Capture {
                    lifted(window);
                }
            });
            window.on_mouse_event(|_: &ScrollEvent, phase, window, _| {
                if phase == gpui::DispatchPhase::Capture {
                    lifted(window);
                }
            });
            window.on_mouse_event(|_: &LongPressEvent, phase, window, _| {
                if phase == gpui::DispatchPhase::Capture {
                    lifted(window);
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}
