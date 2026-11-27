// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The top bar's search box: Finder's toolbar field, a magnifier shut that opens into a field
//! (Find, ⌘F), narrowing the list as one types.

#[allow(clippy::wildcard_imports)]
use super::*;
/// The search box's height, and its width open (Finder's toolbar field).
const BOX_HEIGHT: f32 = 26.0;
const BOX_OPEN: f32 = 240.0;
/// Shut, a toolbar button: its picture and room about it.
const BOX_SHUT: f32 = 32.0;
/// How long it takes to open and to shut: an exit is shorter than its enter (better-motion).
const OPENING: std::time::Duration = std::time::Duration::from_millis(250);
const SHUTTING: std::time::Duration = std::time::Duration::from_millis(150);

thread_local! {
    /// The search box's glide: whether it is opening (else shutting), when that began, and the
    /// width it began from, in points on screen. None before the window's first frame, which
    /// draws it as it stands, with no glide (better-motion: no state animation on first render).
    static GLIDE: std::cell::Cell<Option<(bool, std::time::Instant, f32)>> =
        const { std::cell::Cell::new(None) };
}

/// The search box's width this frame, in points on screen: `open`'s, or on its way there,
/// easing out, asking for the next frame meanwhile; at once where the system asks for less
/// motion.
pub(super) fn search_width(open: bool, window: &mut Window, cx: &Context<Downloads>) -> f32 {
    let of = crate::window::zoom::of;
    let target = |open: bool| of(if open { BOX_OPEN } else { BOX_SHUT });
    let now = std::time::Instant::now();
    let at = |(opening, began, from): (bool, std::time::Instant, f32)| {
        let length = if opening { OPENING } else { SHUTTING };
        let t = if cx.reduce_motion() {
            1.0
        } else {
            (began.elapsed().as_secs_f32() / length.as_secs_f32()).min(1.0)
        };
        let eased = 1.0 - (1.0 - t).powi(5);
        (from + (target(opening) - from) * eased, t < 1.0)
    };
    let glide = match GLIDE.with(std::cell::Cell::get) {
        None => (open, now - OPENING, target(open)),
        Some(g) if g.0 != open => (open, now, at(g).0),
        Some(g) => g,
    };
    GLIDE.with(|c| c.set(Some(glide)));
    let (width, moving) = at(glide);
    if moving {
        window.request_animation_frame();
    }
    width
}

/// The search box shut: its magnifier alone, a toolbar button's size, no edge and no field, as
/// Finder's; a click opens it (Find, ⌘F).
pub(super) fn shut_look(
    b: gpui::Stateful<gpui::Div>,
    theme: Theme,
    cx: &mut Context<Downloads>,
) -> gpui::Stateful<gpui::Div> {
    b.px_0()
        .justify_center()
        .border_color(gpui::transparent_black())
        .bg(gpui::transparent_black())
        .hover(move |s| s.bg(theme.stripe))
        .cursor_pointer()
        .tooltip(|window, cx| {
            gpui_component::tooltip::Tooltip::new(gpui::SharedString::from(t("Filter")))
                .build(window, cx)
        })
        .on_click(cx.listener(|view, _, window, cx| {
            view.find(window, cx);
            cx.notify();
        }))
}

/// Its corners, its focus ring's distance outside it, and its clear button's target.
const BOX_CORNER: f32 = native::RADIUS + 1.0;
const RING_OUT: f32 = space::XXS;
const CLEAR_TARGET: f32 = 24.0;

/// The search box, `width` points on screen wide, or the room it is given where none.
/// Whether the search box has the keyboard.
pub(super) fn search_focused(view: &Downloads, window: &Window, cx: &Context<Downloads>) -> bool {
    gpui::Focusable::focus_handle(view.list.filter.read(cx), cx).contains_focused(window, cx)
}

pub(super) fn search_box(
    view: &Downloads,
    width: Option<f32>,
    shut: bool,
    focused: bool,
    theme: Theme,
    screen: &Screen,
    cx: &mut Context<Downloads>,
) -> gpui::Stateful<gpui::Div> {
    let searching = !view.list.filter.read(cx).value().trim().is_empty();
    let shown = view.list.shown.len();
    div()
        .id("search")
        // Escape in the box clears it and gives the keyboard back to
        // the list.
        .on_key_down(cx.listener(|view, event: &gpui::KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" {
                // At a phone's width, the box closed as well (gaps row N7).
                view.layout.narrow.searching = false;
                view.clear_filter(window, cx);
                window.focus(&view.focus, cx);
                cx.stop_propagation();
            }
        }))
        .map(|d| match width {
            Some(width) => d.w(gpui::px(width)),
            None => d.flex_1().min_w_0(),
        })
        .h(px(BOX_HEIGHT))
        .relative()
        .flex()
        .items_center()
        .gap(px(if shut { 0.0 } else { space::S }))
        .px(px(space::M))
        .rounded(px(BOX_CORNER))
        .border_1()
        .border_color(theme.separator)
        .bg(theme.stripe)
        // Ringed while it has the keyboard, 2 points outside its edge as every control's ring is
        // (`native::ringed`), its corners following the box's (better-ui: outer = inner + gap):
        // the edge's colour alone, 1 point, barely said where the keys go.
        .when(focused, |d| {
            d.child(
                div()
                    .absolute()
                    .top(px(-RING_OUT))
                    .left(px(-RING_OUT))
                    .right(px(-RING_OUT))
                    .bottom(px(-RING_OUT))
                    .rounded(px(BOX_CORNER + RING_OUT))
                    .border_2()
                    .border_color(theme.ring(false)),
            )
        })
        // The magnifier at the left, as the system's search fields
        // and the editor's command centre draw it.
        .child(
            svg()
                .path("icons/search.svg")
                // A toolbar button's picture shut (the toolbar's own 18), the field's small one
                // open, as Finder's.
                .size(px(if shut {
                    native::ICON_TOOL
                } else {
                    native::ICON_SMALL
                }))
                .flex_none()
                .text_color(theme.muted),
        )
        .child(
            // Shut, the field takes no room and shows nothing: its words' first letter stood
            // beside the magnifier. It stays in the window, where Find gives it the keys.
            div()
                .flex_1()
                .min_w_0()
                .when(shut, |d| d.flex_none().w_0().invisible())
                .child(
                    gpui::Styled::h(
                        Input::new(&view.list.filter)
                            .appearance(false)
                            .aria_label(t("Filter")),
                        px(BOX_HEIGHT - 2.0),
                    )
                    .text_size(px(native::TEXT))
                    .px(px(0.0)),
                ),
        )
        // How many rows the words leave, and a way to clear them.
        .when(searching, |d| {
            d.child(
                div()
                    .flex_none()
                    .text_size(px(native::SMALL_TEXT))
                    .text_color(theme.muted)
                    .child(screen.label(tn("{} shown||{} shown", shown as u64, &[&shown]))),
            )
            .child(
                // Its target 24 points (WCAG 2.2, 2.5.8), the drawn circle 16 inside it: at 16
                // the whole target was the circle.
                div()
                    .id("clear-filter")
                    .role(Role::Button)
                    .aria_label(t("Clear"))
                    .flex_none()
                    .size(px(CLEAR_TARGET))
                    .mr(px(-space::XS))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(
                        div()
                            .size(px(native::ICON))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(theme.muted)
                            // A drawn mark named by what it does, not the font's "✕" (E41).
                            .child(screen.mark(
                                t("Clear"),
                                native::icon("icons/close.svg", native::MARK_TEXT, theme.window),
                            )),
                    )
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.clear_filter(window, cx);
                    })),
            )
        })
}
