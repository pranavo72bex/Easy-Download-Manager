// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Windows' Remote Access Service: the dial-up and VPN connections Windows
//! knows, which are up, and dialling one (`RasDialW`, synchronous: no
//! notifier is given, so the call returns once the connection is up or has
//! failed).

use std::ptr::{null, null_mut};

use windows_sys::Win32::NetworkManagement::Rras::{
    RASCS_Connected, RasDialW, RasEnumConnectionsW, RasEnumEntriesW, RasGetConnectStatusW,
    RasGetEntryDialParamsW, RasGetErrorStringW, RasHangUpW, HRASCONN, RASCONNSTATUSW, RASCONNW,
    RASDIALPARAMSW, RASENTRYNAMEW,
};

/// `ERROR_BUFFER_TOO_SMALL`: the list is longer than the buffer offered.
const TOO_SMALL: u32 = 603;

fn text(wide: &[u16]) -> String {
    let end = wide.iter().position(|c| *c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(wide.get(..end).unwrap_or_default())
}

/// `into` filled with `from`, cut to fit and ended with NUL.
fn put(into: &mut [u16], from: &str) {
    let room = into.len().saturating_sub(1);
    for (slot, unit) in into
        .iter_mut()
        .zip(from.encode_utf16().take(room).chain(Some(0)))
    {
        *slot = unit;
    }
}

/// What a dial's error number says, in Windows' words: RAS's own list for its
/// numbers, the system's for the rest (a VPN's refused connection comes back as
/// `0x8007274D`, Windows' 10061 in an HRESULT, which RAS's list does not have).
fn said(error: u32) -> String {
    let mut words = [0u16; 512];
    // SAFETY: the buffer and its length (in characters) are this call's own.
    let got = unsafe { RasGetErrorStringW(error, words.as_mut_ptr(), words.len() as u32) };
    let ras = text(&words);
    if got == 0 && !ras.trim().is_empty() {
        return format!("{} (error {error})", ras.trim());
    }
    let code = if error & 0xFFFF_0000 == 0x8007_0000 {
        error & 0xFFFF
    } else {
        error
    };
    std::io::Error::from_raw_os_error(code as i32).to_string()
}

/// The names of the connections in the system's phone books.
pub(super) fn entries() -> Vec<String> {
    let mut size = std::mem::size_of::<RASENTRYNAMEW>() as u32;
    let mut count = 0u32;
    // SAFETY: a zeroed RASENTRYNAMEW is a valid empty value of this plain struct.
    let mut list: Vec<RASENTRYNAMEW> = vec![unsafe { std::mem::zeroed() }; 1];
    for _ in 0..2 {
        for entry in &mut list {
            entry.dwSize = std::mem::size_of::<RASENTRYNAMEW>() as u32;
        }
        // SAFETY: `list` holds `size` bytes of RASENTRYNAMEW, each with its size set;
        // the phone book and reserved arguments may be null.
        let error =
            unsafe { RasEnumEntriesW(null(), null(), list.as_mut_ptr(), &mut size, &mut count) };
        if error == TOO_SMALL {
            let wanted = (size as usize).div_ceil(std::mem::size_of::<RASENTRYNAMEW>());
            // SAFETY: as above.
            list = vec![unsafe { std::mem::zeroed() }; wanted.max(1)];
            continue;
        }
        if error != 0 {
            return Vec::new();
        }
        return list
            .iter()
            .take(count as usize)
            .map(|e| text(&e.szEntryName))
            .collect();
    }
    Vec::new()
}

/// The RAS connections Windows has open now, still dialling or up.
fn open() -> Vec<RASCONNW> {
    let mut size = std::mem::size_of::<RASCONNW>() as u32;
    let mut count = 0u32;
    // SAFETY: a zeroed RASCONNW is a valid empty value of this plain struct.
    let mut list: Vec<RASCONNW> = vec![unsafe { std::mem::zeroed() }; 1];
    for _ in 0..2 {
        for conn in &mut list {
            conn.dwSize = std::mem::size_of::<RASCONNW>() as u32;
        }
        // SAFETY: `list` holds `size` bytes of RASCONNW, each with its size set.
        let error = unsafe { RasEnumConnectionsW(list.as_mut_ptr(), &mut size, &mut count) };
        if error == TOO_SMALL {
            let wanted = (size as usize).div_ceil(std::mem::size_of::<RASCONNW>());
            // SAFETY: as above.
            list = vec![unsafe { std::mem::zeroed() }; wanted.max(1)];
            continue;
        }
        if error != 0 {
            return Vec::new();
        }
        list.truncate(count as usize);
        return list;
    }
    Vec::new()
}

/// Whether an open connection is up, not still dialling.
fn is_up(conn: &RASCONNW) -> bool {
    // SAFETY: a zeroed RASCONNSTATUSW is a valid empty value of this plain struct.
    let mut status: RASCONNSTATUSW = unsafe { std::mem::zeroed() };
    status.dwSize = std::mem::size_of::<RASCONNSTATUSW>() as u32;
    // SAFETY: the handle is one RasEnumConnectionsW just gave; `status` is sized.
    let error = unsafe { RasGetConnectStatusW(conn.hrasconn, &mut status) };
    error == 0 && status.rasconnstate == RASCS_Connected
}

/// Windows' own dialler.
pub(super) struct Ras;

impl super::Dialer for Ras {
    fn connected(&self, name: &str) -> bool {
        open()
            .iter()
            .any(|conn| text(&conn.szEntryName).eq_ignore_ascii_case(name) && is_up(conn))
    }

    fn dial(&self, name: &str, user: &str, password: &str) -> Result<(), String> {
        // SAFETY: a zeroed RASDIALPARAMSW is a valid empty value of this plain struct.
        let mut params: RASDIALPARAMSW = unsafe { std::mem::zeroed() };
        params.dwSize = std::mem::size_of::<RASDIALPARAMSW>() as u32;
        put(&mut params.szEntryName, name);
        if user.is_empty() && password.is_empty() {
            // The sign-in Windows keeps for the connection, where it keeps one; without one
            // the dial goes as it is, and Windows says what it lacks.
            let mut saved = 0;
            // SAFETY: `params` is sized and names the entry; the phone book may be null.
            unsafe { RasGetEntryDialParamsW(null(), &mut params, &mut saved) };
        } else {
            put(&mut params.szUserName, user);
            put(&mut params.szPassword, password);
        }
        let mut handle: HRASCONN = null_mut();
        // SAFETY: `params` is filled and sized; no notifier, so the call returns when the
        // connection is up or has failed; `handle` receives the connection's handle.
        let error = unsafe { RasDialW(null(), null(), &params, 0, null(), &mut handle) };
        // The password is not kept in memory longer than the call.
        put(&mut params.szPassword, "");
        if error == 0 {
            return Ok(());
        }
        // A failed dial still holds what it opened until it is hung up.
        if !handle.is_null() {
            // SAFETY: `handle` is the one RasDialW returned.
            unsafe { RasHangUpW(handle) };
        }
        Err(said(error))
    }
}
