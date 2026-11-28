// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A phone's rules on W11's holds: mobile data, a low battery, Wi-Fi only, roaming and Data
//! Saver, each as Options says, and an unknown holding nothing.

use super::*;
use edm_shell::settings::{METERED_ASK, METERED_GO, METERED_WAIT};

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
