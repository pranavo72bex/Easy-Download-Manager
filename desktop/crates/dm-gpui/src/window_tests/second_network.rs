// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The second network (Options › Advanced › Network) as the progress window shows it. Moved
//! from the egui window's tests (`dm-desktop/tests/second_network.rs`).

use dm_core::State;

use crate::harness::world::world;

#[test]
fn a_progress_window_says_what_came_over_the_second_network() {
    // This computer's own address on a network: the loopback server answers it as it answers
    // 127.0.0.1 (`dm-core/tests/second_path.rs`).
    let Some(network) = dm_core::interfaces()
        .into_iter()
        .find(|i| i.address.is_ipv4())
    else {
        eprintln!("no network address here; skipped");
        return;
    };
    /// The second path is the process's: off again however the test ends.
    struct Off;
    impl Drop for Off {
        fn drop(&mut self) {
            dm_core::set_second_client(None);
        }
    }
    // Bound to its address alone (by its name, a Linux socket cannot reach the loopback
    // server; `dm-core/tests/second_path.rs`).
    dm_core::set_second_client(Some(
        dm_core::builder()
            .local_address(network.address)
            .build()
            .unwrap(),
    ));
    let _off = Off;
    let world = world();
    // Two connections or more: the second goes out on the second network.
    let mut h = world.harness_with(dm_shell::Settings {
        connections: 4,
        show_progress: true,
        ..world.settings()
    });
    h.add_url(&world.url("ranges-stall.bin"), "Start now");
    // A piece taken on each path (every other connection is the second's).
    h.until("pieces taken on both networks", |h| {
        let rows = h.rows();
        h.view(|view| {
            rows.iter().any(|r| {
                let connections = view.backend.connections(r.id);
                r.state == State::Running
                    && connections.iter().any(|c| c.second)
                    && connections.iter().any(|c| !c.second)
            })
        })
    });
    // The share is with the connections, under their heading.
    h.until_shown("Connections");
    h.click("Connections");
    h.until("the second network's share", |h| {
        h.texts()
            .iter()
            .any(|t| t.contains("came over the second network"))
    });
    h.click("Stop download");
}
