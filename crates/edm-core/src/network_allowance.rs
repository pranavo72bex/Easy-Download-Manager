// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Each network's own allowance (gaps row K5c): so many megabytes every so many hours over the
//! second network or the third, as a phone's data plan allows, counted apart from the
//! allowance all downloads share (`quota`). A network whose allowance is spent takes no
//! connections until its period ends; the others carry the downloads meanwhile.

use std::collections::HashMap;
use std::time::SystemTime;

use crate::quota::{Allowance, Meter};

/// Each network's meter, by its number (1 the second, 2 the third).
static METERS: std::sync::Mutex<Option<HashMap<u8, Meter>>> = std::sync::Mutex::new(None);

/// Network `network`'s allowance; one of no bytes is none. What it has used so far stays
/// counted.
pub fn set_allowance(network: u8, allowance: Allowance) {
    let mut held = crate::lock(&METERS);
    held.get_or_insert_with(HashMap::new)
        .entry(network)
        .or_default()
        .set(allowance);
}

/// `bytes` read over network `network`, counted at `now`: once its allowance is spent, the
/// network is off until its period ends. The system's route (0) has no allowance of its own.
pub fn count(network: u8, bytes: u64, now: SystemTime) {
    if network == 0 {
        return;
    }
    let mut held = crate::lock(&METERS);
    let Some(meter) = held.as_mut().and_then(|meters| meters.get_mut(&network)) else {
        return;
    };
    meter.add(bytes, now);
    if let Some(wait) = meter.spent(now) {
        crate::second_path::set_off_for(network, wait);
    }
}

#[cfg(test)]
mod tests;
