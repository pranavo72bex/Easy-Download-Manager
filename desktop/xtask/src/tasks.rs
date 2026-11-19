// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `cargo xtask build`, `test` and `qa` (and `package`, `package.rs`): one command for each, the
//! same on Windows, macOS and Linux, from the repository's root or `desktop/` (gaps row X7).
//! Before them macOS and Linux had the Makefile, written for `uname`, and Windows had cargo and
//! PowerShell typed by hand; the Makefile's `test` and `qa` and its installers call these now.
//!
//! - `build`: the window dm ships, built for release (`cargo build --release -p dm-gpui`, with
//!   `scripts/release-flags.sh`'s flags on macOS and Linux), and the extension
//!   (`sh scripts/package.sh`: the WebAssembly, `dist/chrome`, `dist/firefox` and their zips).
//!   On Windows `sh` is Git for Windows' when it is not on `PATH`. Anything after `build` goes to
//!   `cargo build`.
//! - `test`: the workspace's tests as CI runs them, the extension's crate among them: cargo
//!   nextest and then the doctests, or `cargo test` where nextest is not installed. Every test
//!   runs (`--no-fail-fast`); anything after `test` goes to the test runner (`--profile ci`).
//! - `qa`: the behaviour contract, whole, in a visible window: the debug build human-qa.py drives
//!   (none where `DM_BINARY` names one), the extension where `dist/chrome` has none, then
//!   `python3 scripts/human-qa.py` as the contract says (`python -X utf8`, `PYTHONUTF8=1`, on
//!   Windows). Anything after `qa` goes to human-qa.py. On Windows it is started from PowerShell
//!   or cmd, never Git Bash, where dm's window is pictured black (refused there); on Linux it needs
//!   a display (`sh scripts/linux-human-qa.sh` runs it in a container that has one).
//!
//! A program that fails ends xtask with that program's exit code.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

/// The window dm ships, on every system.
const WINDOW: &str = "dm-gpui";

/// What `cargo xtask` says when no job, or an unknown one, is named.
pub(crate) const USAGE: &str = "\
usage: cargo xtask <job>, from the repository's root or desktop/
  build [cargo options]       dm-gpui for release and the extension (dist/)
  test [runner options]       the workspace's tests, as CI runs them
  package [packager options]  this system's installers (the .rpm too on Linux alone)
  qa [human-qa options]       scripts/human-qa.py, whole, in sight (Windows: PowerShell or cmd)
  check                       everything that needs no window
  i18n [<code>|--issue <code>] | help-check | scorecard | layers | privacy | parity | secrets
  size-baseline               lowers the size gate's baselines to the lengths now
  headers [fix] | update-key | sign-manifest <latest.json> | crates-list <file>
  sign-release <dir> | verify-release <dir>
  vendor [--version <v>] [<crate>...]  desktop/vendor's copies, made again from crates.io
build, test, package and qa do the same on Windows, macOS and Linux (xtask/src/tasks.rs).";

/* ------------------------------------------------------------ the lines */

/// Words as owned strings, for a command line.
fn words(line: &[&str]) -> Vec<String> {
    line.iter().map(|w| w.to_string()).collect()
}

/// `build`'s two programs, program first: the window, in `desktop/`, then the extension, from
/// the root. `extra` goes to `cargo build`.
fn build_lines(extra: &[String]) -> [Vec<String>; 2] {
    let mut app = words(&["cargo", "build", "--release", "-p", WINDOW]);
    app.extend(extra.iter().cloned());
    [app, words(&["sh", "scripts/package.sh"])]
}

/// `test`'s programs, in `desktop/`: nextest and the doctests it does not run, or `cargo test`,
/// which runs both. `extra` goes to the runner.
fn test_lines(nextest: bool, extra: &[String]) -> Vec<Vec<String>> {
    let mut lines = if nextest {
        vec![
            words(&[
                "cargo",
                "nextest",
                "run",
                "--workspace",
                "--locked",
                "--no-fail-fast",
            ]),
            words(&["cargo", "test", "--doc", "--workspace", "--locked"]),
        ]
    } else {
        vec![words(&[
            "cargo",
            "test",
            "--workspace",
            "--locked",
            "--no-fail-fast",
        ])]
    };
    if let Some(first) = lines.first_mut() {
        first.extend(extra.iter().cloned());
    }
    lines
}

