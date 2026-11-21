// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The system's own programs the engine runs, each behind one function (gaps row P8): the
//! secret store's tool, macOS's `security` (the keychain) and Linux's `secret-tool` (the
//! session's Secret Service), for `secretstore::tools`. Why each is a program and not a call is
//! in `docs/how-it-works.md` ("The system's programs").

/// The secret store's tool: `security` on macOS, `secret-tool` on Linux.
#[cfg(target_os = "macos")]
pub(crate) const SECRET_TOOL: &str = "security";
/// The secret store's tool: `security` on macOS, `secret-tool` on Linux.
#[cfg(target_os = "linux")]
pub(crate) const SECRET_TOOL: &str = "secret-tool";

/// Runs [`SECRET_TOOL`] with `args`, `stdin` fed to it, and gives what it printed and how it
/// ended; why not, where it could not be started or fed.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn secret_tool(
    args: &[String],
    stdin: Option<&[u8]>,
) -> Result<std::process::Output, String> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    let mut command = Command::new(SECRET_TOOL);
    command
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| format!("{SECRET_TOOL}: {e}"))?;
    if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(bytes)
            .map_err(|e| format!("{SECRET_TOOL}: {e}"))?;
    }
    child
        .wait_with_output()
        .map_err(|e| format!("{SECRET_TOOL}: {e}"))
}
