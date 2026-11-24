// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Pieces of a row's cells (`row_cells`): the name's words, and the bar of how far it is.

use gpui::relative;

use super::*;
use crate::window::a11y::Told;

/// A row's name as drawn: its stem, cut short where it must be, then its extension whole; read
/// as the whole name, as it was one label.
pub(super) fn name_drawn(name: &SharedString, screen: &Screen) -> impl IntoElement {
    let (stem, extension) = name_parts(name);
    let drawn = div()
        .flex()
        .min_w_0()
        .overflow_hidden()
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(SharedString::from(stem.to_string())),
        )
        .child(
            div()
                .flex_none()
                .whitespace_nowrap()
                .child(SharedString::from(extension.to_string())),
        );
    // Read as the whole name, as it was one label; in a box that can be narrower than the name,
    // so the name ends in "…" where its cell is narrow, recorded or not (gaps row E7).
    div()
        .min_w_0()
        .overflow_hidden()
        .child(screen.mark(name.clone(), drawn))
}

/// The bar under a running row's status, `f` of the way along.
pub(crate) fn progress_track(id: u64, f: f64, told: bool, theme: Theme) -> AnyElement {
    let target = f.clamp(0.0, 1.0) as f32;
    let fill = div().h_full().rounded_full().bg(theme.selection);
    let track = div()
        .mt(px(space::XXS))
        .w_full()
        // Thick enough to read at a glance across the cell (gaps row E4).
        .h(px(4.0))
        .rounded_full()
        .bg(theme.separator)
        // Filled from the right where the window is mirrored (gaps row U35).
        .when(crate::window::mirror::rtl(), |track| {
            track.flex().flex_row_reverse()
        })
        .child(glided(id, target, fill));
    // How far it is, told to a screen reader as a bar's value.
    if told {
        track
            .id("bar")
            .progress_bar(t("Progress"), f)
            .into_any_element()
    } else {
        track.into_any_element()
    }
}

/// Whether the system asked for less motion: bars move straight to their width (gaps row K7).
static STILL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Set as the window starts, from the system's own setting.
pub(crate) fn set_still(still: bool) {
    STILL.store(still, std::sync::atomic::Ordering::Relaxed);
}

thread_local! {
    /// Each bar's glide, by download: the width it glides from and the one it glides to.
    static GLIDES: std::cell::RefCell<std::collections::HashMap<u64, (f32, f32)>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// A bar's fill at `target` of its track, gliding there over 300 ms from where it last stood
/// rather than jumping at each tick (gaps row K7). An animation of GPUI's own, drawn only
/// while it moves; none where the system asks for less motion.
fn glided(id: u64, target: f32, fill: gpui::Div) -> AnyElement {
    use gpui::{Animation, AnimationExt};
    let (from, to) = GLIDES.with(|g| {
        let mut g = g.borrow_mut();
        let entry = g.entry(id).or_insert((target, target));
        if (entry.1 - target).abs() > 0.0005 {
            *entry = (entry.1, target);
        }
        *entry
    });
    if STILL.load(std::sync::atomic::Ordering::Relaxed) || (to - from).abs() < 0.0005 {
        return fill.w(relative(to)).into_any_element();
    }
    fill.with_animation(
        gpui::ElementId::Name(format!("glide-{id}-{}", (to * 10_000.0) as u32).into()),
        Animation::new(std::time::Duration::from_millis(300)).with_easing(gpui::ease_in_out),
        move |fill, delta| fill.w(relative(from + (to - from) * delta)),
    )
    .into_any_element()
}

/// Whether the list has been drawn yet, and for each download's row when it was first drawn
/// and whether the list had been drawn before then.
type Arrived = (
    bool,
    std::collections::HashMap<u64, (std::time::Instant, bool)>,
);

thread_local! {
    /// When each download's row was first drawn: a row the window opened with does not fade in.
    static ARRIVED: std::cell::RefCell<Arrived> =
        std::cell::RefCell::new((false, std::collections::HashMap::new()));
}

/// How long a row just added takes to fade in.
const ARRIVING: std::time::Duration = std::time::Duration::from_millis(150);

/// Marks the list as drawn once: rows first drawn after this are ones just added.
pub(crate) fn list_drawn() {
    ARRIVED.with(|a| a.borrow_mut().0 = true);
}

/// Whether row `id` was added moments ago (and so fades in): its first drawing, after the
/// list's first, less than [`ARRIVING`] ago. A row scrolled into sight later does not fade.
pub(crate) fn just_added(id: u64) -> bool {
    if STILL.load(std::sync::atomic::Ordering::Relaxed) {
        return false;
    }
    ARRIVED.with(|a| {
        let mut a = a.borrow_mut();
        let listed_before = a.0;
        // When it was first drawn, and whether the list had been drawn before then.
        let (first, added) =
            *a.1.entry(id)
                .or_insert_with(|| (std::time::Instant::now(), listed_before));
        added && first.elapsed() < ARRIVING
    })
}

/// `row`, faded in over [`ARRIVING`] where it was just added (gaps row K7); as it is otherwise.
pub(crate) fn arriving<E: IntoElement + gpui::Styled + 'static>(id: u64, row: E) -> AnyElement {
    use gpui::{Animation, AnimationExt};
    if !just_added(id) {
        return row.into_any_element();
    }
    row.with_animation(
        gpui::ElementId::Name(format!("arrive-{id}").into()),
        Animation::new(ARRIVING).with_easing(gpui::ease_in_out),
        |row, delta| row.opacity(delta),
    )
    .into_any_element()
}

