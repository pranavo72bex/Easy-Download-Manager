// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's home made this Windows account's alone, as 0700 makes it on Unix.
//!
//! A home inside the account's profile (`%LOCALAPPDATA%\dm`, the default) takes
//! the profile's access list: this account, SYSTEM and the administrators. A
//! home anywhere else (`DM_HOME`, a portable copy on a second drive) took that
//! drive's, which on a second drive lets every signed-in account change it and
//! read it (`Authenticated Users: Modify`, `Users: Read`, measured on this
//! computer's D: on 2026-11-07): the control port's token, the queue and the
//! parts of downloads in progress with it. The folder is given a protected access
//! list (nothing taken from its parent) with full control for this account,
//! SYSTEM and the administrators, inherited by everything in it. A home whose
//! list is protected already is left alone, so a start costs one read; a drive
//! with no access lists (FAT) answers an error, which is not a reason to stop dm.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, ERROR_SUCCESS, HANDLE};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    GetNamedSecurityInfoW, SetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    GetSecurityDescriptorControl, GetSecurityDescriptorDacl, GetTokenInformation, TokenUser, ACL,
    DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    SE_DACL_PROTECTED, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}

/// This process's account as a SID string (`S-1-5-21-…`).
fn account() -> std::io::Result<String> {
    let mut token: HANDLE = null_mut();
    // SAFETY: GetCurrentProcess is a pseudo-handle that needs no closing; `token`
    // is written only when the call succeeds.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut needed = 0u32;
    // SAFETY: a size question: no buffer and a length of zero; `token` is open.
    unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &mut needed) };
    // Eight-byte words, so the TOKEN_USER at its start is aligned as it must be.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8).max(1)];
    // SAFETY: the buffer holds at least `needed` bytes and is 8-aligned; `token` is open.
    let got = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    };
    let failed = std::io::Error::last_os_error();
    // SAFETY: `token` was opened above and is closed once, here.
    unsafe { CloseHandle(token) };
    if got == 0 {
        return Err(failed);
    }
    // SAFETY: on success the buffer begins with a TOKEN_USER whose SID points into it.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut text: *mut u16 = null_mut();
    // SAFETY: `sid` is valid while `buffer` lives; `text` receives a LocalAlloc'd string.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `text` is the NUL-terminated wide string the call just made.
    let said = unsafe {
        let mut length = 0;
        while *text.add(length) != 0 {
            length += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(text, length))
    };
    // SAFETY: allocated by ConvertSidToStringSidW with LocalAlloc, freed once.
    unsafe { LocalFree(text.cast()) };
    Ok(said)
}

/// Whether the folder's access list is protected (takes nothing from its parent).
fn protected(path: &[u16]) -> std::io::Result<bool> {
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: `path` ends in NUL; only the access list is asked for, and the
    // descriptor it is returned in is freed below.
    let error = unsafe {
        GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    };
    if error != ERROR_SUCCESS {
        return Err(std::io::Error::from_raw_os_error(error as i32));
    }
    let (mut control, mut revision) = (0u16, 0u32);
    // SAFETY: `descriptor` was filled in above.
    let read = unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) };
    // SAFETY: allocated by GetNamedSecurityInfoW, freed once.
    unsafe { LocalFree(descriptor) };
    Ok(read != 0 && control & SE_DACL_PROTECTED != 0)
}

/// Gives `home` an access list of its own: this account, SYSTEM and the
/// administrators, inherited by everything in it (and by what is there already).
pub(super) fn make(home: &Path) -> std::io::Result<()> {
    let path = wide(home.as_os_str());
    if protected(&path)? {
        return Ok(());
    }
    let me = account()?;
    // P: nothing from the parent. OICI: files and folders inside take it too.
    let sddl = wide(OsStr::new(&format!(
        "D:P(A;OICI;FA;;;{me})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"
    )));
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: `sddl` ends in NUL; `descriptor` receives a LocalAlloc'd descriptor.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    let (mut present, mut defaulted) = (0, 0);
    let mut list: *mut ACL = null_mut();
    // SAFETY: `descriptor` is valid; `list` is set to point inside it.
    let found =
        unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut list, &mut defaulted) };
    let result = if found == 0 || present == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        // SAFETY: `path` ends in NUL; `list` lives in `descriptor`, freed only after.
        let error = unsafe {
            SetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                list,
                null(),
            )
        };
        if error == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(std::io::Error::from_raw_os_error(error as i32))
        }
    };
    // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW, freed once.
    unsafe { LocalFree(descriptor) };
    result
}
