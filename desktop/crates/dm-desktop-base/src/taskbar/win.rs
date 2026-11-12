// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
