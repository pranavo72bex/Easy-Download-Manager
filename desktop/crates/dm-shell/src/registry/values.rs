// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A value read from the registry through its own API: the login item's Run
//! key, the accent colour, Windows' build and the person's regional formats
//! (dm-desktop-base's `autostart::registry`, dm-view's `region`).

/// A registry string as the registry API hands it over: UTF-16, little
/// endian, usually ending in a NUL. Read this way rather than from
/// `reg query`'s text, which comes in the console's code page and garbled
/// a user name outside it.
pub fn utf16_value(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    let end = units.iter().position(|u| *u == 0).unwrap_or(units.len());
    String::from_utf16_lossy(units.get(..end).unwrap_or_default())
}

#[cfg(windows)]
use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
#[cfg(windows)]
use windows_sys::Win32::System::Registry::{
    RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_ROUTINE_FLAGS,
    RRF_RT_REG_BINARY, RRF_RT_REG_SZ,
};

#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// `name` under `root`\`subkey`, of the type `flags` allows. `None`
/// when it is not there or cannot be read.
#[cfg(windows)]
fn read(root: HKEY, subkey: &str, name: &str, flags: REG_ROUTINE_FLAGS) -> Option<Vec<u8>> {
    let (subkey, name) = (wide(subkey), wide(name));
    let mut size: u32 = 0;
    // A value that grows between asking its size and reading it is
    // asked about again, a few times.
    for _ in 0..4 {
        let mut buffer = vec![0u8; size as usize];
        let data = if buffer.is_empty() {
            std::ptr::null_mut()
        } else {
            buffer.as_mut_ptr().cast()
        };
        // SAFETY: both names are NUL-terminated and outlive the call;
        // `data` is null (asking only the size) or `size` bytes we own.
        let status = unsafe {
            RegGetValueW(
                root,
                subkey.as_ptr(),
                name.as_ptr(),
                flags,
                std::ptr::null_mut(),
                data,
                &mut size,
            )
        };
        match status {
            ERROR_SUCCESS if !buffer.is_empty() || size == 0 => {
                buffer.truncate(size as usize);
                return Some(buffer);
            }
            ERROR_SUCCESS | ERROR_MORE_DATA => continue,
            _ => return None,
        }
    }
    None
}

/// A string value, decoded from UTF-16.
#[cfg(windows)]
pub fn string(subkey: &str, name: &str) -> Option<String> {
    read(HKEY_CURRENT_USER, subkey, name, RRF_RT_REG_SZ).map(|bytes| utf16_value(&bytes))
}

/// A string value of the computer's own (HKLM), decoded from UTF-16.
#[cfg(windows)]
pub fn machine_string(subkey: &str, name: &str) -> Option<String> {
    read(HKEY_LOCAL_MACHINE, subkey, name, RRF_RT_REG_SZ).map(|bytes| utf16_value(&bytes))
}

/// A binary value's bytes.
#[cfg(windows)]
pub fn binary(subkey: &str, name: &str) -> Option<Vec<u8>> {
    read(HKEY_CURRENT_USER, subkey, name, RRF_RT_REG_BINARY)
}
