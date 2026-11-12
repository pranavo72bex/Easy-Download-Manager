// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Linux and the other Unix desktops: the first of the desktops' openers that is there, waited
//! on long enough to hear it fail. No portable "select the file", so a folder is opened.

use std::path::{Path, PathBuf};

use crate::settings;

/// Shows `path`'s folder.
pub(crate) fn reveal(path: &Path, _: &str) -> Result<(), String> {
    linux_open(path.parent().unwrap_or(path))
}

/// Opens `path` with the desktop's program for it.
pub(crate) fn open(path: &Path, _: &str) -> Result<(), String> {
    linux_open(path)
}

/// No short forms here.
pub(crate) fn short_form(_: &str) -> Option<String> {
    None
}

/// The programs a Linux desktop opens things with, in the order tried:
/// the freedesktop one, then GNOME's, KDE's and Xfce's own, which a
/// minimal install can have without `xdg-utils`.
const LINUX_OPENERS: [&[&str]; 5] = [
    &["xdg-open"],
    &["gio", "open"],
    &["kde-open"],
    &["kde-open5"],
    &["exo-open"],
];

/// Opens a file or folder on Linux with the first opener that is there.
///
/// `xdg-open` comes with `xdg-utils`, which the packages now require, but
/// a desktop that lacks it (a minimal install, a container) still has its
/// own. Started from an AppImage, the AppImage's private library path is
/// not passed on: an opener loading its libraries from there fails in ways
/// nobody would trace to dm.
pub(crate) fn linux_open(target: &Path) -> Result<(), String> {
    linux_open_in(target, None)
}

/// How long an opener is given to fail before it is taken to be opening.
///
/// `xdg-open` with nothing to hand the file to ("no method available")
/// exits at once with 3 or 4; one that found a program has handed over or
/// is running it within a few hundred milliseconds. Waited for in place,
/// since the answer is the button's: only an opener still running after
/// this holds the window this long.
const OPENER_VERDICT: std::time::Duration = std::time::Duration::from_millis(1500);

/// The same, looking for the openers along `path` where one is given.
///
/// Each opener's exit is waited for (`OPENER_VERDICT`): `xdg-open` that
/// starts and then fails was taken as success, and the file simply did not
/// open. A failing opener passes to the next; when every one there failed,
/// what each said is the error.
pub(crate) fn linux_open_in(target: &Path, path: Option<&std::ffi::OsStr>) -> Result<(), String> {
    let in_appimage = std::env::var_os("APPIMAGE").is_some();
    let mut failures: Vec<String> = Vec::new();
    for opener in LINUX_OPENERS {
        let Some((program, args)) = opener.split_first() else {
            continue;
        };
        let mut command = std::process::Command::new(program);
        command.args(args.iter()).arg(target);
        if let Some(path) = path {
            command.env("PATH", path);
        }
        if in_appimage {
            command
                .env_remove("LD_LIBRARY_PATH")
                .env_remove("LD_PRELOAD");
        }
        // Its error goes to a file, not a pipe: an opener that runs the
        // program in the foreground passes its stderr on, and a pipe whose
        // reader has gone would kill that program at its first warning.
        let said = OpenerSays::new();
        command.stdin(std::process::Stdio::null());
        if let Some(file) = said.as_ref().and_then(|s| s.stdio()) {
            command.stderr(file);
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.to_string()),
        };
        let deadline = std::time::Instant::now() + OPENER_VERDICT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                // Still running: it found something to open the file with.
                // Reaped when it ends, so it leaves no zombie behind.
                Ok(None) | Err(_) => {
                    std::thread::spawn(move || child.wait());
                    break None;
                }
            }
        };
        match status {
            None => return Ok(()),
            Some(status) if status.success() => return Ok(()),
            Some(status) => {
                let words = said.map(|s| s.read()).unwrap_or_default();
                let name = opener.join(" ");
                failures.push(if words.is_empty() {
                    match status.code() {
                        Some(code) => format!("{name} failed (exit {code})"),
                        None => format!("{name} failed"),
                    }
                } else {
                    format!("{name}: {words}")
                });
            }
        }
    }
    if !failures.is_empty() {
        return Err(format!(
            "this computer could not open {}: {}",
            target.display(),
            failures.join("; ")
        ));
    }
    Err("nothing on this computer opens files: install xdg-utils \
         (`sudo apt install xdg-utils`, `sudo dnf install xdg-utils` or \
         `sudo pacman -S xdg-utils`)"
        .into())
}

/// A file an opener's stderr goes to, removed when read or dropped.
///
/// In dm's own home, which only this account can enter, made new (never a file or link already
/// there) and handed to the opener as the same open file: in the shared temporary folder,
/// another account could have put a link at its name first, and dm truncated what it pointed at
/// and wrote the opener's words into it (security audit, 2026-11-12).
struct OpenerSays(PathBuf, std::fs::File);

impl OpenerSays {
    fn new() -> Option<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = settings::home();
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join(format!(
            ".opener-{}-{}.txt",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let file = std::fs::OpenOptions::new()
            .read(true)
            .append(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .ok()?;
        Some(Self(path, file))
    }

    fn stdio(&self) -> Option<std::process::Stdio> {
        self.1.try_clone().ok().map(std::process::Stdio::from)
    }

    /// Its last few lines, which is where an opener says why.
    fn read(self) -> String {
        let text = std::fs::read_to_string(&self.0).unwrap_or_default();
        let lines: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        let tail = lines.len().saturating_sub(3);
        lines.get(tail..).unwrap_or_default().join(" ")
    }
}

impl Drop for OpenerSays {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