thread_local! {
    /// For each download: whether it was last seen running, and when it was first seen
    /// finished after that.
    static SETTLING: std::cell::RefCell<
        std::collections::HashMap<u64, (bool, Option<std::time::Instant>)>,
    > = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// How long a finished bar stays: it fills, then fades out (gaps row K7).
const SETTLE: std::time::Duration = std::time::Duration::from_millis(600);

/// Whether download `id`'s bar is settling: it was seen running and finished moments ago, so
/// its bar fills and fades rather than vanishing at once. Told each drawing whether it runs and
/// whether it is finished; none where the system asks for less motion.
pub(crate) fn settling(id: u64, running: bool, finished: bool) -> bool {
    SETTLING.with(|s| {
        let mut s = s.borrow_mut();
        let entry = s.entry(id).or_insert((false, None));
        if running {
            *entry = (true, None);
            return false;
        }
        if !finished {
            *entry = (false, None);
            return false;
        }
        if entry.0 && entry.1.is_none() {
            entry.1 = Some(std::time::Instant::now());
        }
        entry.0 = false;
        !STILL.load(std::sync::atomic::Ordering::Relaxed)
            && entry.1.is_some_and(|at| at.elapsed() < SETTLE)
    })
}

/// `row`'s settling bar where it has just finished ([`settling`]), or none.
pub(crate) fn finishing_bar(row: &Entry, theme: Theme) -> Option<AnyElement> {
    use dm_core::State;
    let (running, finished) = (row.state == State::Running, row.state == State::Completed);
    settling(row.id, running, finished).then(|| settled_track(row.id, theme))
}

/// A just-finished download's bar: full (glided there from where it stood), then faded out
/// over the second half of [`SETTLE`] (gaps row K7).
pub(crate) fn settled_track(id: u64, theme: Theme) -> AnyElement {
    use gpui::{Animation, AnimationExt};
    div()
        .w_full()
        .child(progress_track(id, 1.0, false, theme))
        .with_animation(
            gpui::ElementId::Name(format!("settle-{id}").into()),
            Animation::new(SETTLE),
            |track, delta| {
                track.opacity(if delta < 0.5 {
                    1.0
                } else {
                    2.0 * (1.0 - delta)
                })
            },
        )
        .into_any_element()
}

/// A download's state as a mark told by its shape (gaps row E4): a play mark while it runs,
/// pause bars when paused, a clock while it waits, a tick once done, a ring with "!" when it
/// failed; none for one cancelled. In the colour the row's words take.
pub(crate) fn state_mark(state: &dm_core::State, theme: Theme) -> Option<AnyElement> {
    use dm_core::State;
    let colour = if theme.is_chosen() {
        theme.text
    } else {
        match state {
            State::Failed(_) => theme.warning,
            State::Completed => theme.success,
            State::Running => theme.accent,
            _ => theme.muted,
        }
    };
    let picture = |path: &'static str| {
        gpui::svg()
            .path(path)
            .size(px(12.0))
            .flex_none()
            .text_color(colour)
            .into_any_element()
    };
    match state {
        State::Running => Some(picture("icons/resume.svg")),
        State::Paused => Some(picture("icons/stop.svg")),
        State::Queued => Some(picture("icons/place-unfinished.svg")),
        State::Completed => Some(picture("icons/place-finished.svg")),
        State::Failed(_) => Some(
            div()
                .flex_none()
                .size(px(12.0))
                .rounded_full()
                .border_1()
                .border_color(colour)
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(9.0))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(colour)
                .child("!")
                .into_any_element(),
        ),
        State::Cancelled => None,
    }
}

#[cfg(test)]
mod glide_tests {
    use super::*;

    /// A bar glides from where it last stood to its new width, and stands still where its
    /// width has not changed (gaps row K7).
    #[test]
    fn a_bar_glides_from_its_last_width_to_its_new_one() {
        let fill = || div();
        let glide = |id| GLIDES.with(|g| g.borrow().get(&id).copied());
        let _ = glided(7_000_001, 0.2, fill());
        assert_eq!(glide(7_000_001), Some((0.2, 0.2)), "a first width stands");
        let _ = glided(7_000_001, 0.5, fill());
        assert_eq!(glide(7_000_001), Some((0.2, 0.5)));
        // Drawn again with the same width (a hover, a tick with no news): the same glide.
        let _ = glided(7_000_001, 0.5, fill());
        assert_eq!(glide(7_000_001), Some((0.2, 0.5)));
        let _ = glided(7_000_001, 0.9, fill());
        assert_eq!(glide(7_000_001), Some((0.5, 0.9)));
    }

    /// A row fades in only where it was added after the list was first drawn, and only for a
    /// moment: one the window opened with, or one scrolled into sight later, does not (K7).
    #[test]
    fn only_a_row_just_added_fades_in() {
        assert!(!just_added(8_000_001), "a row the window opened with");
        list_drawn();
        assert!(!just_added(8_000_001), "the same row drawn again");
        assert!(just_added(8_000_002), "a row added since");
        std::thread::sleep(ARRIVING + std::time::Duration::from_millis(50));
        assert!(!just_added(8_000_002), "drawn again once it has faded in");
    }

    /// A bar settles only where its download was seen running and has just finished: not one
    /// finished when first seen, nor one stopped, nor once it has faded (K7).
    #[test]
    fn only_a_download_seen_finishing_settles() {
        assert!(
            !settling(9_000_001, false, true),
            "finished when first seen"
        );
        assert!(!settling(9_000_002, true, false), "running");
        assert!(settling(9_000_002, false, true), "seen finishing");
        assert!(
            settling(9_000_002, false, true),
            "drawn again while it settles"
        );
        assert!(!settling(9_000_003, true, false));
        assert!(!settling(9_000_003, false, false), "stopped, not finished");
        std::thread::sleep(SETTLE + std::time::Duration::from_millis(50));
        assert!(!settling(9_000_002, false, true), "once it has faded");
    }
}
