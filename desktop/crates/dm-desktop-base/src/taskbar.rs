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
mod dock {
    use super::Outside;
    use objc2::rc::Retained;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSApplication, NSDockTile, NSImageView, NSProgressIndicator, NSProgressIndicatorStyle,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

    pub(super) struct Tile {
        tile: Retained<NSDockTile>,
        picture: Retained<NSImageView>,
        bar: Retained<NSProgressIndicator>,
    }

    impl Tile {
        /// None off the main thread, which AppKit's objects belong to, or
        /// where the program has no picture yet.
        pub(super) fn new() -> Option<Tile> {
            let mtm = MainThreadMarker::new()?;
            let app = NSApplication::sharedApplication(mtm);
            let tile = app.dockTile();
            let icon = app.applicationIconImage()?;
            let size = tile.size();
            let picture = NSImageView::imageViewWithImage(&icon, mtm);
            picture.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), size));
            // A bar a tenth of the tile tall, across its foot, as Finder's
            // own copy shows one.
            let frame = NSRect::new(
                NSPoint::new(size.width * 0.08, size.height * 0.06),
                NSSize::new(size.width * 0.84, (size.height * 0.1).max(8.0)),
            );
            let bar = NSProgressIndicator::initWithFrame(NSProgressIndicator::alloc(mtm), frame);
            bar.setStyle(NSProgressIndicatorStyle::Bar);
            bar.setIndeterminate(false);
            bar.setMinValue(0.0);
            bar.setMaxValue(100.0);
            bar.setHidden(true);
            picture.addSubview(&bar);
            Some(Tile { tile, picture, bar })
        }

        pub(super) fn show(&self, now: Outside) {
            match now.percent {
                Some(percent) => {
                    self.bar.setDoubleValue(f64::from(percent));
                    self.bar.setHidden(false);
                    self.tile.setContentView(Some(&self.picture));
                }
                None => {
                    self.bar.setHidden(true);
                    // The Dock draws dm's own picture again.
                    self.tile.setContentView(None);
                }
            }
            let badge = (now.running > 0).then(|| NSString::from_str(&now.running.to_string()));
            self.tile.setBadgeLabel(badge.as_deref());
            self.tile.display();
        }
    }
}

/// The launcher's progress and count on Linux: Unity's `LauncherEntry`
/// signal, which GNOME's Dash to Dock and Ubuntu Dock, KDE's task manager,
/// Plank and others read, sent on the session bus from a thread of its own
/// so the window never waits on the bus.
#[cfg(target_os = "linux")]
mod launcher {
    use super::Outside;
    use std::collections::HashMap;
    use std::sync::mpsc::{channel, Sender};
    use zbus::zvariant::Value;

    /// The launcher dm is installed with (`dm-gpui.desktop`).
    const APP: &str = "application://dm-gpui.desktop";

    pub(super) struct Entry(Option<Sender<Outside>>);

    impl Entry {
        pub(super) fn start() -> Entry {
            let (send, take) = channel::<Outside>();
            let started = std::thread::Builder::new()
                .name("dm-launcher".into())
                .spawn(move || {
                    let Ok(bus) = zbus::blocking::Connection::session() else {
                        return;
                    };
                    while let Ok(mut now) = take.recv() {
                        // Only the latest of a burst is sent.
                        while let Ok(later) = take.try_recv() {
                            now = later;
                        }
                        let _ = bus.emit_signal(
                            None::<&str>,
                            "/com/canonical/unity/launcherentry/dm",
                            "com.canonical.Unity.LauncherEntry",
                            "Update",
                            &(APP, properties(now)),
                        );
                    }
                });
            Entry(started.ok().map(|_| send))
        }

        pub(super) fn show(&self, now: Outside) {
            if let Some(send) = &self.0 {
                let _ = send.send(now);
            }
        }
    }

    /// What the signal says of `now`.
    pub(super) fn properties(now: Outside) -> HashMap<&'static str, Value<'static>> {
        let mut said = HashMap::new();
        said.insert(
            "progress",
            Value::from(f64::from(now.percent.unwrap_or(0)) / 100.0),
        );
        said.insert("progress-visible", Value::from(now.percent.is_some()));
        said.insert("count", Value::from(now.running as i64));
        said.insert("count-visible", Value::from(now.running > 0));
        said
    }
}

#[cfg(windows)]
mod win {
    use super::Mode;
    use std::ffi::c_void;
    use windows_sys::core::GUID;
    use windows_sys::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

    // CLSID_TaskbarList and IID_ITaskbarList3.
    const CLSID: GUID = GUID::from_u128(0x56fdf344_fd6d_11d0_958a_006097c9a090);
    const IID: GUID = GUID::from_u128(0xea1afb91_9e28_4b86_90e9_9e9f8a5eefaf);

    const NOPROGRESS: i32 = 0;
    const INDETERMINATE: i32 = 1;
    const NORMAL: i32 = 2;
    const ERROR: i32 = 4;
    const PAUSED: i32 = 8;

    type Hr = i32;
    type Plain = unsafe extern "system" fn(*mut c_void) -> u32;
    type Init = unsafe extern "system" fn(*mut c_void) -> Hr;

    /// ITaskbarList3's methods up to the two used, in the order the
    /// interface declares them (IUnknown, ITaskbarList, ITaskbarList2).
    #[repr(C)]
    struct Vtable {
        query_interface: usize,
        add_ref: usize,
        release: Plain,
        hr_init: Init,
        add_tab: usize,
        delete_tab: usize,
        activate_tab: usize,
        set_active_alt: usize,
        mark_fullscreen_window: usize,
        set_progress_value: unsafe extern "system" fn(*mut c_void, isize, u64, u64) -> Hr,
        set_progress_state: unsafe extern "system" fn(*mut c_void, isize, i32) -> Hr,
    }

