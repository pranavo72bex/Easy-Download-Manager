// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The words on screen, where they were laid out this frame: what the
//! control port's `ui.texts` gives and `ui.click` aims at, as the egui window
//! reads its painted text. Each piece of text the window draws through
//! [`Screen::label`] records its bounds as it is laid out, so the record is
//! of the frame on screen, not of what the window meant to draw.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{canvas, div, prelude::*, AnyWindowHandle, App, Bounds, Global, Pixels, SharedString};

/// A piece of text, and where it was laid out (window points).
#[derive(Debug, Clone)]
pub struct Drawn {
    /// The words.
    pub text: SharedString,
    /// Where.
    pub bounds: Bounds<Pixels>,
}

/// This frame's words, in the order they were laid out (back to front).
#[derive(Clone, Default)]
pub struct Screen(Rc<RefCell<Vec<Drawn>>>);

impl Screen {
    /// A new frame: what the last one drew is forgotten.
    pub fn begin(&self) {
        self.0.borrow_mut().clear();
    }

    /// The words of the last frame drawn, back to front.
    pub fn drawn(&self) -> Vec<Drawn> {
        self.0.borrow().clone()
    }

    /// `text`, drawn, and recorded where it is laid out.
    pub fn label(&self, text: impl Into<SharedString>) -> impl IntoElement {
        let text: SharedString = text.into();
        let record = Rc::clone(&self.0);
        let words = text.clone();
        div().relative().child(text).child(
            canvas(
                move |bounds, window, _| {
                    // Only what can be seen: words scrolled out of a page's
                    // view are not on screen, and a click aimed at them
                    // landed on whatever was there.
                    let seen = bounds.intersect(&window.content_mask().bounds);
                    if seen.size.width > gpui::px(0.0) && seen.size.height > gpui::px(0.0) {
                        record.borrow_mut().push(Drawn {
                            text: words,
                            bounds: seen,
                        });
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
}

/// Every window's words: the main window's and each dialog's, so the
/// control port reads and clicks words in any of them, as it does the
/// egui window's dialogs.
#[derive(Default)]
pub struct Screens(Vec<(AnyWindowHandle, Screen, bool)>);

impl Global for Screens {}

impl Screens {
    /// `screen` is `window`'s (`dialog` if it is a dialog's).
    pub fn register(window: AnyWindowHandle, screen: Screen, dialog: bool, cx: &mut App) {
        let screens = cx.default_global::<Screens>();
        screens.0.retain(|(w, _, _)| *w != window);
        screens.0.push((window, screen, dialog));
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
