// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What a window's draw does that it must not (gaps row 180): wait for the
//! engine (`Backend::block_on`), copy the settings (`Backend::settings`),
//! or ask the disk. Counted in debug builds only, and only on a thread
//! while it draws (between [`drawing`] and the end of what it returned), so
//! a check can hold a frame to none of the first and third and one copy of
//! the settings at most. A release build counts nothing: every call here is
//! empty there and goes away.
//!
//! The disk is counted by a small library a check preloads
//! (`scripts/fs-count.c`): it counts each thread's file-system calls and
//! says how many as `dm_fs_calls`. Without it, the disk is not counted
//! (`Costs::fs` is `None`), so a check knows it was not, rather than
//! reading a zero.

use serde::Serialize;

/// What draws have done since dm started (debug builds).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Costs {
    /// Waits on the engine (`Backend::block_on`) made while drawing.
    pub block_on: u64,
    /// Copies of the settings (`Backend::settings`) made while drawing.
    pub settings: u64,
    /// File-system calls made while drawing, where the counting library is
    /// loaded.
    pub fs: Option<u64>,
}

/// Marks this thread as drawing until it is dropped; nested marks count as
/// one.
#[must_use = "the thread counts as drawing only while this is kept"]
pub struct Drawing {
    #[cfg(debug_assertions)]
    _not_send: std::marker::PhantomData<*const ()>,
}

/// This thread draws from now until the mark returned is dropped.
#[inline]
pub fn drawing() -> Drawing {
    #[cfg(debug_assertions)]
    counted::enter();
    Drawing {
        #[cfg(debug_assertions)]
        _not_send: std::marker::PhantomData,
    }
}

impl Drop for Drawing {
    #[inline]
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        counted::leave();
    }
}

/// The engine was waited for on this thread.
#[inline]
pub fn waited() {
    #[cfg(debug_assertions)]
    counted::count(&counted::BLOCK_ON);
}

/// The settings were copied on this thread.
#[inline]
pub fn copied_settings() {
    #[cfg(debug_assertions)]
    counted::count(&counted::SETTINGS);
}

/// What draws have done so far; `None` in a release build, which does not
/// count.
#[must_use]
pub fn so_far() -> Option<Costs> {
    #[cfg(debug_assertions)]
    {
        Some(counted::so_far())
    }
    #[cfg(not(debug_assertions))]
    {
        None
    }
}

#[cfg(debug_assertions)]
mod counted {
    use std::cell::Cell;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
    #[cfg(unix)]
    use std::sync::OnceLock;

    pub(super) static BLOCK_ON: AtomicU64 = AtomicU64::new(0);
    pub(super) static SETTINGS: AtomicU64 = AtomicU64::new(0);
    static FS: AtomicU64 = AtomicU64::new(0);
    static FS_COUNTED: AtomicBool = AtomicBool::new(false);

    thread_local! {
        /// How deep in drawing marks this thread is.
        static DEPTH: Cell<u32> = const { Cell::new(0) };
        /// The library's count for this thread as the outermost mark began.
        static FS_AT: Cell<u64> = const { Cell::new(0) };
    }

    #[cfg(unix)]
    type Counter = extern "C" fn() -> u64;

    /// The preloaded library's count of this thread's file-system calls,
    /// where it is loaded. A library preloaded beside dm is a Unix way
    /// (`LD_PRELOAD`, `DYLD_INSERT_LIBRARIES`); Windows has none to ask.
    #[cfg(not(unix))]
    fn fs_calls() -> Option<u64> {
        None
    }

    /// The preloaded library's count of this thread's file-system calls,
    /// where it is loaded.
    #[cfg(unix)]
    fn fs_calls() -> Option<u64> {
        static COUNTER: OnceLock<Option<Counter>> = OnceLock::new();
        let counter = COUNTER.get_or_init(|| {
            // SAFETY: `dlsym` is given a valid handle and a NUL-terminated
            // name; a symbol it finds by this name is the counting
            // library's `unsigned long long dm_fs_calls(void)`, which has
            // this type.
            unsafe {
                let found = libc::dlsym(libc::RTLD_DEFAULT, c"dm_fs_calls".as_ptr());
                (!found.is_null()).then(|| std::mem::transmute::<*mut libc::c_void, Counter>(found))
            }
        });
        counter.map(|count| count())
    }

    pub(super) fn enter() {
        let depth = DEPTH.with(|d| {
            d.set(d.get() + 1);
            d.get()
        });
        if depth == 1 {
            if let Some(now) = fs_calls() {
                FS_AT.with(|at| at.set(now));
            }
        }
    }

    pub(super) fn leave() {
        let depth = DEPTH.with(|d| {
            d.set(d.get().saturating_sub(1));
            d.get()
        });
        if depth == 0 {
            if let Some(now) = fs_calls() {
                let at = FS_AT.with(Cell::get);
                FS.fetch_add(now.saturating_sub(at), Relaxed);
                FS_COUNTED.store(true, Relaxed);
            }
        }
    }

    pub(super) fn count(what: &AtomicU64) {
        if DEPTH.with(Cell::get) > 0 {
            what.fetch_add(1, Relaxed);
        }
    }

    pub(super) fn so_far() -> super::Costs {
        super::Costs {
            block_on: BLOCK_ON.load(Relaxed),
            settings: SETTINGS.load(Relaxed),
            fs: FS_COUNTED.load(Relaxed).then(|| FS.load(Relaxed)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only what is done while drawing counts, on the thread that draws,
    /// nested marks counting as one; the backend's own waits and copies
    /// among it. One test, so no other draws beside it.
    #[test]
    fn only_a_draws_own_waits_and_copies_are_counted() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let backend = crate::app::backend::testing::backend(&runtime, dir.path());
        let before = so_far().unwrap();
        waited();
        copied_settings();
        let _ = backend.list();
        assert_eq!(so_far().unwrap(), before);
        {
            let _outer = drawing();
            let _inner = drawing();
            waited();
            copied_settings();
            // Another thread's wait, beside the draw, is not the draw's.
            std::thread::spawn(waited).join().unwrap();
            let _ = backend.list();
            let _ = backend.settings();
        }
        waited();
        let after = so_far().unwrap();
        assert_eq!(after.block_on, before.block_on + 2);
        assert_eq!(after.settings, before.settings + 2);
        // No counting library in a test: the disk is said to be uncounted.
        assert_eq!(after.fs, None);
    }
}
