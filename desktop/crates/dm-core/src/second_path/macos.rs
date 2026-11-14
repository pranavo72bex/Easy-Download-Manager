// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! macOS's names for its interfaces, for the second path's list.

use std::collections::HashMap;

/// The name macOS gives an interface ("iPhone USB", "Wi-Fi"): `networksetup`'s
/// hardware ports, read at most every few seconds (a phone plugged in shows
/// up by its name at the next look).
pub(super) fn system_name(interface: &str) -> Option<String> {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static PORTS: Mutex<Option<(Instant, HashMap<String, String>)>> = Mutex::new(None);
    let mut held = crate::lock(&PORTS);
    let stale = held
        .as_ref()
        .is_none_or(|(at, _)| at.elapsed() > Duration::from_secs(5));
    if stale {
        let listed = std::process::Command::new("/usr/sbin/networksetup")
            .arg("-listallhardwareports")
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .map(|o| ports_of(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or_default();
        *held = Some((Instant::now(), listed));
    }
    held.as_ref()
        .and_then(|(_, ports)| ports.get(interface).cloned())
}

/// Each device's hardware port, from `networksetup -listallhardwareports`
/// ("Hardware Port: iPhone USB" then "Device: en5").
pub(super) fn ports_of(listing: &str) -> HashMap<String, String> {
    let mut ports = HashMap::new();
    let mut port: Option<String> = None;
    for line in listing.lines() {
        if let Some(name) = line.strip_prefix("Hardware Port:") {
            port = Some(name.trim().to_string());
        } else if let Some(device) = line.strip_prefix("Device:") {
            if let Some(name) = port.take() {
                ports.insert(device.trim().to_string(), name);
            }
        }
    }
    ports
}
