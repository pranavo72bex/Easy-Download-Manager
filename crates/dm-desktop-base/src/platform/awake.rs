// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Keeping the computer awake while a download is running: a 25 GB file on a
//! machine set to sleep after ten minutes would otherwise stop at the first
//! idle spell, every socket dropped. Windows is asked not to sleep for as
//! long as a row is Running, and told it may again when none is; a phone
//! is held by a wake lock the app takes (`phone::keep_awake`, gaps row
//! N17). Other systems leave sleep to their own power settings, so there
//! this does nothing.

/// What the queue's busyness last asked of the system, so the system is
/// told only when that changes.
#[derive(Default)]
pub struct Keeper {
    held: bool,
}

impl Keeper {
    /// The new state to set when it differs from the last, else `None`.
    pub fn step(&mut self, busy: bool) -> Option<bool> {
        if busy == self.held {
            return None;
        }
        self.held = busy;
        Some(busy)
    }
}

/// Holds the system awake while `busy` says a download is running. Looked at on a thread of
/// its own each time the queue changes (`changes::subscribe`), not every few seconds (gaps row
/// S18): the setting belongs to the thread that made it and lasts as long as the thread does.
pub fn watch(busy: impl Fn() -> bool + Send + 'static) {
    #[cfg(any(windows, target_os = "android"))]
    {
        // One nudge waiting at most: changes that come while the queue is being looked at are
        // one more look, not one each.
        let (nudge, nudged) = std::sync::mpsc::sync_channel::<()>(1);
        dm_shell::app::changes::subscribe(move || {
            let _ = nudge.try_send(());
        });
        let _ = std::thread::Builder::new()
            .name("dm-awake".into())
            .spawn(move || {
                let mut keeper = Keeper::default();
                loop {
                    if let Some(hold) = keeper.step(busy()) {
                        hold_awake(hold);
                    }
                    if nudged.recv().is_err() {
                        return;
                    }
                }
            });
    }
    #[cfg(not(any(windows, target_os = "android")))]
    let _ = busy;
}

