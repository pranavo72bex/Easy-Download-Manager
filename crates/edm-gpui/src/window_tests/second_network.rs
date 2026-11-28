// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The second network (Options › Advanced › Network) as the progress window shows it.

use edm_core::State;

use crate::harness::world::world;

#[test]
fn a_progress_window_says_what_came_over_the_second_network() {
    // This computer's own address on a network: the loopback server answers it as it answers
    // 127.0.0.1 (`edm-core/tests/second_path.rs`).
    let Some(network) = edm_core::interfaces()
        .into_iter()
        .find(|i| i.address.is_ipv4())
    else {
        eprintln!("no network address here; skipped");
        return;
    };
    // Settings put in force by another test's window set the process's second network again
    // (`shell::engine_wide`, at every change): held off while this one runs, as those that
    // write the settings hold the file.
    let _file = crate::harness::settings_file();
    /// The second path is the process's: off again however the test ends.
    struct Off;
    impl Drop for Off {
        fn drop(&mut self) {
            edm_core::set_second_client(None);
        }
    }
    // Bound to its address alone (by its name, a Linux socket cannot reach the loopback
    // server; `edm-core/tests/second_path.rs`).
    edm_core::set_second_client(Some(
        edm_core::builder()
            .local_address(network.address)
            .build()
            .unwrap(),
    ));
    let _off = Off;
    let world = world();
    // Two connections or more: the second goes out on the second network.
    let mut h = world.harness_with(world.settings_with(|s| {
        s.downloads.connections = 4;
        s.general.show_progress = true;
    }));
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
