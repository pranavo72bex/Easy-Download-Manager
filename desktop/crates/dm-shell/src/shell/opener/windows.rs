// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Windows: Explorer opens a file with its program, an address in the browser, and shows a file
//! selected in its folder; a path too long for the shell is handed over in its 8.3 short form.

use std::os::windows::process::CommandExt;
use std::path::Path;

use super::super::for_shell;

/// Shows `path` selected in its folder. Passed as it is, quoted by hand: Explorer reads
/// `/select,` and the path as one argument, and Rust's own quoting of a path with spaces wraps
/// the whole of it, which Explorer does not undo.
pub(crate) fn reveal(_: &Path, path: &str) -> Result<(), String> {
    std::process::Command::new("explorer")
        .raw_arg(format!("/select,\"{}\"", for_shell(path)))
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Opens `path` with its program. Not `cmd /C start`: cmd reads `&`, `^` and `%` in a name a
/// server chose, so a file called `a&calc` ran calc. Quoted by hand, as for `/select`: Explorer
/// splits its command line on commas, and Rust quotes only a path with a space in it.
pub(crate) fn open(_: &Path, path: &str) -> Result<(), String> {
    std::process::Command::new("explorer")
        .raw_arg(format!("\"{}\"", for_shell(path)))
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// The file system's 8.3 short form of a verbatim path, where it has one.
pub(crate) fn short_form(path: &str) -> Option<String> {
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is NUL-terminated; with no buffer the call only says
    // how long the answer is, NUL included.
    let needed = unsafe { GetShortPathNameW(wide.as_ptr(), std::ptr::null_mut(), 0) };
    if needed == 0 {
        return None;
    }
    let mut out = vec![0u16; needed as usize];
    // SAFETY: `out` holds the `needed` letters the call asked for.
    let written = unsafe { GetShortPathNameW(wide.as_ptr(), out.as_mut_ptr(), needed) };
    if written == 0 || written >= needed {
        return None;
    }
    out.truncate(written as usize);
    String::from_utf16(&out).ok()
}
