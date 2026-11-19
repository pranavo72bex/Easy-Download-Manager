// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The manifest a browser reads, the extensions it allows, and the places macOS will not let
//! a browser start dm from.

use std::path::Path;

use dm_protocol::{CHROME_EXTENSION_ID, EDGE_EXTENSION_ID, FIREFOX_EXTENSION_ID, NATIVE_HOST};

use super::Family;
use crate::Error;

/// Which extensions may talk to dm: the two built from this repository, the
/// Edge Add-ons listing's, and any named — a Chrome Web Store id, or a
/// signed Firefox add-on's.
#[derive(Debug, Clone, PartialEq)]
pub struct Allowed {
    /// The `chrome-extension://…/` origins Chromium browsers may connect from.
    pub chromium: Vec<String>,
    /// The add-on ids Firefox may connect from.
    pub firefox: Vec<String>,
}

impl Allowed {
    /// The extensions built here and the Edge listing's, and those `named`:
    /// a 32-letter Chrome Web Store id or a Firefox add-on id (`name@host` or
    /// a `{uuid}`). Anything else is refused in words.
    pub fn with(named: &[String]) -> Result<Self, Error> {
        let mut allowed = Allowed {
            chromium: vec![
                CHROME_EXTENSION_ID.to_string(),
                EDGE_EXTENSION_ID.to_string(),
            ],
            firefox: vec![FIREFOX_EXTENSION_ID.to_string()],
        };
        for id in named {
            let id = id.trim();
            let list = if id.contains('@') || (id.starts_with('{') && id.ends_with('}')) {
                // Firefox ids are an address or a braced UUID.
                &mut allowed.firefox
            } else if id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b)) {
                // Chrome's are 32 letters from a to p, and nothing else.
                &mut allowed.chromium
            } else {
                return Err(format!(
                    "{id:?} is not an extension id. Chrome's is 32 letters from a to p, shown \
                     on chrome://extensions with Developer mode on; Firefox's looks like an \
                     address."
                )
                .into());
            };
            if !list.iter().any(|known| known == id) {
                list.push(id.to_string());
            }
        }
        Ok(allowed)
    }
}

/// The manifest one family reads.
pub fn manifest(family: Family, exe: &Path, allowed: &Allowed) -> serde_json::Value {
    let mut manifest = serde_json::json!({
        "name": NATIVE_HOST,
        "description": "dm download manager",
        "path": exe,
        "type": "stdio",
    });
    let (key, value): (&str, serde_json::Value) = match family {
        Family::Chromium => (
            "allowed_origins",
            allowed
                .chromium
                .iter()
                .map(|id| format!("chrome-extension://{id}/"))
                .collect(),
        ),
        Family::Firefox => ("allowed_extensions", allowed.firefox.clone().into()),
    };
    if let Some(fields) = manifest.as_object_mut() {
        fields.insert(key.to_string(), value);
    }
    manifest
}

/// Whether a browser on macOS will be refused when it runs `exe`.
///
/// Downloads, Desktop and Documents are behind a folder permission browsers
/// do not have. The failure is silent and looks exactly like a bug in dm: the
/// host hangs in the loader opening its own executable while the system
/// waits on a prompt nobody sees, and the extension reports that dm did not
/// answer. Found by the Chrome hand-off check, with this repository under
/// ~/Downloads.
pub fn protected(exe: &Path, home: &Path, macos: bool) -> bool {
    macos
        && ["Downloads", "Desktop", "Documents"]
            .iter()
            .any(|folder| exe.starts_with(home.join(folder)))
}

/// Whether macOS is running `exe` from a place that will not be there next
/// time: a mounted disk image, or AppTranslocation — the randomised
/// read-only copy macOS runs a quarantined app from when it was opened
/// where it was downloaded rather than moved to Applications. A browser
/// manifest or login item naming either starts nothing later.
/// A disk image is told by being mounted read-only under `/Volumes`
/// (`read_only` says whether the mount holding a path is). An external
/// disk is mounted there too, writable, and an install on it is a place
/// like any other — refusing it left that install with no browser
/// registration and no login item.
pub fn transient_on(exe: &Path, macos: bool, read_only: impl FnOnce(&Path) -> bool) -> bool {
    macos
        && (exe.to_string_lossy().contains("/AppTranslocation/")
            || (exe.starts_with("/Volumes") && read_only(exe)))
}

/// [`transient_on`] for the system this runs on, the mount's flags read.
pub fn transient(exe: &Path) -> bool {
    transient_on(exe, cfg!(target_os = "macos"), mounted_read_only)
}

/// Whether the file system holding `path` is mounted read-only. Unreadable
/// is taken as writable: the answer that refuses nothing.
pub fn mounted_read_only(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            return false;
        };
        let mut stat = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
        // SAFETY: `c` is a valid NUL-terminated string and `stat` is
        // writable memory of the right size, filled only on success.
        let done = unsafe { libc::statvfs(c.as_ptr(), stat.as_mut_ptr()) };
        if done != 0 {
            return false;
        }
        // SAFETY: statvfs returned 0, so it filled `stat`.
        let stat = unsafe { stat.assume_init() };
        (stat.f_flag & libc::ST_RDONLY) != 0
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}
