// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use std::time::{Duration, SystemTime};

use super::*;

/// A network whose allowance is spent takes no connections until its period ends; the system's
/// route is never held, and a network with no allowance never is (gaps row K5c).
#[test]
fn a_network_whose_allowance_is_spent_takes_no_connections_until_its_period_ends() {
    // Network 9: a number no other test chooses, so the marks are this test's alone.
    let hour = Duration::from_secs(3600);
    set_allowance(
        9,
        Allowance {
            bytes: 1000,
            period: hour,
        },
    );
    let now = SystemTime::now();
    count(9, 600, now);
    let early = crate::second_path::is_off(9);
    count(9, 600, now);
    let spent = crate::second_path::is_off(9);
    count(0, 1 << 30, now);
    count(8, 1 << 30, now);
    let others = crate::second_path::is_off(0) || crate::second_path::is_off(8);
    set_allowance(9, Allowance::default());
    crate::second_path::set_off(9, false);
    assert!(!early, "off before its allowance was spent");
    assert!(spent, "its allowance spent, it still took connections");
    assert!(!others, "a network with no allowance of its own was held");
}