    pub(super) struct List(*mut *const Vtable);

    impl List {
        pub(super) fn new() -> Option<List> {
            let mut raw: *mut c_void = std::ptr::null_mut();
            // SAFETY: a documented call; `raw` receives an owned interface
            // pointer on success, released in `Drop`.
            let made = unsafe {
                CoCreateInstance(
                    &CLSID,
                    std::ptr::null_mut(),
                    CLSCTX_INPROC_SERVER,
                    &IID,
                    &mut raw,
                )
            };
            if made < 0 || raw.is_null() {
                return None;
            }
            let list = List(raw.cast());
            // SAFETY: `raw` is a live ITaskbarList3.
            let ready = unsafe { ((*(*list.0)).hr_init)(raw) };
            (ready >= 0).then_some(list)
        }

        pub(super) fn set(&self, hwnd: isize, mode: Mode) {
            let this = self.0.cast::<c_void>();
            // SAFETY: `this` is a live ITaskbarList3 for as long as `self`.
            unsafe {
                let vtable = &*(*self.0);
                let state = match mode {
                    Mode::Nothing => NOPROGRESS,
                    Mode::Unknown => INDETERMINATE,
                    Mode::Running { .. } => NORMAL,
                    Mode::Paused => PAUSED,
                    Mode::Failed => ERROR,
                };
                (vtable.set_progress_state)(this, hwnd, state);
                match mode {
                    Mode::Running { done, total } => {
                        (vtable.set_progress_value)(this, hwnd, done, total);
                    }
                    // Paused and red show a full bar in their colour.
                    Mode::Paused | Mode::Failed => {
                        (vtable.set_progress_value)(this, hwnd, 1, 1);
                    }
                    _ => {}
                }
            }
        }
    }

    impl Drop for List {
        fn drop(&mut self) {
            // SAFETY: the one reference this value owns.
            unsafe {
                ((*(*self.0)).release)(self.0.cast());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dm_core::Progress;

    fn row(state: State, downloaded: u64, total: Option<u64>, finished_at: Option<u64>) -> Entry {
        Entry {
            id: 1,
            url: "https://e.com/f.bin".into(),
            state,
            progress: Progress {
                downloaded,
                total,
                speed: 0.0,
                eta: None,
                estimated: false,
            },
            filename: "f.bin".into(),
            finished_at,
            category: None,
            checksum: None,
            verified: false,
            mirrors: 0,
            path: String::new(),
            referrer: String::new(),
            paused_by_schedule: false,
            description: String::new(),
            queue: String::new(),
            save_dir: None,
            save_name: None,
            accept_page: false,
            validator: None,
            quality: None,
            sound: None,
            span: None,
            shape: None,
            mp3: false,
            gif: false,
            delete_after: None,
            parts_dir: String::new(),
            began_in: String::new(),
            cookies: String::new(),
            cookies_once: false,
            headers: Vec::new(),
            replaces: None,
            speed_limit: 0,
            stalled: String::new(),
            fresh_link: false,
            title_hint: None,
            torrent_files: Default::default(),
            said: String::new(),
            post: None,
            posted: false,
        }
    }

    /// The Dock and a launcher show a bar in whole hundredths while sized
    /// downloads run, and the count of those running (gaps row U16).
    #[test]
    fn outside_the_window_a_bar_in_hundredths_and_a_count() {
        let rows = [
            row(State::Running, 50, Some(100), None),
            row(State::Running, 30, Some(300), None),
            row(State::Paused, 0, Some(10), None),
        ];
        let now = outside_of(mode_of(&rows, 0), running_of(&rows));
        assert_eq!(
            now,
            Outside {
                percent: Some(20),
                running: 2
            }
        );
        let unknown = [row(State::Running, 5, None, None)];
        assert_eq!(
            outside_of(mode_of(&unknown, 0), running_of(&unknown)),
            Outside {
                percent: None,
                running: 1
            }
        );
        let paused = [row(State::Paused, 1, Some(2), None)];
        assert_eq!(
            outside_of(mode_of(&paused, 0), running_of(&paused)),
            Outside {
                percent: None,
                running: 0
            }
        );
        // Past its size (a server's wrong length), the bar stops full.
        assert_eq!(
            outside_of(Mode::Running { done: 9, total: 4 }, 1).percent,
            Some(100)
        );
    }

    #[test]
    fn running_rows_add_up() {
        let rows = [
            row(State::Running, 50, Some(100), None),
            row(State::Running, 30, Some(300), None),
            row(State::Paused, 0, Some(10), None),
        ];
        assert_eq!(
            mode_of(&rows, 0),
            Mode::Running {
                done: 80,
                total: 400
            }
        );
    }

    #[test]
    fn a_running_row_of_unknown_size_is_indeterminate() {
        let rows = [row(State::Running, 5, None, None)];
        assert_eq!(mode_of(&rows, 0), Mode::Unknown);
    }

    #[test]
    fn paused_when_nothing_runs() {
        let rows = [row(State::Paused, 1, Some(2), None)];
        assert_eq!(mode_of(&rows, 0), Mode::Paused);
        assert_eq!(
            mode_of(&[row(State::Completed, 2, Some(2), Some(1))], 9),
            Mode::Nothing
        );
    }

    #[test]
    fn a_failure_is_red_for_a_minute_only() {
        let rows = [row(State::Failed("x".into()), 0, None, Some(1000))];
        assert_eq!(mode_of(&rows, 1030), Mode::Failed);
        assert_eq!(mode_of(&rows, 1100), Mode::Nothing);
    }
}
