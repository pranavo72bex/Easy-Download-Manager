// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A phone's rules (gaps row N10), on W11's holds: what the phone says of its connection and
//! battery ([`PhoneNetwork`], read by `dm-android` through JNI) is the computer's state on a
//! phone, so mobile data is W11's metered connection and a low battery W11's battery, each
//! as Options › Downloads says. The phone's own lines (Wi-Fi only, not while roaming, wait
//! while Data Saver is on) hold every download the same way, each with its own reason. An
//! unknown holds nothing, as in W11. Built on every system, so the host's tests reach it; a
//! computer's phone says nothing, and its three lines are never shown or on.

use dm_core::Hold;
use dm_shell::settings::DownloadSettings;

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
mod tests {
    use super::*;
    use dm_shell::settings::{METERED_ASK, METERED_GO, METERED_WAIT};

    /// A phone on mobile data at home, with a full battery off its charger.
    fn mobile() -> PhoneNetwork {
        PhoneNetwork {
            metered: Some(true),
            roaming: Some(false),
            wifi: Some(false),
            data_saver: Some(false),
            battery_percent: Some(90),
            charging: Some(false),
        }
    }

    /// A phone on Wi-Fi at home, charging.
    fn home_wifi() -> PhoneNetwork {
        PhoneNetwork {
            metered: Some(false),
            roaming: Some(false),
            wifi: Some(true),
            data_saver: Some(false),
            battery_percent: Some(10),
            charging: Some(true),
        }
    }

    fn options(change: impl FnOnce(&mut DownloadSettings)) -> DownloadSettings {
        let mut downloads = DownloadSettings::default();
        change(&mut downloads);
        downloads
    }

    /// Mobile data is W11's metered connection: go on, wait, or ask, as Options says.
    #[test]
    fn mobile_data_is_the_metered_hold() {
        let go = options(|d| d.on_metered = METERED_GO.into());
        assert_eq!(hold_of(&mobile(), &go, None), None);
        let wait = options(|d| d.on_metered = METERED_WAIT.into());
        assert_eq!(hold_of(&mobile(), &wait, None), Some(Hold::Metered));
        let ask = options(|d| d.on_metered = METERED_ASK.into());
        assert_eq!(hold_of(&mobile(), &ask, None), Some(Hold::Metered));
        assert_eq!(hold_of(&mobile(), &ask, Some(true)), None);
        assert_eq!(hold_of(&home_wifi(), &wait, None), None);
    }

    /// A low battery is W11's battery hold, and only off the charger.
    #[test]
    fn a_low_battery_is_the_battery_hold() {
        let below = options(|d| d.battery_below = 20);
        let low = PhoneNetwork {
            battery_percent: Some(12),
            ..mobile()
        };
        assert_eq!(hold_of(&low, &below, None), Some(Hold::Battery));
        // Charging at 10 %: it goes on.
        assert_eq!(hold_of(&home_wifi(), &below, None), None);
        // The battery holds first, before every other line.
        let all = options(|d| {
            d.battery_below = 20;
            d.on_metered = METERED_WAIT.into();
            d.wifi_only = true;
        });
        assert_eq!(hold_of(&low, &all, None), Some(Hold::Battery));
    }

    /// Wi-Fi only holds off Wi-Fi, and lets a cable or Wi-Fi go on.
    #[test]
    fn wifi_only_holds_on_mobile_data() {
        let wifi_only = options(|d| d.wifi_only = true);
        assert_eq!(hold_of(&mobile(), &wifi_only, None), Some(Hold::NotWifi));
        assert_eq!(hold_of(&home_wifi(), &wifi_only, None), None);
        // The person said to go on over the metered connection; Wi-Fi only still holds.
        let both = options(|d| {
            d.wifi_only = true;
            d.on_metered = METERED_ASK.into();
        });
        assert_eq!(hold_of(&mobile(), &both, Some(true)), Some(Hold::NotWifi));
    }

    /// Not while roaming and Data Saver, each with its own reason, roaming said first.
    #[test]
    fn roaming_and_data_saver_hold_with_their_reasons() {
        let abroad = PhoneNetwork {
            roaming: Some(true),
            data_saver: Some(true),
            ..mobile()
        };
        let roaming = options(|d| d.not_roaming = true);
        assert_eq!(hold_of(&abroad, &roaming, None), Some(Hold::Roaming));
        let saver = options(|d| d.data_saver_waits = true);
        assert_eq!(hold_of(&abroad, &saver, None), Some(Hold::DataSaver));
        let every = options(|d| {
            d.not_roaming = true;
            d.data_saver_waits = true;
            d.wifi_only = true;
        });
        assert_eq!(hold_of(&abroad, &every, None), Some(Hold::Roaming));
        // The lines off: roaming and Data Saver hold nothing.
        assert_eq!(hold_of(&abroad, &DownloadSettings::default(), None), None);
    }

    /// What the phone does not say holds nothing, whatever Options says; a computer says
    /// nothing, and its settings leave the three lines off.
    #[test]
    fn an_unknown_holds_nothing() {
        let every = options(|d| {
            d.not_roaming = true;
            d.data_saver_waits = true;
            d.wifi_only = true;
            d.on_metered = METERED_WAIT.into();
            d.battery_below = 50;
        });
        assert_eq!(hold_of(&PhoneNetwork::default(), &every, None), None);
        assert_eq!(reading_of(&PhoneNetwork::default()), Reading::default());
        assert!(watched(&every));
        assert!(!watched(&DownloadSettings::default()));
    }
}
