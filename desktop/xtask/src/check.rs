// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `cargo xtask check`: every check dm has that needs no window, in one command, in the order
//! that fails soonest (gaps row A1). Before it the gate was nine or more commands in three
//! languages, written out in `CLAUDE.md`, the workflows and people's memories.
//!
//! Each step runs to its end even when one before it failed, so one run names everything that is
//! wrong; the command fails if any step did. The window's own checks (`scripts/human-qa.py`, in a
//! visible window) are not here: they need a person's screen.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// One step: what it is called, the folder it runs in, and the command.
struct Step {
    name: &'static str,
    dir: PathBuf,
    program: &'static str,
    args: Vec<&'static str>,
    /// What to say where the program is not installed.
    install: Option<&'static str>,
}

fn step(name: &'static str, dir: &Path, program: &'static str, args: &[&'static str]) -> Step {
    Step {
        name,
        dir: dir.to_path_buf(),
        program,
        args: args.to_vec(),
        install: None,
    }
}

/// The steps, cheapest first. `root` is the repository's root.
fn steps(root: &Path) -> Vec<Step> {
    let desktop = root.join("desktop");
    let browser = root.join("browser");
    // nextest runs the test binaries side by side and stops a test that hangs; plain
    // `cargo test` where it is not installed, which is slower but checks the same.
    let nextest = Command::new("cargo")
        .args(["nextest", "--version"])
        .current_dir(&desktop)
        .output()
        .is_ok_and(|o| o.status.success());
    let mut steps = vec![
        step(
            "desktop: formatting",
            &desktop,
            "cargo",
            &["fmt", "--all", "--", "--check"],
        ),
        step(
            "browser: formatting",
            &browser,
            "cargo",
            &["fmt", "--", "--check"],
        ),
        step(
            "desktop: lints (warnings refused)",
            &desktop,
            "cargo",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--locked",
                "--",
                "-D",
                "warnings",
            ],
        ),
        step(
            "browser: lints (warnings refused)",
            &browser,
            "cargo",
            &["clippy", "--all-targets", "--", "-D", "warnings"],
        ),
        step(
            "browser: lints as WebAssembly (warnings refused)",
            &browser,
            "cargo",
            &[
                "clippy",
                "--target",
                "wasm32-unknown-unknown",
                "--",
                "-D",
                "warnings",
            ],
        ),
        Step {
            install: Some("cargo install cargo-deny --locked"),
            // Advisories are left to the audit workflow: they need the advisory database
            // fetched, and a check run offline should not fail for that.
            ..step(
                "desktop: licences, sources and duplicate versions",
                &desktop,
                "cargo",
                &["deny", "check", "licenses", "bans", "sources"],
            )
        },
    ];
    if nextest {
        steps.push(step(
            "desktop: tests",
            &desktop,
            "cargo",
            &[
                "nextest",
                "run",
                "--workspace",
                "--locked",
                "--no-fail-fast",
            ],
        ));
        // nextest does not run doctests.
        steps.push(step(
            "desktop: doctests",
            &desktop,
            "cargo",
            &["test", "--doc", "--workspace", "--locked"],
        ));
    } else {
        // Doctests included, and xtask's own checks: layers, privacy, parity, secrets, headers,
        // the scorecard (each is a test).
        steps.push(step(
            "desktop: tests",
            &desktop,
            "cargo",
            &["test", "--workspace", "--locked", "--no-fail-fast"],
        ));
    }
    steps.push(step("browser: tests", &browser, "cargo", &["test"]));
    steps
}

/// Runs every step and says how each went.
pub(crate) fn run(root: &Path) -> Result<String, String> {
    let mut failed = Vec::new();
    let mut report = Vec::new();
    for step in steps(root) {
        println!("== {}: {} {}", step.name, step.program, step.args.join(" "));
        let began = Instant::now();
        let status = Command::new(step.program)
            .args(&step.args)
            .current_dir(&step.dir)
            .status();
        let took = began.elapsed().as_secs();
        let line = match status {
            Ok(s) if s.success() => format!("pass  {} ({took} s)", step.name),
            Ok(s) => {
                failed.push(step.name);
                let why = match (s.code(), step.install) {
                    // cargo says "no such command" with 101 where a subcommand is missing.
                    (Some(101), Some(install)) if !installed(&step) => {
                        format!("not installed: {install}")
                    }
                    (Some(code), _) => format!("exit {code}"),
                    (None, _) => "stopped by a signal".to_string(),
                };
                format!("FAIL  {} ({why}, {took} s)", step.name)
            }
            Err(e) => {
                failed.push(step.name);
                format!(
                    "FAIL  {} (could not start {}: {e})",
                    step.name, step.program
                )
            }
        };
        report.push(line);
    }
    let report = report.join("\n");
    if failed.is_empty() {
        Ok(format!("{report}\nevery check passed"))
    } else {
        Err(format!("{report}\n{} of the checks failed", failed.len()))
    }
}

/// Whether a cargo subcommand a step runs is there at all.
fn installed(step: &Step) -> bool {
    let Some(sub) = step.args.first() else {
        return true;
    };
    Command::new(step.program)
        .args([sub, "--version"])
        .current_dir(&step.dir)
        .output()
        .is_ok_and(|o| o.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gate_holds_formatting_lints_licences_and_tests_of_both_workspaces() {
        let root = Path::new("/repo");
        let names: Vec<_> = steps(root).iter().map(|s| s.name).collect();
        for wanted in [
            "desktop: formatting",
            "browser: formatting",
            "desktop: lints (warnings refused)",
            "browser: lints (warnings refused)",
            "desktop: licences, sources and duplicate versions",
            "desktop: tests",
            "browser: tests",
        ] {
            assert!(
                names.contains(&wanted),
                "{wanted} is not in the gate: {names:?}"
            );
        }
        // The cheap checks come before the tests.
        let tests = names.iter().position(|n| *n == "desktop: tests").unwrap();
        let lints = names
            .iter()
            .position(|n| *n == "desktop: lints (warnings refused)")
            .unwrap();
        assert!(lints < tests);
    }
}
