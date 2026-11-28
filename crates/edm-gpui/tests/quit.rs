// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The GPUI binary with no window (`--headless`): it starts with no display
//! to draw on, answers on its control port, and, asked to end by the system
//! (a logout, `kill`, a service manager), ends as Program › Exit does, the
//! queue written down and `control.port` gone.

#![cfg(unix)]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn until(what: &str, within: Duration, said: &std::path::Path, mut done: impl FnMut() -> bool) {
    let end = Instant::now() + within;
    while !done() {
        assert!(
            Instant::now() < end,
            "{what} did not happen; EDM said:\n{}",
            std::fs::read_to_string(said).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn quits_cleanly_on(signal: &str) {
    let home = tempfile::tempdir().unwrap();
    let edm_home = home.path().join("EDM");
    std::fs::create_dir_all(&edm_home).unwrap();
    std::fs::write(
        edm_home.join("settings.json"),
        serde_json::json!({ "download_dir": home.path().join("out") }).to_string(),
    )
    .unwrap();
    let mut edm = Command::new(env!("CARGO_BIN_EXE_edm-gpui"))
        .args(["--headless", "--control", "0"])
        .env("EDM_HOME", &edm_home)
        .env("HOME", home.path())
        // No display to draw on: headless needs none.
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(home.path().join("stderr")).unwrap())
        .spawn()
        .unwrap();
    let said = home.path().join("stderr");
    let port = edm_home.join("control.port");
    // Generous: macOS checks a freshly built program on its first launch.
    until(
        "EDM writing control.port",
        Duration::from_secs(90),
        &said,
        || std::fs::read_to_string(&port).is_ok_and(|p| !p.trim().is_empty()),
    );

    let sent = Command::new("kill")
        .args([signal, &edm.id().to_string()])
        .status()
        .unwrap();
    assert!(sent.success());
    let mut ended = None;
    until("EDM ending", Duration::from_secs(30), &said, || {
        ended = edm.try_wait().unwrap();
        ended.is_some()
    });
    let ended = ended.unwrap();
    assert!(
        ended.success(),
        "{signal}: cut off rather than closed: {ended:?}; EDM said:\n{}",
        std::fs::read_to_string(&said).unwrap_or_default()
    );
    assert!(!port.exists(), "{signal}: control.port was left behind");
}

#[test]
fn sigterm_quits_as_program_exit_does() {
    quits_cleanly_on("-TERM");
}

#[test]
fn sigint_quits_as_program_exit_does() {
    quits_cleanly_on("-INT");
}
