//! The progress on dm's taskbar button, so a minimised window still says how
//! far the downloads are: green while they run, yellow while they are
//! paused, red for a minute after one failed. Only Windows has such a
//! button; elsewhere [`Taskbar::show`] does nothing.

use dm_core::{Entry, State};

/// How long a failure keeps the button red.
const RED_FOR: u64 = 60;

/// What the button shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Nothing,
    /// Running, with no total to measure against.
    Unknown,
    Running {
        done: u64,
        total: u64,
    },
    Paused,
    Failed,
}

/// What the rows say the button should show; `now` is seconds since 1970.
pub fn mode_of(rows: &[Entry], now: u64) -> Mode {
    let running: Vec<&Entry> = rows.iter().filter(|r| r.state == State::Running).collect();
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

/// The taskbar button of one window.
#[derive(Default)]
pub struct Taskbar {
    last: Option<Mode>,
    #[cfg(windows)]
    list: Option<win::List>,
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
            validator: None,
            quality: None,
            parts_dir: String::new(),
            began_in: String::new(),
            cookies: String::new(),
            cookies_once: false,
            replaces: None,
            speed_limit: 0,
            stalled: String::new(),
        }
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
