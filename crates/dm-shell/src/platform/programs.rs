// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The system's own programs dm-shell runs, each behind one function (gaps row P8): `ps` for
//! the browser that started the native host, macOS's `open` for dm's window from Safari's
//! sandbox, and Windows' `reg.exe` for the `.reg` copies. Why each is a program and not a call
//! is in `docs/how-it-works.md` ("The system's programs").

use std::path::Path;

/// The program name of the process `parent` (`ps -o comm= -p <parent>`), as `ps` printed it;
/// `None` where `ps` could not be run.
#[cfg(all(unix, not(target_os = "android")))]
pub(crate) fn process_name(parent: u32) -> Option<String> {
    let out = std::process::Command::new("/bin/ps")
        .args(["-o", "comm=", "-p", &parent.to_string()])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// macOS's `open`, which opens an app as a person's click would.
pub(crate) const OPEN: &str = "/usr/bin/open";

/// Opens `app` with `--desktop` as a person's click would, through macOS's `open` ([`OPEN`]),
/// outside the sandbox the native host runs in; why not, where it could not.
pub(crate) fn open_app(app: &Path) -> std::io::Result<()> {
    open_app_through(Path::new(OPEN), app)
}

/// [`open_app`] through `opener` (a test's stand-in for `open`).
pub(crate) fn open_app_through(opener: &Path, app: &Path) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    let opened = Command::new(opener)
        .arg("-a")
        .arg(app)
        .args(["--args", "--desktop"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if opened.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "macOS could not open {} ({opened})",
            app.display()
        )))
    }
}

/// Windows' own `reg.exe` run with `args` (its `export` and `import`, the `.reg` files): `Ok`
/// where it succeeded, otherwise what it said on its error output.
#[cfg(windows)]
pub(crate) fn reg_exe(args: &[String]) -> Result<(), crate::Error> {
    let out = dm_core::no_window(&mut std::process::Command::new("reg"))
        .args(args)
        .output()
        .map_err(|e| format!("reg.exe could not be run: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().into())
    }
}