/// The build `qa` makes for human-qa.py, in `desktop/`: a debug one, where it looks first.
fn qa_build_line() -> Vec<String> {
    words(&["cargo", "build", "-p", WINDOW])
}

/// human-qa.py on system `os` (`std::env::consts::OS`), from the root, with `extra` after it.
/// Windows' `python3` can be the Microsoft Store's placeholder, so it is `python` there.
fn qa_line(os: &str, extra: &[String]) -> Vec<String> {
    let mut line = if os == "windows" {
        words(&["python", "-X", "utf8"])
    } else {
        words(&["python3"])
    };
    line.push("scripts/human-qa.py".to_string());
    line.extend(extra.iter().cloned());
    line
}

/// What human-qa.py is given on `os` beside the caller's own environment.
fn qa_env(os: &str) -> Vec<(String, String)> {
    if os == "windows" {
        vec![("PYTHONUTF8".to_string(), "1".to_string())]
    } else {
        Vec::new()
    }
}

/// Why `qa` cannot run here, if it cannot: on Windows from Git Bash (`msystem`, its `MSYSTEM`),
/// on Linux with no display.
fn qa_refusal(os: &str, msystem: Option<&str>, display: bool) -> Option<String> {
    match os {
        "windows" if msystem.is_some_and(|m| !m.is_empty()) => Some(
            "cargo xtask qa: started from Git Bash, where dm's window is pictured solid black: \
             start it from PowerShell or cmd"
                .to_string(),
        ),
        "linux" if !display => Some(
            "cargo xtask qa: no display (DISPLAY or WAYLAND_DISPLAY): human-qa.py shows dm's \
             window; sh scripts/linux-human-qa.sh runs it in a container that has one"
                .to_string(),
        ),
        _ => None,
    }
}

/// The program's file name on `os`.
fn binary_name(os: &str) -> String {
    if os == "windows" {
        format!("{WINDOW}.exe")
    } else {
        WINDOW.to_string()
    }
}

/* ------------------------------------------------------------- running */

/// `path` without Windows' verbatim prefix (`\\?\`), which `canonicalize` puts on a drive's path
/// and which Git for Windows' `sh`, and some programs given it as a folder or in `DM_BINARY`, do
/// not read.
pub(crate) fn plain(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/// Where cargo builds: `CARGO_TARGET_DIR`, made absolute (cargo reads a relative one against the
/// folder it runs in, which is not the caller's here), or `desktop/target`.
pub(crate) fn target_dir(root: &Path) -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from) {
        Some(dir) if dir.is_absolute() => dir,
        Some(dir) => std::env::current_dir().map(|c| c.join(&dir)).unwrap_or(dir),
        None => root.join("desktop").join("target"),
    }
}

/// What every program run here is given: the absolute target folder, where one is set.
fn cargo_env(root: &Path) -> Vec<(String, String)> {
    if std::env::var_os("CARGO_TARGET_DIR").is_none() {
        return Vec::new();
    }
    let dir = target_dir(root).to_string_lossy().into_owned();
    vec![("CARGO_TARGET_DIR".to_string(), dir)]
}

/// `scripts/release-flags.sh`'s flags for a release build (without the building computer's
/// paths), or `None` where they are not asked: on Windows, whose target has rustflags of its own
/// that these would not replace (`desktop/.cargo/config.toml`), and where the caller set
/// `CARGO_BUILD_RUSTFLAGS` already.
pub(crate) fn release_flags(root: &Path) -> Result<Option<String>, String> {
    if std::env::consts::OS == "windows" || std::env::var_os("CARGO_BUILD_RUSTFLAGS").is_some() {
        return Ok(None);
    }
    let flags = Command::new("sh")
        .arg("scripts/release-flags.sh")
        .current_dir(root)
        .output()
        .map_err(|e| format!("sh scripts/release-flags.sh: {e}"))?;
    if !flags.status.success() {
        return Err("sh scripts/release-flags.sh failed".to_string());
    }
    Ok(Some(
        String::from_utf8_lossy(&flags.stdout).trim().to_string(),
    ))
}

