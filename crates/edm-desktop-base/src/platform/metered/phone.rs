// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A phone's rules (gaps row N10), on W11's holds: what the phone says of its connection and
//! battery ([`PhoneNetwork`], read by `edm-android` through JNI) is the computer's state on a
//! phone, so mobile data is W11's metered connection and a low battery W11's battery, each
//! as Options › Downloads says. The phone's own lines (Wi-Fi only, not while roaming, wait
//! while Data Saver is on) hold every download the same way, each with its own reason. An
//! unknown holds nothing, as in W11. Built on every system, so the host's tests reach it; a
//! computer's phone says nothing, and its three lines are never shown or on.

use edm_core::Hold;
use edm_shell::settings::DownloadSettings;

use super::{Battery, Reading};
use crate::phone::PhoneNetwork;

/// W11's reading made of the phone's: metered as the phone says, and its battery, on it
/// where the phone is off its charger.
#[must_use]
pub fn reading_of(phone: &PhoneNetwork) -> Reading {
    let battery =
        (phone.battery_percent.is_some() || phone.charging.is_some()).then_some(Battery {
            discharging: phone.charging == Some(false),
            percent: phone.battery_percent,
        });
    Reading {
        metered: phone.metered,
        battery,
    }
}

/// What the phone's own lines make of `phone`: roaming first, then Data Saver, then a
/// connection that is not Wi-Fi or a cable; `None` while none holds.
#[must_use]
pub fn rules(phone: &PhoneNetwork, downloads: &DownloadSettings) -> Option<Hold> {
    if downloads.not_roaming && phone.roaming == Some(true) {
        return Some(Hold::Roaming);
    }
    if downloads.data_saver_waits && phone.data_saver == Some(true) {
        return Some(Hold::DataSaver);
    }
    if downloads.wifi_only && phone.wifi == Some(false) {
        return Some(Hold::NotWifi);
    }
    None
}

/// Everything that holds a phone's downloads: W11's battery and metered connection first, as
/// on a computer (`answer` is the person's to "ask"), then the phone's own lines.
#[must_use]
pub fn hold_of(
    phone: &PhoneNetwork,
    downloads: &DownloadSettings,
    answer: Option<bool>,
) -> Option<Hold> {
    super::hold_of(
        &reading_of(phone),
        &downloads.on_metered,
        downloads.battery_below,
        answer,
    )
    .or_else(|| rules(phone, downloads))
}

/// Whether one of the phone's own lines is on, so the phone is read.
#[must_use]
pub fn watched(downloads: &DownloadSettings) -> bool {
    downloads.wifi_only || downloads.not_roaming || downloads.data_saver_waits
}

/// Whether the connection is metered, as the phone says.
#[cfg(target_os = "android")]
pub(super) fn metered_now() -> Option<bool> {
    crate::phone::network().metered
}

/// The battery, as the phone says.
#[cfg(target_os = "android")]
pub(super) fn battery_now() -> Option<Battery> {
    reading_of(&crate::phone::network()).battery
}

#[cfg(test)]
mod tests;
