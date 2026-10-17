//! dm asked to end by the system — a logout, `kill`, a service manager —
//! ends as Program › Exit does: it was cut off where it stood, and left
//! `control.port` naming a port nobody listened on.

#![cfg(unix)]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn until(what: &str, within: Duration, mut done: impl FnMut() -> bool) {
    let end = Instant::now() + within;
    while !done() {
        assert!(Instant::now() < end, "{what} did not happen");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn quits_cleanly_on(signal: &str) {
    let home = tempfile::tempdir().unwrap();
    let dm_home = home.path().join("dm");
    std::fs::create_dir_all(&dm_home).unwrap();
    std::fs::write(
        dm_home.join("settings.json"),
        serde_json::json!({ "download_dir": home.path().join("out") }).to_string(),
    )
    .unwrap();
    let mut dm = Command::new(env!("CARGO_BIN_EXE_dm-desktop"))
        .args(["--headless", "--control", "0"])
        .env("DM_HOME", &dm_home)
        .env("HOME", home.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let port = dm_home.join("control.port");
    until("dm writing control.port", Duration::from_secs(30), || {
        std::fs::read_to_string(&port).is_ok_and(|p| !p.trim().is_empty())
    });

    let sent = Command::new("kill")
        .args([signal, &dm.id().to_string()])
        .status()
        .unwrap();
    assert!(sent.success());
    let mut ended = None;
    until("dm ending", Duration::from_secs(30), || {
        ended = dm.try_wait().unwrap();
        ended.is_some()
    });
    let ended = ended.unwrap();
    assert!(
        ended.success(),
        "{signal}: cut off rather than closed: {ended:?}"
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
