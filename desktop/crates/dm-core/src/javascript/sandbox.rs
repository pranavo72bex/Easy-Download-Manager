// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The JavaScript host shut off from the computer by the system itself, before it runs a
//! script (gaps row 201): the engine's own context already gives a script no files, programs or
//! network, and this is the wall behind it, for a fault in the engine that a script could use.
//!
//! The host loads its engine and runs a first call first (libraries, data files), then enters:
//!
//! - macOS: the system's sandbox, its "pure computation" profile: no files opened, no network,
//!   no programs started ([`macos`]);
//! - Linux: a seccomp filter on every thread: no program started or process made, no socket,
//!   no file opened to write or made, moved or removed, no other process read or signalled
//!   ([`linux`]);
//! - Windows: the process's own token lowered to low integrity (nothing of the person's can be
//!   written) and the process put in a job that starts no other ([`windows`]).
//!
//! The answer goes out on standard output, open before it entered.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// Shuts this process off from the computer, for good: Ok once it is, or why it could not be.
/// Where the system has no such wall (a Linux kernel built without seccomp, another system), Ok:
/// the engine's own context is then the only one, as before.
pub(super) fn enter() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    return linux::enter();
    #[cfg(target_os = "macos")]
    return macos::enter();
    #[cfg(windows)]
    return windows::enter();
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    Ok(())
}

#[cfg(test)]
mod tests {
    /// Set where this test binary is started to enter the sandbox and try the computer.
    const TRY_IT: &str = "DM_SANDBOX_TRY";

    /// Where this test binary is started so (the test below starts it): enters, then tries to
    /// write a file in the folder named, start a program and reach a port, saying what each did.
    #[test]
    fn inside_the_sandbox_when_this_binary_is_started_so() {
        let Some(folder) = std::env::var_os(TRY_IT) else {
            return;
        };
        let entered = super::enter();
        let wrote = std::fs::write(std::path::Path::new(&folder).join("out.txt"), b"x").is_ok();
        let program = if cfg!(windows) { "cmd" } else { "/bin/sh" };
        let flag = if cfg!(windows) { "/c" } else { "-c" };
        let started = std::process::Command::new(program)
            .args([flag, "exit 0"])
            .status()
            .is_ok();
        let port = std::env::var("DM_SANDBOX_PORT").unwrap_or_default();
        let reached = std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok();
        println!(
            "sandbox: entered={} wrote={wrote} started={started} reached={reached}",
            entered.is_ok()
        );
        std::process::exit(0);
    }

    /// The JavaScript host, once in, cannot write a file of the person's, start a program or,
    /// on macOS and Linux, reach the network (gaps row 201).
    #[test]
    fn once_in_the_host_cannot_write_start_or_reach() {
        let folder = tempfile::tempdir().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "javascript::sandbox::tests::inside_the_sandbox_when_this_binary_is_started_so",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(TRY_IT, folder.path())
            .env("DM_SANDBOX_PORT", port.to_string())
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stdout);
        let line = said
            .lines()
            .find_map(|l| l.find("sandbox: ").map(|at| &l[at..]))
            .unwrap_or_else(|| panic!("the sandboxed run said nothing: {said} {out:?}"));
        assert!(line.contains("entered=true"), "{line}");
        assert!(line.contains("wrote=false"), "{line}");
        assert!(line.contains("started=false"), "{line}");
        // Windows' low integrity keeps a process from the person's things, not the network.
        if !cfg!(windows) {
            assert!(line.contains("reached=false"), "{line}");
        }
        assert!(!folder.path().join("out.txt").exists());
    }
}
