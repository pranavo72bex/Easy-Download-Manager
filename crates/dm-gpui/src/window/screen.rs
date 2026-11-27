// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The words on screen, where they were laid out this frame: what the
//! control port's `ui.texts` gives and `ui.click` aims at. Each piece of
//! text the window draws through
//! [`Screen::label`] records its bounds as it is laid out, so the record is
//! of the frame on screen, not of what the window meant to draw; only while
//! a control port is open ([`record`]), as nothing else reads it.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{div, prelude::*, AnyWindowHandle, App, Bounds, Global, Pixels, SharedString};

/// A piece of text, and where it was laid out (window points).
#[derive(Debug, Clone)]
pub struct Drawn {
    /// The words.
    pub text: SharedString,
    /// Where they can be seen: their place, less what the window or a scrolled page hides.
    pub bounds: Bounds<Pixels>,
    /// Where they were laid out, the part cut off included: words that run past the window's
    /// edge show it here (gaps row E2). Read by the window's tests.
    #[cfg_attr(not(test), allow(dead_code))]
    pub laid: Bounds<Pixels>,
}

/// This frame's words, in the order they were laid out (back to front), and the width of the
/// window they are laid out in, in points of dm's design (0 where it was not said).
#[derive(Clone, Default)]
pub struct Screen(Rc<RefCell<Vec<Drawn>>>, Rc<std::cell::Cell<f32>>);

impl Screen {
    /// A new frame: what the last one drew is forgotten.
    pub fn begin(&self) {
        self.0.borrow_mut().clear();
    }

    /// A new frame of `window`: what the last one drew is forgotten, and the window's width
    /// kept for a form's rows to lay their labels out by (`native::row`, gaps row E32).
    pub fn begin_in(&self, window: &gpui::Window) {
        self.begin();
        let width = f32::from(window.viewport_size().width) / crate::window::zoom::factor();
        self.1.set(width);
    }

    /// The width of the window this frame is laid out in, in points of dm's design; 0 where
    /// it was not said.
    pub fn width(&self) -> f32 {
        self.1.get()
    }

    /// The words of the last frame drawn, back to front.
    pub fn drawn(&self) -> Vec<Drawn> {
        self.0.borrow().clone()
    }

    /// `text`, drawn, and recorded where it is laid out.
    pub fn label(&self, text: impl Into<SharedString>) -> impl IntoElement {
        let text: SharedString = text.into();
        // Right-to-left words that wrap are broken in reading order (gaps row E52).
        if crate::widgets::reading::needs(&text) {
            return self
                .mark(text.clone(), crate::widgets::reading::Paragraph(text))
                .into_any_element();
        }
        self.mark(text.clone(), text).into_any_element()
    }

    /// `element`, drawn, and recorded as saying `text` where it is laid
    /// out: words drawn by an element of their own (text to select and
    /// copy, a digest's) are read and clicked as a label's are.
    pub fn mark(
        &self,
        text: impl Into<SharedString>,
        element: impl IntoElement,
    ) -> impl IntoElement {
        // With no control port open nothing reads the record: the words are
        // drawn as they are, without a box around each to say where (about
        // half the elements of a screenful of rows, gaps row 180).
        if !RECORDING.load(Ordering::Relaxed) {
            return element.into_any_element();
        }
        let words: SharedString = text.into();
        let record = Rc::clone(&self.0);
        // Where the element was laid out, told by the box around it as its
        // contents are placed: a canvas of its own beside it, for each piece
        // of text, was an element more to lay out and paint in every frame,
        // about a quarter of the main window's (gaps row 176).
        div()
            .relative()
            .child(element)
            .on_children_prepainted(move |placed, window, _| {
                let Some(bounds) = placed.first() else {
                    return;
                };
                // Only what can be seen: words scrolled out of a page's
                // view are not on screen, and a click aimed at them
                // landed on whatever was there.
                let seen = bounds.intersect(&window.content_mask().bounds);
                if seen.size.width > gpui::px(0.0) && seen.size.height > gpui::px(0.0) {
                    record.borrow_mut().push(Drawn {
                        text: words.clone(),
                        bounds: seen,
                        laid: *bounds,
                    });
                }
            })
            .into_any_element()
    }
}

/// Whether the words drawn are recorded: from when a control port opens
/// (`record`), the only reader of the record.
static RECORDING: AtomicBool = AtomicBool::new(false);

/// Records the words drawn from now on, for the control port just opened.
pub fn record() {
    RECORDING.store(true, Ordering::Relaxed);
}

/// Every window's words: the main window's and each dialog's, so the
/// control port reads and clicks words in any of them. A dialog whose
/// title changes as it goes (a download's progress) is also known by its
/// name. And each window's title
/// as it was last set, which GPUI's Windows and Linux windows do not say.
#[derive(Default)]
pub struct Screens(
    Vec<(AnyWindowHandle, Screen, bool)>,
    Vec<(AnyWindowHandle, SharedString)>,
    Vec<(AnyWindowHandle, SharedString)>,
);

impl Global for Screens {}