/// How many phone watches have started in this process: the newest is the one that speaks.
static PHONE_WATCH: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The phone watch running now steps aside at its next look, leaving the foreground service as
/// it is: called as a window's dm winds down, before its downloads stop, so the service holds
/// the process until dm boots again without the window (`dm-android`'s engine), and nothing
/// reads the closed queue.
pub fn retire_phone_watch() {
    PHONE_WATCH.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

/// On a phone: tells it what keeps dm busy (`phone::busy`), each time the queue changes and
/// once a second while anything does, so its ongoing notification follows the count and the
/// speed and its wake lock is held while a row runs, a queued one is due or a finished torrent
/// is shared (gaps row N6). `summary` reads the queue; the phone decides what to show and
/// when. Its own thread, as `watch`'s.
pub fn watch_phone(summary: impl Fn() -> Option<crate::phone::Busy> + Send + 'static) {
    // The newest watch alone speaks for dm: a phone's dm that goes on without its window boots
    // again in the same process (`dm-android`'s engine), and the window's watch, whose queue
    // has closed, then steps aside at its next look rather than read a queue that is gone.
    let generation = PHONE_WATCH.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let newest = move || PHONE_WATCH.load(std::sync::atomic::Ordering::SeqCst) == generation;
    let (nudge, nudged) = std::sync::mpsc::sync_channel::<()>(1);
    dm_shell::app::changes::subscribe(move || {
        let _ = nudge.try_send(());
    });
    let _ = std::thread::Builder::new()
        .name("dm-awake".into())
        .spawn(move || loop {
            if !newest() {
                return;
            }
            let busy = summary();
            let held = busy.is_some();
            crate::phone::busy(busy);
            let looked_again = if held {
                // Seeding and the speed change with no change to the queue's rows.
                nudged.recv_timeout(std::time::Duration::from_secs(1))
            } else {
                nudged
                    .recv()
                    .map_err(|_| std::sync::mpsc::RecvTimeoutError::Disconnected)
            };
            if looked_again == Err(std::sync::mpsc::RecvTimeoutError::Disconnected) {
                return;
            }
        });
}

/// Keeps the computer awake for as long as a row of `queue` is Running ([`watch`]), the queue
/// read on `runtime`. A phone is watched by [`PhoneQueue::watch`] instead, once the backend is
/// made (`boot::start`).
#[cfg(not(target_os = "android"))]
pub(crate) fn watch_queue(queue: &dm_core::DownloadList, runtime: &tokio::runtime::Runtime) {
    let (queue, handle) = (queue.clone(), runtime.handle().clone());
    watch(move || {
        handle
            .block_on(queue.list())
            .iter()
            .any(|row| row.state == dm_core::State::Running)
    });
}

/// On a phone the queue is watched by [`PhoneQueue::watch`].
#[cfg(target_os = "android")]
pub(crate) fn watch_queue(_queue: &dm_core::DownloadList, _runtime: &tokio::runtime::Runtime) {}

/// The queue the phone's watch reads, taken before the backend is made (which takes the
/// queue): on a phone a copy of it.
#[cfg(target_os = "android")]
pub(crate) struct PhoneQueue(dm_core::DownloadList);

/// The queue the phone's watch reads: nothing, as only a phone has that watch.
#[cfg(not(target_os = "android"))]
pub(crate) struct PhoneQueue;

#[cfg(target_os = "android")]
impl PhoneQueue {
    /// A copy of `queue`.
    pub(crate) fn of(queue: &dm_core::DownloadList) -> Self {
        Self(queue.clone())
    }

    /// The phone kept awake, its ongoing notification up, for as long as anything keeps dm
    /// busy: a row running, one queued and due, a finished torrent shared (gaps row N6;
    /// [`watch_phone`]), in the language of `backend`'s settings.
    pub(crate) fn watch(
        self,
        backend: &dm_shell::app::backend::Backend,
        runtime: &tokio::runtime::Runtime,
    ) {
        let phone_queue = self.0;
        let (settings, handle) = (backend.settings_handle(), runtime.handle().clone());
        watch_phone(move || {
            // The notification's words in the language the window is in.
            let language = dm_core::lock(&settings).general.language.clone();
            crate::i18n::set(crate::i18n::chosen(&language));
            let rows = handle.block_on(phone_queue.list_shared());
            crate::phone::Busy::of(&rows, phone_queue.may_run(), dm_core::sharing_any())
        });
    }
}

#[cfg(not(target_os = "android"))]
impl PhoneQueue {
    /// Nothing is kept of `queue`.
    pub(crate) fn of(_queue: &dm_core::DownloadList) -> Self {
        Self
    }

    /// Nothing to watch: the computer is kept awake by [`watch_queue`].
    pub(crate) fn watch(
        self,
        _backend: &dm_shell::app::backend::Backend,
        _runtime: &tokio::runtime::Runtime,
    ) {
    }
}

// Holds the system awake, or lets it sleep: the phone's wake lock, or
// Windows' thread state.
#[cfg(target_os = "android")]
use crate::phone::keep_awake as hold_awake;
#[cfg(windows)]
use win::hold as hold_awake;

#[cfg(windows)]
mod win {
    use windows_sys::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
    };

    pub(super) fn hold(on: bool) {
        let flags = if on {
            ES_CONTINUOUS | ES_SYSTEM_REQUIRED
        } else {
            ES_CONTINUOUS
        };
        // SAFETY: a plain call with flags; it affects only this thread's request.
        unsafe {
            SetThreadExecutionState(flags);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_is_told_only_when_the_answer_changes() {
        let mut keeper = Keeper::default();
        assert_eq!(keeper.step(false), None);
        assert_eq!(keeper.step(true), Some(true));
        assert_eq!(keeper.step(true), None);
        assert_eq!(keeper.step(false), Some(false));
        assert_eq!(keeper.step(false), None);
    }
}
