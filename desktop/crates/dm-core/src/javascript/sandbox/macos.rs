// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! macOS's sandbox, entered by the process itself with one of the system's own named profiles
//! (`sandbox_init`, in libSystem). Deprecated as an API since macOS 10.8 in favour of a signed
//! app's entitlements, which cover a whole app and not one helper process; it still works, and
//! the browsers' own helper processes are shut in the same way.

use std::ffi::{c_char, c_int, CStr};

/// The profile, in the sandbox's own language: no network, no program started or process
/// made, no file written, made, moved or removed.
const PROFILE: &CStr = c"(version 1)\n(allow default)\n(deny network*)\n(deny process-fork)\n(deny process-exec*)\n(deny file-write*)\n";

/// No flags: the profile is given as its text.
const AS_TEXT: u64 = 0;

extern "C" {
    fn sandbox_init(profile: *const c_char, flags: u64, error: *mut *mut c_char) -> c_int;
    fn sandbox_free_error(error: *mut c_char);
}

pub(super) fn enter() -> Result<(), String> {
    let mut error: *mut c_char = std::ptr::null_mut();
    // SAFETY: a NUL-terminated profile name the system defines; `error` receives a message the
    // call allocates, freed below.
    let entered = unsafe { sandbox_init(PROFILE.as_ptr(), AS_TEXT, &mut error) };
    if entered == 0 {
        return Ok(());
    }
    let why = if error.is_null() {
        "no reason given".to_string()
    } else {
        // SAFETY: a NUL-terminated message from `sandbox_init`.
        let why = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: the message `sandbox_init` allocated, freed once, after it was read.
        unsafe { sandbox_free_error(error) };
        why
    };
    Err(format!(
        "macOS's sandbox would not take the JavaScript host: {why}"
    ))
}