impl Screens {
    /// `screen` is `window`'s (`dialog` if it is a dialog's).
    pub fn register(window: AnyWindowHandle, screen: Screen, dialog: bool, cx: &mut App) {
        let screens = cx.default_global::<Screens>();
        screens.0.retain(|(w, _, _)| *w != window);
        screens.0.push((window, screen, dialog));
    }

    /// `window` is out of the port's sight, as a closed window is, while it
    /// is still going (a question's sheet sliding off, `question::close`:
    /// macOS's alone).
    #[cfg(target_os = "macos")]
    pub fn forget(window: AnyWindowHandle, cx: &mut App) {
        let screens = cx.default_global::<Screens>();
        screens.0.retain(|(w, _, _)| *w != window);
    }

    /// `window`'s dialog is called `name`, whatever its title says now (a
    /// progress window's title starts with how far it is).
    pub fn named(window: AnyWindowHandle, name: impl Into<SharedString>, cx: &mut App) {
        let screens = cx.default_global::<Screens>();
        screens.1.retain(|(w, _)| *w != window);
        screens.1.push((window, name.into()));
    }

    /// The name `window`'s dialog was given, if it was.
    pub fn name_of(window: AnyWindowHandle, cx: &App) -> Option<SharedString> {
        cx.try_global::<Screens>()?
            .1
            .iter()
            .find(|(w, _)| *w == window)
            .map(|(_, name)| name.clone())
    }

    /// `window`'s title as it was last set (`set_title`, `open_window`).
    pub fn title_of(window: AnyWindowHandle, cx: &App) -> Option<SharedString> {
        cx.try_global::<Screens>()?
            .2
            .iter()
            .find(|(w, _)| *w == window)
            .map(|(_, title)| title.clone())
    }

    /// `window`'s title is now `title`.
    fn titled(window: AnyWindowHandle, title: SharedString, cx: &mut App) {
        let screens = cx.default_global::<Screens>();
        match screens.2.iter_mut().find(|(w, _)| *w == window) {
            Some((_, was)) => *was = title,
            None => screens.2.push((window, title)),
        }
    }

    /// `text` drawn at `bounds` in `window`, recorded where the window's own
    /// words are, as far as it can be seen.
    pub fn record(window: &gpui::Window, cx: &App, text: String, bounds: Bounds<Pixels>) {
        if text.is_empty() {
            return;
        }
        let seen = bounds.intersect(&window.content_mask().bounds);
        if seen.size.width <= gpui::px(0.0) || seen.size.height <= gpui::px(0.0) {
            return;
        }
        let handle = window.window_handle();
        if let Some(screens) = cx.try_global::<Screens>() {
            if let Some((_, screen, _)) = screens.0.iter().find(|(w, _, _)| *w == handle) {
                screen.0.borrow_mut().push(Drawn {
                    text: text.into(),
                    bounds: seen,
                    laid: bounds,
                });
            }
        }
    }

    /// Each window still open, its words, and whether it is a dialog's;
    /// dialogs first, as they stand in front, the newest first.
    pub fn all(cx: &App) -> Vec<(AnyWindowHandle, Screen, bool)> {
        let open = cx.windows();
        let mut all: Vec<_> = cx
            .try_global::<Screens>()
            .map(|s| s.0.clone())
            .unwrap_or_default()
            .into_iter()
            .filter(|(w, _, _)| open.contains(w))
            .rev()
            .collect();
        all.sort_by_key(|(_, _, dialog)| !dialog);
        all
    }
}

/// The dialog called `name`, where one is open, brought in front as it stands: a dialog asked
/// for again while it is open is the same one, what was typed in it kept, not a second beside
/// it. Whether there was one.
pub fn bring_named(name: &str, cx: &mut App) -> bool {
    let open = Screens::all(cx).into_iter().find(|(handle, _, dialog)| {
        *dialog && Screens::name_of(*handle, cx).is_some_and(|n| n.as_ref() == name)
    });
    match open {
        Some((handle, _, _)) => {
            let _ = handle.update(cx, |_, window, _| window.activate_window());
            true
        }
        None => false,
    }
}

/// The name every dm window gives the desktop on Linux: the `.desktop`
/// entry's `StartupWMClass`.
pub const APP_ID: &str = "dm";

/// Opens a dialog's window as `App::open_window` does, and keeps the title
/// it was opened with as its name, where it was not named as it was built,
/// and as its title, where its first frame did not set another: the port
/// names dialogs by it, and Linux's and Windows' windows do not say their
/// title back (GPUI's X11, Wayland and Windows windows have no
/// `get_title`). Every dm window but the main one opens through here.
///
/// On Linux each window also says it is dm's (`APP_ID`): the class X11's
/// window managers and the app id Wayland's compositors match to the
/// launcher (`StartupWMClass=dm` in `packaging/deb.desktop`). Without it a
/// dock showed a second, generic icon beside dm's.
/// And on X11 a dialog stands where it asked to (over dm's window, B42), as
/// on macOS and Windows, not where the window manager finds room.
pub fn open_window<V: 'static + gpui::Render>(
    cx: &mut App,
    options: gpui::WindowOptions,
    build: impl FnOnce(&mut gpui::Window, &mut App) -> gpui::Entity<V>,
) -> gpui::Result<gpui::WindowHandle<V>> {
    open(cx, options, build, true)
}

