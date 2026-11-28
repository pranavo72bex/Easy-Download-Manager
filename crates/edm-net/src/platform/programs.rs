// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The system's own programs edm-net runs, each behind one function (gaps row P8): macOS's
//! `scutil` for the proxy settings and `networksetup` for the interfaces' names. Why each is a
//! program and not a call is in `docs/how-it-works.md` ("The system's programs").

/// What `scutil --proxy` printed (the network settings' proxy dictionary), where it ran and
/// succeeded; `None` otherwise, and on every system but macOS.
pub(crate) fn proxy_settings() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/sbin/scutil")
            .arg("--proxy")
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// What `networksetup -listallhardwareports` printed (each hardware port and its device),
/// however it ended; `None` where it could not be run.
#[cfg(target_os = "macos")]
pub(crate) fn hardware_ports() -> Option<String> {
    std::process::Command::new("/usr/sbin/networksetup")
        .arg("-listallhardwareports")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
}
