// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! NetworkManager's word on the connection's cost, and the kernel's on the battery (gaps row
//! W11).

use std::path::Path;
use std::sync::OnceLock;

use super::Battery;

/// NetworkManager's name and object on the system bus.
const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";

/// The system bus, connected once: a connection made at each read would be a socket and a
/// handshake every few seconds. `None` where there is none (a container, a system without
/// D-Bus).
fn bus() -> Option<&'static zbus::blocking::Connection> {
    static BUS: OnceLock<Option<zbus::blocking::Connection>> = OnceLock::new();
    BUS.get_or_init(|| zbus::blocking::Connection::system().ok())
        .as_ref()
}

/// Whether the connection is metered, as NetworkManager's `Metered` says of the primary
/// connection: yes (1) or guessed so (3, a phone's hotspot); no (2) or guessed not (4).
/// `None` for unknown (0), or with no NetworkManager.
pub(super) fn metered() -> Option<bool> {
    let connection = bus()?;
    let proxy = zbus::blocking::proxy::Builder::<zbus::blocking::Proxy>::new(connection)
        .destination(NM)
        .and_then(|b| b.path(NM_PATH))
        .and_then(|b| b.interface(NM))
        .map(|b| b.cache_properties(zbus::proxy::CacheProperties::No))
        .and_then(|b| b.build())
        .ok()?;
    of_nm_metered(proxy.get_property::<u32>("Metered").ok()?)
}

/// What NetworkManager's `NMMetered` value says.
fn of_nm_metered(value: u32) -> Option<bool> {
    match value {
        1 | 3 => Some(true),
        2 | 4 => Some(false),
        _ => None,
    }
}

/// The computer's battery from the kernel's power supplies under `dir`
/// (`/sys/class/power_supply`): each of `type` "Battery" that powers the computer (not a
/// mouse's or a keyboard's, whose `scope` is "Device"). Discharging where any says so; the
/// emptiest one's `capacity`. `None` with no such battery.
pub(super) fn battery(dir: &Path) -> Option<Battery> {
    let read = |supply: &Path, name: &str| {
        std::fs::read_to_string(supply.join(name))
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };
    let mut found: Option<Battery> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let supply = entry.path();
        if read(&supply, "type") != "Battery" || read(&supply, "scope") == "Device" {
            continue;
        }
        let discharging = read(&supply, "status") == "Discharging";
        let percent = read(&supply, "capacity")
            .parse::<u8>()
            .ok()
            .filter(|p| *p <= 100);
        let before = found.unwrap_or_default();
        found = Some(Battery {
            discharging: before.discharging || discharging,
            percent: match (before.percent, percent) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            },
        });
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supply(dir: &Path, name: &str, files: &[(&str, &str)]) {
        let at = dir.join(name);
        std::fs::create_dir_all(&at).unwrap();
        for (file, text) in files {
            std::fs::write(at.join(file), format!("{text}\n")).unwrap();
        }
    }

    /// A laptop off its charger at 12 %, with a mouse's battery beside it that is not the
    /// computer's; on its charger; and a desktop with only mains.
    #[test]
    fn the_kernels_power_supplies_are_read() {
        let dir = tempfile::tempdir().unwrap();
        supply(dir.path(), "AC", &[("type", "Mains"), ("online", "0")]);
        supply(
            dir.path(),
            "BAT0",
            &[
                ("type", "Battery"),
                ("status", "Discharging"),
                ("capacity", "12"),
            ],
        );
        supply(
            dir.path(),
            "hidpp_battery_0",
            &[("type", "Battery"), ("scope", "Device"), ("capacity", "3")],
        );
        assert_eq!(
            battery(dir.path()),
            Some(Battery {
                discharging: true,
                percent: Some(12)
            })
        );
        supply(dir.path(), "BAT0", &[("status", "Charging")]);
        assert_eq!(battery(dir.path()).map(|b| b.discharging), Some(false));

        let desktop = tempfile::tempdir().unwrap();
        supply(desktop.path(), "AC", &[("type", "Mains"), ("online", "1")]);
        assert_eq!(battery(desktop.path()), None);
    }

    /// NetworkManager's five answers: yes, guessed yes, no, guessed no, and unknown.
    #[test]
    fn network_managers_metered_is_read() {
        assert_eq!(of_nm_metered(1), Some(true));
        assert_eq!(of_nm_metered(3), Some(true));
        assert_eq!(of_nm_metered(2), Some(false));
        assert_eq!(of_nm_metered(4), Some(false));
        assert_eq!(of_nm_metered(0), None);
    }
}
