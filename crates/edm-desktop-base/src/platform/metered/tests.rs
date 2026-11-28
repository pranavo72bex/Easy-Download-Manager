// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What Options › Downloads makes of the connection and the battery (gaps row W11).

use super::*;
use edm_shell::settings::{METERED_ASK, METERED_GO, METERED_WAIT};

fn metered() -> Reading {
    Reading {
        metered: Some(true),
        battery: None,
    }
}

fn on_battery(percent: Option<u8>) -> Reading {
    Reading {
        metered: Some(false),
        battery: Some(Battery {
            discharging: true,
            percent,
        }),
    }
}

/// On a metered connection: go on, wait, or ask, which waits until the person says to go on.
#[test]
fn a_metered_connection_holds_as_options_says() {
    assert_eq!(hold_of(&metered(), METERED_GO, 0, None), None);
    assert_eq!(
        hold_of(&metered(), METERED_WAIT, 0, None),
        Some(Hold::Metered)
    );
    assert_eq!(
        hold_of(&metered(), METERED_ASK, 0, None),
        Some(Hold::Metered)
    );
    assert_eq!(
        hold_of(&metered(), METERED_ASK, 0, Some(false)),
        Some(Hold::Metered)
    );
    assert_eq!(hold_of(&metered(), METERED_ASK, 0, Some(true)), None);
    // A word `settings.json` has that is none of the three goes on.
    assert_eq!(hold_of(&metered(), "sometimes", 0, None), None);
}

/// Below the level on the battery, downloads wait; at it, above it, or charging, they go on.
#[test]
fn a_low_battery_holds_below_the_level_alone() {
    assert_eq!(
        hold_of(&on_battery(Some(15)), METERED_GO, 20, None),
        Some(Hold::Battery)
    );
    assert_eq!(hold_of(&on_battery(Some(20)), METERED_GO, 20, None), None);
    assert_eq!(hold_of(&on_battery(Some(15)), METERED_GO, 0, None), None);
    let charging = Reading {
        metered: None,
        battery: Some(Battery {
            discharging: false,
            percent: Some(5),
        }),
    };
    assert_eq!(hold_of(&charging, METERED_GO, 20, None), None);
    // The battery's hold is said first where both hold.
    let both = Reading {
        metered: Some(true),
        ..on_battery(Some(5))
    };
    assert_eq!(hold_of(&both, METERED_WAIT, 20, None), Some(Hold::Battery));
}

/// What the system does not say holds nothing: an unknown connection, an unknown charge, a
/// computer with no battery.
#[test]
fn an_unknown_state_goes_on() {
    let unknown = Reading::default();
    assert_eq!(hold_of(&unknown, METERED_WAIT, 50, None), None);
    assert_eq!(hold_of(&on_battery(None), METERED_WAIT, 50, None), None);
}

/// The state is read only where Options asks for it.
#[test]
fn the_state_is_read_only_where_options_asks() {
    assert!(!watched(METERED_GO, 0));
    assert!(watched(METERED_ASK, 0));
    assert!(watched(METERED_WAIT, 0));
    assert!(watched(METERED_GO, 10));
}

/// A check's stand-in file: a metered connection, a battery at 12 %, both, and neither.
#[test]
fn a_stand_in_file_is_read_as_the_state() {
    assert_eq!(stand_in_of("metered\n"), metered_on_no_battery());
    let low = stand_in_of("battery 12\n");
    assert_eq!(
        (low.metered, low.battery.and_then(|b| b.percent)),
        (Some(false), Some(12))
    );
    let both = stand_in_of("metered\nbattery 5\n");
    assert_eq!(both.metered, Some(true));
    assert!(both.battery.is_some_and(|b| b.discharging));
    assert_eq!(
        stand_in_of(""),
        Reading {
            metered: Some(false),
            battery: None
        }
    );
}

fn metered_on_no_battery() -> Reading {
    Reading {
        metered: Some(true),
        battery: None,
    }
}
