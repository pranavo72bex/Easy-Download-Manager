//! The progress window with a second network path (Options › Network), in
//! a test binary of its own: the second path is the process's, and the
//! window tests beside it apply settings that turn it off as they go.

#[path = "window/support.rs"]
#[allow(dead_code, unused_imports)]
mod support;

use support::*;

#[test]
fn a_progress_window_says_what_came_over_the_second_network() {
    // This computer's own address on a network: the loopback server answers
    // it as it answers 127.0.0.1 (`dm-core/tests/second_path.rs`).
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
            dm_core::second_path::set_second_client(None);
        }
    }
    // Bound to its address alone (by its name, a Linux socket cannot reach
    // the loopback server; `dm-core/tests/second_path.rs`).
    dm_core::second_path::set_second_client(Some(
        dm_core::builder()
            .local_address(network.address)
            .build()
            .unwrap(),
    ));
    let _off = Off;
    let world = world();
    // Two connections or more: the second goes out on the second network.
    let mut harness = world.harness_with(Settings {
        connections: 4,
        show_progress: true,
        ..world.settings()
    });
    add_url(&mut harness, &world.url("ranges-stall.bin"), "Start now");
    // A piece taken on each path (every other connection is the second's).
    until(&mut harness, "pieces taken on both networks", |app| {
        app.rows().iter().any(|r| {
            let connections = app.backend().connections(r.id);
            r.state == State::Running
                && connections.iter().any(|c| c.second)
                && connections.iter().any(|c| !c.second)
        })
    });
    harness.run_steps(3);
    // The share is with the connections, under their heading.
    click(&mut harness, "Connections");
    harness.run_steps(3);
    assert!(
        harness
            .query_by_label_contains("came over the second network")
            .is_some(),
        "the progress window does not say each path's share"
    );
    click(&mut harness, "Stop download");
}
