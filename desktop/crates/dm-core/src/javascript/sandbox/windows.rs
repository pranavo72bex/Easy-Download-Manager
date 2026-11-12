// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Windows: the process lowers its own token to low integrity, as a browser's renderer runs,
//! so nothing at the person's own level (their files, the registry's keys under their name,
//! their other programs) can be written or opened to change; and it puts itself in a job that
//! allows one process, itself, so it can start no other.

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW;
use windows_sys::Win32::Security::{
    GetLengthSid, SetTokenInformation, TokenIntegrityLevel, PSID, SID_AND_ATTRIBUTES,
    TOKEN_ADJUST_DEFAULT, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// `SE_GROUP_INTEGRITY`: the group is the token's integrity level.
const SE_GROUP_INTEGRITY: u32 = 0x20;

pub(super) fn enter() -> Result<(), String> {
    alone()?;
    low()
}

/// Into a job of one process: no program can be started from here.
fn alone() -> Result<(), String> {
    // SAFETY: documented calls; the job's handle is kept open for the life of the process (a
    // job closed with no handle left would end its limits with it), and the limits are a plain
    // structure of the size given.
    unsafe {
        let job: HANDLE = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return Err(format!(
                "Windows made no job for the JavaScript host: {}",
                std::io::Error::last_os_error()
            ));
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
        limits.BasicLimitInformation.ActiveProcessLimit = 1;
        let set = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            std::ptr::addr_of!(limits).cast(),
            u32::try_from(std::mem::size_of_val(&limits)).unwrap_or(u32::MAX),
        );
        if set == 0 || AssignProcessToJobObject(job, GetCurrentProcess()) == 0 {
            let why = std::io::Error::last_os_error();
            CloseHandle(job);
            return Err(format!(
                "Windows would not hold the JavaScript host alone: {why}"
            ));
        }
    }
    Ok(())
}

/// The process's own token at low integrity (`S-1-16-4096`).
fn low() -> Result<(), String> {
    let low: Vec<u16> = "S-1-16-4096\0".encode_utf16().collect();
    // SAFETY: documented calls; the SID `ConvertStringSidToSidW` makes is the system's to free
    // with `LocalFree`, kept here until the process ends with it; the token's handle is closed.
    unsafe {
        let mut sid: PSID = std::ptr::null_mut();
        if ConvertStringSidToSidW(low.as_ptr(), &mut sid) == 0 {
            return Err(format!(
                "Windows has no low integrity level to give: {}",
                std::io::Error::last_os_error()
            ));
        }
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_DEFAULT | TOKEN_QUERY,
            &mut token,
        ) == 0
        {
            return Err(format!(
                "Windows would not open the JavaScript host's token: {}",
                std::io::Error::last_os_error()
            ));
        }
        let label = TOKEN_MANDATORY_LABEL {
            Label: SID_AND_ATTRIBUTES {
                Sid: sid,
                Attributes: SE_GROUP_INTEGRITY,
            },
        };
        let size = std::mem::size_of::<TOKEN_MANDATORY_LABEL>() as u32 + GetLengthSid(sid);
        let set = SetTokenInformation(
            token,
            TokenIntegrityLevel,
            std::ptr::addr_of!(label).cast(),
            size,
        );
        let why = std::io::Error::last_os_error();
        CloseHandle(token);
        if set == 0 {
            return Err(format!(
                "Windows would not lower the JavaScript host's rights: {why}"
            ));
        }
    }
    Ok(())
}
