// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Secrets overwritten before their memory is let go (gaps row H15): a sign-in's cookie and
//! header values, a row's own copy of them. Freed memory is handed to the next allocation as it
//! is, so a cookie dropped plainly stayed readable in the process (a crash report, a swap file,
//! a debugger) until something happened to write over it. dm's own wipe, not a crate's (the
//! clean-room rule; the maintainer's standing word, 2026-11-27): a volatile write of zero to
//! each byte, which the compiler may not drop as a write nobody reads, then a fence so no later
//! work is moved before it. What a library copied on its way (reqwest's own header values, the
//! kernel's socket buffers) is out of its reach.

use std::sync::atomic::{compiler_fence, Ordering};

/// Writes zero over every byte of `bytes`.
pub fn bytes(bytes: &mut [u8]) {
    for byte in bytes.iter_mut() {
        // SAFETY: `byte` is a valid, aligned, exclusive reference to one `u8` of the slice.
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
    compiler_fence(Ordering::SeqCst);
}

/// Writes zero over `text`'s bytes, its spare room included (a value shortened in place left
/// its old tail there), and leaves it empty, its room kept.
pub fn string(text: &mut String) {
    // SAFETY: only zero bytes are written, which are valid UTF-8, and the vector is emptied
    // before the `String` is used again; growing to its capacity does not reallocate.
    let buffer = unsafe { text.as_mut_vec() };
    buffer.resize(buffer.capacity(), 0);
    self::bytes(buffer);
    buffer.clear();
}

/// Writes zero over `text`'s bytes where nothing else holds them: zero bytes are text too, so
/// the `str` stays valid.
pub fn str(text: &mut str) {
    // SAFETY: zero bytes are valid UTF-8, so the `str` is valid text after the write.
    self::bytes(unsafe { text.as_bytes_mut() });
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_wiped_string_is_empty_and_its_whole_room_is_zero() {
        let mut secret = String::with_capacity(32);
        secret.push_str("session=abc123, and more");
        // Shortened in place: the old tail stays in its room.
        secret.truncate(14);
        super::string(&mut secret);
        assert!(secret.is_empty());
        let room = secret.capacity();
        assert!(room >= 32);
        // SAFETY: the room is the string's own allocation, and every byte of it was written
        // (zero) by the wipe, so it may be read as initialised for this check.
        let bytes = unsafe { std::slice::from_raw_parts(secret.as_ptr(), room) };
        assert!(bytes.iter().all(|&b| b == 0), "{bytes:?}");
    }

    #[test]
    fn a_wiped_str_is_zeros_of_its_length() {
        let mut text: Box<str> = "token=xyz".into();
        super::str(&mut text);
        assert_eq!(text.len(), 9);
        assert!(text.bytes().all(|b| b == 0));
    }
}
