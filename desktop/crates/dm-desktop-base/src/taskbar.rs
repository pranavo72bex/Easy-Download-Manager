// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The progress on dm's taskbar button, so a minimised window still says how
//! far the downloads are: green while they run, yellow while they are
//! paused, red for a minute after one failed. Only Windows has such a
//! button; elsewhere [`Taskbar::show`] does nothing.
//!
//! The same, where the other systems show it ([`Taskbar::show_outside`],
//! gaps row U16): on macOS a bar over dm's picture in the Dock and a badge
//! of how many download; on Linux the launcher's progress and count (the
//! dock or panel that shows dm's launcher: GNOME's Dash to Dock, KDE's task
//! manager, Plank), asked through the session bus as every program asks it.

use dm_core::{Entry, State};

/// How long a failure keeps the button red.
const RED_FOR: u64 = 60;

/// What the button shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Nothing to show.
    Nothing,
    /// Running, with no total to measure against.
    Unknown,
    /// Running, with a total to measure against.
    Running {
        /// Bytes done so far.
        done: u64,
        /// Bytes in all.
        total: u64,
    },
    /// Downloads are paused.
    Paused,
    /// A download failed lately: the button stays red for a while.
    Failed,
}

/// What the rows say the button should show; `now` is seconds since 1970.
pub fn mode_of<R: std::borrow::Borrow<Entry>>(rows: &[R], now: u64) -> Mode {
    let rows: Vec<&Entry> = rows.iter().map(|r| r.borrow()).collect();
    let running: Vec<&Entry> = rows
        .iter()
        .copied()
        .filter(|r| r.state == State::Running)
        .collect();
    if !running.is_empty() {
        let (mut done, mut total) = (0u64, 0u64);
        for row in &running {
            match row.progress.total {
                Some(t) if t > 0 => {
                    done += row.progress.downloaded.min(t);
                    total += t;
                }
                _ => return Mode::Unknown,
            }
        }
        return Mode::Running { done, total };
    }
    let recent_failure = rows.iter().any(|r| {
        matches!(r.state, State::Failed(_))
            && r.finished_at
                .is_some_and(|at| now.saturating_sub(at) < RED_FOR)
    });
    if recent_failure {
        return Mode::Failed;
    }
    if rows.iter().any(|r| r.state == State::Paused) {
        return Mode::Paused;
    }
    Mode::Nothing
}

/// How many downloads run.
pub fn running_of<R: std::borrow::Borrow<Entry>>(rows: &[R]) -> usize {
    rows.iter()
        .filter(|r| r.borrow().state == State::Running)
        .count()
}

/// What the Dock or a launcher shows: a bar while downloads run and a count
/// of them. Their bars have no colours for paused or failed, so those show
/// nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outside {
    /// How far, in hundredths, while downloads with a size run; `None` hides
    /// the bar (nothing runs, or a size is unknown).
    pub percent: Option<u8>,
    /// How many downloads run; 0 hides the count.
    pub running: usize,
}

/// What `mode`, with `running` downloads, shows outside the window. In whole
/// hundredths, so the Dock is drawn again at most a hundred times a download.
pub fn outside_of(mode: Mode, running: usize) -> Outside {
    let percent = match mode {
        Mode::Running { done, total } if total > 0 => {
            Some((done.min(total) as u128 * 100 / total as u128) as u8)
        }
        _ => None,
    };
    Outside { percent, running }
}

/// The taskbar button of one window.
#[derive(Default)]
pub struct Taskbar {
    last: Option<Mode>,
    #[cfg(windows)]
    list: Option<win::List>,
    outside: Option<Outside>,
    #[cfg(target_os = "macos")]
    dock: Option<dock::Tile>,
    #[cfg(target_os = "linux")]
    launcher: Option<launcher::Entry>,
}

impl Taskbar {
    /// Shows `mode` on the button of `window`, when it differs from last time.
    pub fn show(&mut self, window: raw_window_handle::RawWindowHandle, mode: Mode) {
        if self.last == Some(mode) {
            return;
        }
        self.last = Some(mode);
        #[cfg(windows)]
        if let raw_window_handle::RawWindowHandle::Win32(handle) = window {
            if self.list.is_none() {
                self.list = win::List::new();
            }
            if let Some(list) = &self.list {
                list.set(handle.hwnd.get(), mode);
            }
        }
        #[cfg(not(windows))]
        let _ = window;
    }

    /// Shows how far the downloads are outside the window, on macOS's Dock
    /// and Linux's launcher, when it differs from last time; nothing on
    /// Windows, whose button [`Taskbar::show`] fills.
    pub fn show_outside(&mut self, now: Outside) {
        if self.outside == Some(now) {
            return;
        }
        self.outside = Some(now);
        #[cfg(target_os = "macos")]
        {
            if self.dock.is_none() {
                self.dock = dock::Tile::new();
            }
            if let Some(dock) = &self.dock {
                dock.show(now);
            }
        }
        #[cfg(target_os = "linux")]
        self.launcher
            .get_or_insert_with(launcher::Entry::start)
            .show(now);
    }
}

/// The Dock's tile: dm's picture with a bar over its foot while downloads
/// run, and the count of them in its badge.
#[cfg(target_os = "macos")]
mod dock;

/// The launcher's progress and count on Linux: Unity's `LauncherEntry`
/// signal, which GNOME's Dash to Dock and Ubuntu Dock, KDE's task manager,
/// Plank and others read, sent on the session bus from a thread of its own
/// so the window never waits on the bus.
#[cfg(target_os = "linux")]
mod launcher;

#[cfg(windows)]
mod win;

#[cfg(test)]
mod tests;
