// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Whether the connection is metered and whether the computer runs on its battery (gaps row
//! W11), read by direct calls to each system, and what Options › Downloads makes of them: go
//! on, or every download waits ([`edm_core::Hold`]).
//!
//! Each system's reading is one function behind [`Source`], so the tests stand a fake in for
//! the computer. What a system does not say is unknown, and an unknown never holds a download:
//!
//! * Windows: the connection's cost from `NetworkInformation`'s Internet profile (metered where
//!   its cost is fixed or variable, roaming, or over its data limit), and the battery from
//!   `GetSystemPowerStatus`.
//! * macOS: the battery from IOKit's power sources (`IOPSCopyPowerSourcesInfo`). Whether the
//!   connection is metered is said only by the Network framework's path monitor, which answers
//!   through a block: unknown here.
//! * Linux: NetworkManager's `Metered` over the system bus, and the battery from
//!   `/sys/class/power_supply`.

use edm_core::Hold;

#[cfg(target_os = "linux")]
mod on_linux;
#[cfg(target_os = "macos")]
mod on_macos;
#[cfg(windows)]
mod on_windows;
pub mod phone;

/// The computer's battery, where it has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Battery {
    /// Running on it: no charger, or one that does not keep up.
    pub discharging: bool,
    /// How full it is, 0 to 100; `None` where the system does not say.
    pub percent: Option<u8>,
}

/// What the system says now; `None` where it does not say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Reading {
    /// Whether the connection to the Internet is metered.
    pub metered: Option<bool>,
    /// The battery; `None` for a computer with none, or a system that does not say.
    pub battery: Option<Battery>,
}

/// Where the state is read from: the computer ([`System`]), or a test's fake.
pub trait Source: Send {
    /// What the state is now. Called every few seconds, and only while Options asks for it.
    fn read(&self) -> Reading;

    /// What a phone says of its connection and battery, for its own rules (gaps row N10);
    /// nothing, by default.
    fn phone(&self) -> crate::phone::PhoneNetwork {
        crate::phone::PhoneNetwork::default()
    }
}

/// The computer EDM runs on, read by its own calls.
#[derive(Debug, Clone, Copy, Default)]
pub struct System;

impl Source for System {
    fn read(&self) -> Reading {
        if let Some(stood_in) = stand_in() {
            return stood_in;
        }
        Reading {
            metered: metered_now(),
            battery: battery_now(),
        }
    }

    fn phone(&self) -> crate::phone::PhoneNetwork {
        crate::phone::network()
    }
}

/// A debug build's stand-in for the system, for a check's run (`scripts/human-qa.py`, contract
/// row B76): the file `EDM_STATE_FILE` names, read each time, as [`stand_in_of`] reads it. None
/// in a release build, or with the switch unset.
fn stand_in() -> Option<Reading> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let path = edm_core::env::get_os("EDM_STATE_FILE")?;
    Some(stand_in_of(
        &std::fs::read_to_string(path).unwrap_or_default(),
    ))
}

/// The state a stand-in file says: `metered` on a line of its own for a metered connection,
/// `battery 12` for a computer off its charger at 12 %. A connection it does not call metered
/// is not; a file that names no battery is a computer with none.
pub fn stand_in_of(text: &str) -> Reading {
    let mut reading = Reading {
        metered: Some(false),
        battery: None,
    };
    for line in text.lines() {
        match line.split_whitespace().collect::<Vec<_>>().as_slice() {
            ["metered"] => reading.metered = Some(true),
            ["battery", percent] => {
                reading.battery = Some(Battery {
                    discharging: true,
                    percent: percent.parse().ok(),
                });
            }
            _ => {}
        }
    }
    reading
}

/// Whether the connection is metered, as Windows says.
#[cfg(windows)]
fn metered_now() -> Option<bool> {
    on_windows::metered()
}

/// Whether the connection is metered, as NetworkManager says.
#[cfg(target_os = "linux")]
fn metered_now() -> Option<bool> {
    on_linux::metered()
}

/// Whether the connection is metered, as the phone says (gaps row N10).
#[cfg(target_os = "android")]
fn metered_now() -> Option<bool> {
    phone::metered_now()
}

/// Nothing here says whether the connection is metered.
#[cfg(not(any(windows, target_os = "linux", target_os = "android")))]
fn metered_now() -> Option<bool> {
    None
}

/// The battery, as Windows says.
#[cfg(windows)]
fn battery_now() -> Option<Battery> {
    on_windows::battery()
}

/// The battery, as the kernel's power supplies say.
#[cfg(target_os = "linux")]
fn battery_now() -> Option<Battery> {
    on_linux::battery(std::path::Path::new("/sys/class/power_supply"))
}

/// The battery, as IOKit's power sources say.
#[cfg(target_os = "macos")]
fn battery_now() -> Option<Battery> {
    on_macos::battery()
}

/// The battery, as the phone says (gaps row N10).
#[cfg(target_os = "android")]
fn battery_now() -> Option<Battery> {
    phone::battery_now()
}

/// Nothing here says what the battery is.
#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "macos",
    target_os = "android"
)))]
fn battery_now() -> Option<Battery> {
    None
}

/// What Options › Downloads makes of `reading`: `on_metered` (`go`, `ask` or `wait`) and
/// `battery_below` (a percent, 0 for never). `answer` is the person's to this metered period's
/// question, where "ask" asked it: `Some(true)` goes on, anything else waits. A low battery
/// holds first; an unknown connection or charge holds nothing.
pub fn hold_of(
    reading: &Reading,
    on_metered: &str,
    battery_below: u32,
    answer: Option<bool>,
) -> Option<Hold> {
    let low = reading.battery.is_some_and(|battery| {
        battery.discharging
            && battery
                .percent
                .is_some_and(|p| u32::from(p) < battery_below)
    });
    if low {
        return Some(Hold::Battery);
    }
    if reading.metered != Some(true) {
        return None;
    }
    match on_metered {
        edm_shell::settings::METERED_WAIT => Some(Hold::Metered),
        edm_shell::settings::METERED_ASK if answer != Some(true) => Some(Hold::Metered),
        _ => None,
    }
}

/// Whether Options asks for the state at all: something other than going on over a metered
/// connection, or a battery level to wait below. Nothing is read otherwise.
pub fn watched(on_metered: &str, battery_below: u32) -> bool {
    battery_below > 0
        || on_metered == edm_shell::settings::METERED_ASK
        || on_metered == edm_shell::settings::METERED_WAIT
}

#[cfg(test)]
mod tests;