/// Opens dm's main window as `open_window` opens a dialog's, but where the
/// desktop places it on Linux (`frame::DESKTOP_PLACES_IT`).
pub fn open_main_window<V: 'static + gpui::Render>(
    cx: &mut App,
    options: gpui::WindowOptions,
    build: impl FnOnce(&mut gpui::Window, &mut App) -> gpui::Entity<V>,
) -> gpui::Result<gpui::WindowHandle<V>> {
    open(cx, options, build, false)
}

fn open<V: 'static + gpui::Render>(
    cx: &mut App,
    mut options: gpui::WindowOptions,
    build: impl FnOnce(&mut gpui::Window, &mut App) -> gpui::Entity<V>,
    dialog: bool,
) -> gpui::Result<gpui::WindowHandle<V>> {
    // On Windows a dialog has no minimize button, as Windows' own dialogs
    // (and reference A's) have none: only the main window, whose title bar
    // dm draws, goes down to the taskbar. Not the floating drop target (a
    // pop-up), and nothing changes elsewhere.
    if cfg!(windows) && dialog && options.kind != gpui::WindowKind::PopUp {
        options.is_minimizable = false;
    }
    let title = options.titlebar.as_ref().and_then(|t| t.title.clone());
    // Not a window opened hidden, which a question is on macOS before it hangs from dm's window as
    // a sheet: the system places a sheet itself, and one asked for at a corner of its own was
    // not shown in turn (B17, B39, B40 and B48 failing in `human-qa.py`, 2026-11-23).
    if options.show {
        if let Some(bounds) = options.window_bounds.as_mut() {
            *bounds = on_whole_points(*bounds);
        }
    }
    // GPUI hands it to the system as it makes the window.
    options.app_id.get_or_insert_with(|| APP_ID.to_string());
    // Where a dialog asked to stand, for X11 to be told (`syswindow::place`).
    let asked_at = (dialog && crate::window::frame::DESKTOP_PLACES_IT)
        .then(|| options.window_bounds.map(|b| b.get_bounds().origin))
        .flatten();
    let opened = cx.open_window(options, |window, cx| {
        #[cfg(not(any(target_os = "macos", windows)))]
        if let Some(at) = asked_at {
            // Refused on Wayland, which places windows itself.
            let _ = crate::platform::syswindow::place(window, f32::from(at.x), f32::from(at.y));
        }
        #[cfg(any(target_os = "macos", windows))]
        let _ = asked_at;
        build(window, cx)
    })?;
    let handle: AnyWindowHandle = opened.into();
    if let Some(title) = title.filter(|t| !t.is_empty()) {
        if Screens::name_of(handle, cx).is_none() {
            Screens::named(handle, title.clone(), cx);
        }
        if Screens::title_of(handle, cx).is_none() {
            Screens::titled(handle, title, cx);
        }
    }
    Ok(opened)
}

/// `bounds` with its corner on a whole point. Centred on a screen whose free part is an odd
/// number of points wide (a Dock at the side), a window's left edge fell on a half point, and
/// macOS rounded its two edges apart: a 520-point dialog opened 521 points wide.
fn on_whole_points(bounds: gpui::WindowBounds) -> gpui::WindowBounds {
    let whole = |b: Bounds<Pixels>| {
        Bounds::new(gpui::point(b.origin.x.round(), b.origin.y.round()), b.size)
    };
    match bounds {
        gpui::WindowBounds::Windowed(b) => gpui::WindowBounds::Windowed(whole(b)),
        gpui::WindowBounds::Maximized(b) => gpui::WindowBounds::Maximized(whole(b)),
        gpui::WindowBounds::Fullscreen(b) => gpui::WindowBounds::Fullscreen(whole(b)),
    }
}

/// Sets `window`'s title, where it changed, and keeps it for the port
/// (`Screens::title_of`), which cannot ask GPUI's Windows and Linux windows
/// for it: a progress window's, which starts with how far it is.
pub fn set_title(window: &mut gpui::Window, title: &str, cx: &mut App) {
    let handle = window.window_handle();
    if Screens::title_of(handle, cx).is_some_and(|was| was.as_ref() == title) {
        return;
    }
    window.set_window_title(title);
    Screens::titled(handle, SharedString::from(title.to_string()), cx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size, WindowBounds};

    /// A dialog centred beside a Dock at the side stood on a half point and opened a point wider.
    #[test]
    fn a_window_is_asked_for_on_whole_points_at_its_own_size() {
        let asked = Bounds::new(point(px(497.5), px(355.5)), size(px(520.0), px(220.0)));
        let WindowBounds::Windowed(got) = on_whole_points(WindowBounds::Windowed(asked)) else {
            panic!("still a window of its own");
        };
        assert_eq!(got.origin, point(px(498.0), px(356.0)));
        assert_eq!(got.size, asked.size);
    }
}