/// The `sh` the shell scripts run in: on `PATH`, or on Windows Git for Windows' own.
fn shell() -> Result<String, String> {
    let on_path = Command::new("sh")
        .args(["-c", "exit 0"])
        .output()
        .is_ok_and(|o| o.status.success());
    if on_path {
        return Ok("sh".to_string());
    }
    if std::env::consts::OS == "windows" {
        let git = std::env::var_os("ProgramFiles")
            .map(|p| PathBuf::from(p).join("Git").join("bin").join("sh.exe"))
            .filter(|p| p.is_file());
        if let Some(sh) = git {
            return Ok(sh.to_string_lossy().into_owned());
        }
        return Err("needs sh: install Git for Windows, whose sh runs the scripts".to_string());
    }
    Err("needs sh, which runs the scripts".to_string())
}

/// Ends xtask with `status`'s exit code (1 where a signal stopped it), saying what failed.
pub(crate) fn exit_with(what: &str, status: ExitStatus) -> ! {
    eprintln!("{what} failed ({status})");
    std::process::exit(status.code().filter(|c| *c != 0).unwrap_or(1))
}

/// Runs `line` (program first) in `dir`, its output the terminal's, with `env` added.
fn run_status(dir: &Path, line: &[String], env: &[(String, String)]) -> Result<ExitStatus, String> {
    let (program, args) = line.split_first().ok_or("nothing to run")?;
    println!("== in {}: {}", dir.display(), line.join(" "));
    Command::new(program)
        .args(args)
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .current_dir(dir)
        .status()
        .map_err(|e| format!("could not start {program}: {e}"))
}

/// Runs `line` as [`run_status`] does; a failure ends xtask with the program's exit code.
fn run(dir: &Path, line: &[String], env: &[(String, String)]) -> Result<(), String> {
    let status = run_status(dir, line, env)?;
    if !status.success() {
        exit_with(&line.join(" "), status);
    }
    Ok(())
}

/// `line` with its `sh` replaced by the one found here.
fn with_shell(mut line: Vec<String>) -> Result<Vec<String>, String> {
    if line.first().is_some_and(|p| p == "sh") {
        line[0] = shell()?;
    }
    Ok(line)
}

/* --------------------------------------------------------------- tasks */

/// `cargo xtask build`. `root` is the repository's root.
pub(crate) fn build(root: &Path, extra: &[String]) -> Result<String, String> {
    let root = &plain(root);
    let [app, extension] = build_lines(extra);
    let env = cargo_env(root);
    let mut app_env = env.clone();
    if let Some(flags) = release_flags(root)? {
        app_env.push(("CARGO_BUILD_RUSTFLAGS".to_string(), flags));
    } else if std::env::consts::OS == "windows" {
        println!("(release-flags.sh's flags: macOS and Linux alone; Windows has its own)");
    }
    run(&root.join("desktop"), &app, &app_env)?;
    run(root, &with_shell(extension)?, &env)?;
    let binary = target_dir(root)
        .join("release")
        .join(binary_name(std::env::consts::OS));
    Ok(format!(
        "built {} (under release/ of a --target's own folder where one was named) and the \
         extension: dist/chrome, dist/firefox and their zips",
        binary.display()
    ))
}

/// `cargo xtask test`. Every program runs, and the first that failed gives the exit code.
pub(crate) fn test(root: &Path, extra: &[String]) -> Result<String, String> {
    let root = &plain(root);
    let desktop = root.join("desktop");
    let nextest = Command::new("cargo")
        .args(["nextest", "--version"])
        .current_dir(&desktop)
        .output()
        .is_ok_and(|o| o.status.success());
    if !nextest {
        println!("(cargo nextest is not installed: cargo test, slower, checks the same)");
    }
    let env = cargo_env(root);
    let mut failed = None;
    for line in test_lines(nextest, extra) {
        let status = run_status(&desktop, &line, &env)?;
        if !status.success() && failed.is_none() {
            failed = Some((line.join(" "), status));
        }
    }
    match failed {
        Some((what, status)) => exit_with(&what, status),
        None => Ok("every test passed".to_string()),
    }
}

