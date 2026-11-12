// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A magnet link Windows opens through dm's key becomes a row in the dm that
//! is running (gaps T6; done by hand on 2026-11-02).
//!
//! On Windows it writes the real `HKCU\Software\Classes\magnet` key, so it runs
//! only when asked: `DM_REGISTRY_TEST=1 cargo test -p dm-gpui --test
//! magnet_registry -- --ignored`. What the key held before is exported first
//! and put back afterwards, however the test ends. A program chosen in
//! Settings › Default apps decides over any key and cannot be written, so the
//! test stops and says so where one is chosen.
//!
//! Elsewhere there is no registry: the test runs the command the key would
//! hold, as Windows would, which checks that command and the hand-off to the
//! running dm. Moved from the egui window's tests, with the GPUI binary.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use dm_desktop_base::mcp::{finder, Control};
use serde_json::json;

/// A made-up torrent: the row appears before any peer is asked for it.
const MAGNET: &str =
    "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=dm-registry-test";

fn said(file: &Path) -> String {
    std::fs::read_to_string(file).unwrap_or_default()
}

fn until(what: &str, within: Duration, log: &Path, mut done: impl FnMut() -> bool) {
    let end = Instant::now() + within;
    while !done() {
        assert!(
            Instant::now() < end,
            "{what} did not happen; dm said:\n{}",
            said(log)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The dm under test, stopped however the test ends.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// dm's magnet key for the test's run, and what it replaced put back after.
#[cfg(windows)]
struct Key {
    backup: Option<std::path::PathBuf>,
}

#[cfg(windows)]
impl Key {
    const KEY: &'static str = r"HKCU\Software\Classes\magnet";

    fn take(exe: &Path, dir: &Path) -> Key {
        use dm_shell::register::{magnet_keys, magnet_owner, run_reg, Owner};
        let s = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        assert_ne!(
            magnet_owner(run_reg),
            Owner::WindowsChoice,
            "a program is chosen for magnet links in Settings › Default apps, which decides \
             over any key: choose none there (or dm) and run this again"
        );
        let backup = run_reg(&s(&["query", Key::KEY])).is_ok().then(|| {
            let file = dir.join("magnet-before-test.reg");
            run_reg(&s(&["export", Key::KEY, &file.display().to_string(), "/y"]))
                .expect("the magnet key could not be saved, so it is left alone");
            run_reg(&s(&["delete", Key::KEY, "/f"])).unwrap();
            file
        });
        let key = Key { backup };
        for args in magnet_keys(exe) {
            run_reg(&args).unwrap();
        }
        key
    }
}

#[cfg(windows)]
impl Drop for Key {
    fn drop(&mut self) {
        use dm_shell::register::run_reg;
        let s = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        let _ = run_reg(&s(&["delete", Key::KEY, "/f"]));
        if let Some(file) = &self.backup {
            if let Err(e) = run_reg(&s(&["import", &file.display().to_string()])) {
                eprintln!(
                    "the magnet key as it was could not be put back ({e}); it is in {}",
                    file.display()
                );
            }
        }
    }
}

/// Opens `MAGNET` as a click on a link does: through the shell, which reads
/// the key and starts its command.
#[cfg(windows)]
fn open_magnet(_exe: &Path, dm_home: &Path, home: &Path) -> std::process::ExitStatus {
    Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!("Start-Process '{MAGNET}'"),
        ])
        .env("DM_HOME", dm_home)
        .env("USERPROFILE", home)
        .status()
        .unwrap()
}

/// Runs the command dm's key would hold, `"<program>" "%1"`, with `%1` the link.
#[cfg(not(windows))]
fn open_magnet(exe: &Path, dm_home: &Path, home: &Path) -> std::process::ExitStatus {
    let keys = dm_shell::register::magnet_keys(exe);
    let command = keys
        .last()
        .and_then(|args| {
            let at = args.iter().position(|a| a == "/d")?;
            args.get(at + 1)
        })
        .expect("the key's command is the data of its last value");
    let words: Vec<String> = command
        .split('"')
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .map(|w| w.replace("%1", MAGNET))
        .collect();
    assert_eq!(words.len(), 2, "not a program and a link: {command}");
    Command::new(&words[0])
        .args(&words[1..])
        .env("DM_HOME", dm_home)
        .env("HOME", home)
        .status()
        .unwrap()
}

#[test]
#[ignore = "writes the system's magnet handler on Windows: DM_REGISTRY_TEST=1, -- --ignored"]
fn a_magnet_opened_through_dms_key_becomes_a_row_in_the_running_dm() {
    if cfg!(windows) && std::env::var_os("DM_REGISTRY_TEST").is_none_or(|v| v.is_empty()) {
        eprintln!("not run: DM_REGISTRY_TEST is not set");
        return;
    }
    // Spelled as the running dm finds itself (`register::ensure_this_copy`). Cargo
    // keeps a `CARGO_TARGET_DIR` as given, so `D:/dm-target` makes the path
    // `D:/dm-target\debug\…`, and Windows' shell refuses a key whose command
    // names its program with a forward slash: "Access is denied" (2026-11-10).
    let exe = Path::new(env!("CARGO_BIN_EXE_dm-gpui"))
        .canonicalize()
        .map(dm_core::plain_path)
        .unwrap();
    let exe = exe.as_path();
    let home = tempfile::tempdir().unwrap();
    let dm_home = home.path().join("dm");
    std::fs::create_dir_all(&dm_home).unwrap();
    std::fs::write(
        dm_home.join("settings.json"),
        json!({ "download_dir": home.path().join("out") }).to_string(),
    )
    .unwrap();
    let log = home.path().join("stderr");
    let _dm = Running(
        Command::new(exe)
            .args(["--headless", "--control", "0"])
            .env("DM_HOME", &dm_home)
            .env("HOME", home.path())
            .env("USERPROFILE", home.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&log).unwrap())
            .spawn()
            .unwrap(),
    );
    let port = dm_home.join("control.port");
    // A first launch of a fresh build is checked by the system, which took
    // seconds under load.
    until(
        "dm writing control.port",
        Duration::from_secs(90),
        &log,
        || std::fs::read_to_string(&port).is_ok_and(|p| !p.trim().is_empty()),
    );
    let mut control = Control::with(finder(&dm_home, None, None)).unwrap();
    let rows = |control: &mut Control| control.call("queue.list", json!({})).unwrap();
    assert_eq!(rows(&mut control), json!([]), "a new dm has no rows");

    #[cfg(windows)]
    let _key = Key::take(exe, home.path());
    let opened = open_magnet(exe, &dm_home, home.path());
    assert!(
        opened.success(),
        "the magnet link was not opened: {opened:?}"
    );

    let mut seen = json!(null);
    until(
        "a row for the magnet link",
        Duration::from_secs(60),
        &log,
        || {
            seen = rows(&mut control);
            seen.as_array().is_some_and(|all| {
                all.iter().any(|row| {
                    row.get("url")
                        .and_then(|u| u.as_str())
                        .is_some_and(|u| u.contains("0123456789abcdef0123456789abcdef01234567"))
                })
            })
        },
    );
    assert_eq!(
        seen.as_array().map(Vec::len),
        Some(1),
        "one link opened, one row: {seen}"
    );
}
