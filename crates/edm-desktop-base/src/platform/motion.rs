// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Whether the person asked their system for less motion (gaps row K7): macOS's Reduce motion
//! (Accessibility › Display), Windows' "Show animations in Windows" turned off, GNOME's
//! animations turned off, Android's Remove animations. EDM's window then moves nothing that is
//! not needed, asked again each time the window comes in front, as the setting can change while
//! EDM runs.

/// Whether the system asks for reduced motion now.
pub fn reduced() -> bool {
    ask()
}

#[cfg(target_os = "macos")]
fn ask() -> bool {
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send};
    // SAFETY: plain AppKit messages to the shared workspace, which lives for the process.
    unsafe {
        let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        let Some(workspace) = workspace.as_ref() else {
            return false;
        };
        let reduced: Bool = msg_send![workspace, accessibilityDisplayShouldReduceMotion];
        reduced.as_bool()
    }
}

#[cfg(windows)]
fn ask() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION,
    };
    let mut on: i32 = 1;
    // SAFETY: `on` is a local BOOL the call writes, as SPI_GETCLIENTAREAANIMATION asks.
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut on as *mut i32).cast(),
            0,
        )
    };
    read != 0 && on == 0
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
fn ask() -> bool {
    std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "enable-animations"])
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "false")
}

/// The phone's Remove animations, asked of its Java side (`phone::PhoneServices`); none before
/// the phone's services are set.
#[cfg(target_os = "android")]
fn ask() -> bool {
    crate::platform::phone::phone().is_some_and(|phone| phone.reduced_motion())
}