/// `cargo xtask qa`.
pub(crate) fn qa(root: &Path, extra: &[String]) -> Result<String, String> {
    let root = &plain(root);
    let os = std::env::consts::OS;
    let msystem = std::env::var("MSYSTEM").ok();
    let display = ["DISPLAY", "WAYLAND_DISPLAY"]
        .iter()
        .any(|v| std::env::var_os(v).is_some_and(|d| !d.is_empty()));
    if let Some(why) = qa_refusal(os, msystem.as_deref(), display) {
        return Err(why);
    }
    let mut env = cargo_env(root);
    if std::env::var_os("DM_BINARY").is_some() {
        println!("(DM_BINARY names the build human-qa.py drives: none is made)");
    } else {
        run(&root.join("desktop"), &qa_build_line(), &env)?;
        let binary = target_dir(root).join("debug").join(binary_name(os));
        env.push((
            "DM_BINARY".to_string(),
            binary.to_string_lossy().into_owned(),
        ));
    }
    if root
        .join("dist")
        .join("chrome")
        .join("manifest.json")
        .is_file()
    {
        println!("(the extension in dist/chrome as it is: sh scripts/package.sh builds it again)");
    } else {
        run(
            root,
            &with_shell(words(&["sh", "scripts/package.sh"]))?,
            &env,
        )?;
    }
    env.extend(qa_env(os));
    run(root, &qa_line(os, extra), &env)?;
    Ok("human-qa.py: every row of the contract held".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_makes_the_shipped_window_for_release_and_the_extension() {
        let [app, extension] = build_lines(&words(&["--target", "x86_64-apple-darwin"]));
        let wanted = ["cargo", "build", "--release", "-p", "dm-gpui"];
        assert_eq!(app[..5], words(&wanted));
        assert_eq!(app[5..], words(&["--target", "x86_64-apple-darwin"]));
        assert_eq!(extension, words(&["sh", "scripts/package.sh"]));
    }

    #[test]
    fn test_runs_every_test_and_the_doctests_and_gives_the_runner_what_it_was_given() {
        let extra = words(&["--profile", "ci"]);
        let lines = test_lines(true, &extra);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with(&words(&["cargo", "nextest", "run", "--workspace"])));
        assert!(lines[0].ends_with(&extra) && lines[0].contains(&"--no-fail-fast".to_string()));
        assert_eq!(
            lines[1],
            words(&["cargo", "test", "--doc", "--workspace", "--locked"])
        );
        let lines = test_lines(false, &[]);
        assert_eq!(
            lines,
            [words(&[
                "cargo",
                "test",
                "--workspace",
                "--locked",
                "--no-fail-fast"
            ])]
        );
    }

    /// The contract's own command where it is valid as written; Windows' Python, in UTF-8.
    #[test]
    fn qa_runs_human_qa_as_the_contract_says_on_each_system() {
        let extra = words(&["--save-baseline"]);
        for os in ["macos", "linux"] {
            assert_eq!(
                qa_line(os, &extra),
                words(&["python3", "scripts/human-qa.py", "--save-baseline"])
            );
            assert!(qa_env(os).is_empty());
        }
        assert_eq!(
            qa_line("windows", &[]),
            words(&["python", "-X", "utf8", "scripts/human-qa.py"])
        );
        assert_eq!(
            qa_env("windows"),
            [("PYTHONUTF8".to_string(), "1".to_string())]
        );
        assert_eq!(qa_build_line(), words(&["cargo", "build", "-p", "dm-gpui"]));
    }

    #[test]
    fn qa_is_refused_from_git_bash_and_without_a_display_alone() {
        assert!(qa_refusal("windows", Some("MINGW64"), false).is_some());
        assert!(qa_refusal("windows", None, false).is_none());
        assert!(qa_refusal("windows", Some(""), false).is_none());
        assert!(qa_refusal("linux", None, false).is_some());
        assert!(qa_refusal("linux", None, true).is_none());
        assert!(qa_refusal("macos", Some("MINGW64"), false).is_none());
    }

    #[test]
    fn a_drives_path_loses_the_verbatim_prefix_and_a_shares_keeps_it() {
        assert_eq!(plain(Path::new(r"\\?\D:\dm")), Path::new(r"D:\dm"));
        assert_eq!(plain(Path::new("/repo")), Path::new("/repo"));
        let share = Path::new(r"\\?\UNC\host\dm");
        assert_eq!(plain(share), share);
    }

    #[test]
    fn the_program_is_named_as_each_system_names_it() {
        assert_eq!(binary_name("windows"), "dm-gpui.exe");
        assert_eq!(binary_name("macos"), "dm-gpui");
        assert_eq!(binary_name("linux"), "dm-gpui");
    }
}
