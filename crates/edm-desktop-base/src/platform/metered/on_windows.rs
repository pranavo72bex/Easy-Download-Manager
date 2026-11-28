// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Windows' word on the connection's cost and the battery (gaps row W11).

use windows::Networking::Connectivity::{NetworkCostType, NetworkInformation};
use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

use super::Battery;

/// Whether the Internet connection is metered, as Settings › Network sets it (or Windows for a
/// phone's connection): a fixed or variable cost, roaming, or over its data limit. `None` with
/// no Internet connection, or where Windows does not answer.
pub(super) fn metered() -> Option<bool> {
    let profile = NetworkInformation::GetInternetConnectionProfile().ok()?;
    let cost = profile.GetConnectionCost().ok()?;
    let kind = cost.NetworkCostType().ok()?;
    if kind == NetworkCostType::Unknown {
        return None;
    }
    let capped = kind == NetworkCostType::Fixed || kind == NetworkCostType::Variable;
    Some(capped || cost.Roaming().unwrap_or(false) || cost.OverDataLimit().unwrap_or(false))
}

/// The battery: discharging where the computer is off its charger, and how full. `None` for a
/// computer with no battery, or where Windows does not say.
pub(super) fn battery() -> Option<Battery> {
    let mut status = SYSTEM_POWER_STATUS {
        ACLineStatus: 0,
        BatteryFlag: 0,
        BatteryLifePercent: 0,
        SystemStatusFlag: 0,
        BatteryLifeTime: 0,
        BatteryFullLifeTime: 0,
    };
    // SAFETY: a pointer to a local the call fills in; the call has no other input.
    let read = unsafe { GetSystemPowerStatus(&mut status) };
    if read == 0 {
        return None;
    }
    of_status(&status)
}

/// What a `SYSTEM_POWER_STATUS` says of the battery: none where its flag says "no system
/// battery" (128) or "unknown" (255); off the charger where `ACLineStatus` is 0 (1 is on it,
/// 255 unknown); a percent where it is not 255, "unknown".
fn of_status(status: &SYSTEM_POWER_STATUS) -> Option<Battery> {
    if status.BatteryFlag == 255 || status.BatteryFlag & 128 != 0 {
        return None;
    }
    Some(Battery {
        discharging: status.ACLineStatus == 0,
        percent: (status.BatteryLifePercent <= 100).then_some(status.BatteryLifePercent),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(line: u8, flag: u8, percent: u8) -> SYSTEM_POWER_STATUS {
        SYSTEM_POWER_STATUS {
            ACLineStatus: line,
            BatteryFlag: flag,
            BatteryLifePercent: percent,
            SystemStatusFlag: 0,
            BatteryLifeTime: 0,
            BatteryFullLifeTime: 0,
        }
    }

    /// Off the charger at 15 %, on it at 80 %, a desktop with no battery, and a battery that
    /// does not say how full it is.
    #[test]
    fn the_power_status_is_read_as_windows_sets_it() {
        assert_eq!(
            of_status(&status(0, 2, 15)),
            Some(Battery {
                discharging: true,
                percent: Some(15)
            })
        );
        assert_eq!(
            of_status(&status(1, 8, 80)),
            Some(Battery {
                discharging: false,
                percent: Some(80)
            })
        );
        assert_eq!(of_status(&status(1, 128, 255)), None);
        assert_eq!(of_status(&status(255, 255, 255)), None);
        assert_eq!(of_status(&status(0, 1, 255)).and_then(|b| b.percent), None);
    }
}
