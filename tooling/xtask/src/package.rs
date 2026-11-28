// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `cargo xtask package`: this system's installers, from any folder of the checkout, the
//! repository's root included (gaps row X4).
//!
//! cargo-packager reads its settings from `[package.metadata.packager]` in the window's
//! `Cargo.toml`, and its `beforePackagingCommand`, icons and out folder are written for that
//! crate's folder: run anywhere else (the root has no `Cargo.toml`) it found no configuration,
//! and run in the workspace without `-p` it packaged every crate that has one. This runs it in
//! that folder, for that package, with this system's formats, so no one has to know.
//!
//! Anything after `package` goes to cargo-packager as it is (`--formats nsis`, `--target …`,
//! `--out-dir …`). With `CARGO_TARGET_DIR` set, the packages go to `<it>/release/bundle`, beside
//! the build, rather than into the checkout.
//!
//! On Linux alone the .rpm follows, made from the same build by `scripts/linux/nfpm.sh` as CI's
//! and the Makefile's are, when no `--formats` or `--target` was named; elsewhere a line says it
//! was not made. A failed program ends xtask with its exit code (gaps row X7).

use std::path::{Path, PathBuf};
use std::process::Command;

/// The package made: the window EDM ships.
const PACKAGE: &str = "edm-gpui";

/// The formats made where none are asked for: what CI and the Makefile make on each system.
fn default_formats(os: &str) -> &'static str {
    match os {
        "windows" => "nsis",
        "macos" => "app,dmg",
        _ => "deb,appimage",
    }
}

/// Whether `args` already says `long` (as `--long`, `--long=…`) or `short`.
fn given(args: &[String], long: &str, short: &str) -> bool {
    args.iter()
        .any(|a| a == long || a == short || a.starts_with(&format!("{long}=")))
}

/// cargo's arguments for this system (`os`), with what the caller gave (`extra`) and the build's
/// own folder (`target_dir`, from `CARGO_TARGET_DIR`).
fn arguments(os: &str, extra: &[String], target_dir: Option<&Path>) -> Vec<String> {
    let mut args: Vec<String> = ["packager", "--release", "-p", PACKAGE]
        .iter()
        .map(|s| s.to_string())
        .collect();
    if !given(extra, "--formats", "-f") {
        args.extend(["--formats".to_string(), default_formats(os).to_string()]);
    }
    if let Some(dir) = target_dir.filter(|_| !given(extra, "--out-dir", "-o")) {
        args.extend([
            "--out-dir".to_string(),
            dir.join("release")
                .join("bundle")
                .to_string_lossy()
                .into_owned(),
        ]);
    }
    args.extend(extra.iter().cloned());
    args
}

/// The folder cargo-packager is run in: the shipped window's crate, whose `Cargo.toml` holds its
/// settings.
fn crate_dir(root: &Path) -> PathBuf {
    root.join("crates").join(PACKAGE)
}

/// Makes the installers. `root` is the repository's root.
pub(crate) fn run(root: &Path, extra: &[String]) -> Result<String, String> {
    let dir = crate_dir(root);
    let installed = Command::new("cargo")
        .args(["packager", "--version"])
        .current_dir(&dir)
        .output()
        .is_ok_and(|o| o.status.success());
    if !installed {
        return Err("needs cargo-packager: cargo install cargo-packager --locked".to_string());
    }
    let target_dir = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from);
    let args = arguments(std::env::consts::OS, extra, target_dir.as_deref());
    let mut cargo = Command::new("cargo");
    cargo.args(&args).current_dir(&dir);
    if std::env::consts::OS == "macos" {
        // Skips the Finder AppleScript that lays out the .dmg window, which fails from a shell
        // that may not script Finder (as the Makefile's `app`).
        cargo.env("CI", "true");
    }
    // Without the building computer's paths in what ships (scripts/release-flags.sh); not on
    // Windows, whose target flags these would not replace.
    if let Some(flags) = crate::tasks::release_flags(root)? {
        cargo.env("CARGO_BUILD_RUSTFLAGS", flags);
    }
    println!("== in {}: cargo {}", dir.display(), args.join(" "));
    let status = cargo
        .status()
        .map_err(|e| format!("could not start cargo: {e}"))?;
    if !status.success() {
        crate::tasks::exit_with("cargo packager", status);
    }
    let out = out_dir(extra, target_dir.as_deref(), &dir)
        .unwrap_or_else(|| root.join("target").join("release").join("bundle"));
    rpm(root, extra, &out)?;
    Ok(format!("{PACKAGE}'s packages made, in {}", out.display()))
}

/// The folder the packages go to: the caller's `--out-dir` (against `dir`, where the packager
/// runs) or the build's own; `None` for the settings' `outDir`.
fn out_dir(extra: &[String], target_dir: Option<&Path>, dir: &Path) -> Option<PathBuf> {
    let mut given_dir = None;
    for (i, a) in extra.iter().enumerate() {
        if let Some(value) = a.strip_prefix("--out-dir=") {
            given_dir = Some(value.to_string());
        } else if a == "--out-dir" || a == "-o" {
            given_dir = extra.get(i + 1).cloned();
        }
    }
    match given_dir {
        Some(out) => Some(dir.join(out)),
        None => target_dir.map(|t| t.join("release").join("bundle")),
    }
}

/// The .rpm's command line on `os`, from `binary` at `version` into `out`: Linux's alone, and only
/// where the caller named no formats or target of their own.
fn rpm_line(
    os: &str,
    extra: &[String],
    binary: &Path,
    version: &str,
    out: &Path,
) -> Option<Vec<String>> {
    if os != "linux" || given(extra, "--formats", "-f") || given(extra, "--target", "--target") {
        return None;
    }
    Some(vec![
        "sh".to_string(),
        "scripts/linux/nfpm.sh".to_string(),
        binary.to_string_lossy().into_owned(),
        version.to_string(),
        out.to_string_lossy().into_owned(),
    ])
}

/// The .rpm, on Linux, of the build just packaged, into `out`.
fn rpm(root: &Path, extra: &[String], out: &Path) -> Result<(), String> {
    let manifest = crate::read(&crate_dir(root).join("Cargo.toml"))?;
    let parsed: toml::Table = manifest.parse().map_err(|e| format!("{PACKAGE}: {e}"))?;
    let version = parsed
        .get("package")
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("{PACKAGE}'s Cargo.toml names no version"))?;
    let binary = crate::tasks::target_dir(root).join("release").join(PACKAGE);
    let os = std::env::consts::OS;
    let Some(line) = rpm_line(os, extra, &binary, version, out) else {
        if os == "linux" {
            println!("(the .rpm is not made where --formats or --target is named)");
        } else {
            println!("(the .rpm is Linux's alone: not made here)");
        }
        return Ok(());
    };
    println!("== in {}: {}", root.display(), line.join(" "));
    let status = Command::new(&line[0])
        .args(&line[1..])
        .current_dir(root)
        .status()
        .map_err(|e| format!("could not start sh: {e}"))?;
    if !status.success() {
        crate::tasks::exit_with("scripts/linux/nfpm.sh", status);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn each_system_makes_its_own_formats_of_the_shipped_window_alone() {
        for (os, formats) in [
            ("windows", "nsis"),
            ("macos", "app,dmg"),
            ("linux", "deb,appimage"),
        ] {
            assert_eq!(
                arguments(os, &[], None),
                strings(&[
                    "packager",
                    "--release",
                    "-p",
                    "edm-gpui",
                    "--formats",
                    formats
                ])
            );
        }
    }

    #[test]
    fn what_the_caller_gives_is_passed_on_and_not_said_twice() {
        let extra = strings(&["--formats", "nsis", "-o", "out"]);
        let args = arguments("windows", &extra, Some(Path::new("D:/t")));
        assert_eq!(args.iter().filter(|a| *a == "--formats").count(), 1);
        assert!(!args.contains(&"--out-dir".to_string()));
        assert!(args.ends_with(&extra));
        let args = arguments("linux", &strings(&["--formats=deb"]), None);
        assert!(!args.contains(&"--formats".to_string()));
    }

    #[test]
    fn the_rpm_is_made_on_linux_alone_and_only_with_the_default_formats() {
        let (bin, out, dir) = (Path::new("b"), Path::new("o"), Path::new("crate"));
        let line = rpm_line("linux", &[], bin, "1", out).expect("Linux makes the .rpm");
        assert_eq!(line.join(" "), "sh scripts/linux/nfpm.sh b 1 o");
        for os in ["windows", "macos"] {
            assert!(rpm_line(os, &[], bin, "1", out).is_none(), "{os}");
        }
        for flag in ["-f", "--target"] {
            assert!(rpm_line("linux", &strings(&[flag, "x"]), bin, "1", out).is_none());
        }
        let given = out_dir(&strings(&["-o", "o"]), Some(Path::new("t")), dir);
        assert_eq!(given, Some(dir.join("o")));
    }

    #[test]
    fn with_a_target_dir_the_packages_go_beside_the_build() {
        let args = arguments("windows", &[], Some(Path::new("D:/edm-target")));
        let at = args
            .iter()
            .position(|a| a == "--out-dir")
            .expect("--out-dir");
        assert_eq!(
            PathBuf::from(&args[at + 1]),
            Path::new("D:/edm-target").join("release").join("bundle")
        );
    }

    /// The folder it runs in is the one whose `Cargo.toml` holds the packager's settings.
    #[test]
    fn the_folder_it_runs_in_holds_the_packagers_settings() {
        let root = crate::root();
        let manifest = std::fs::read_to_string(crate_dir(&root).join("Cargo.toml"))
            .expect("the shipped window's Cargo.toml");
        let parsed: toml::Table = manifest.parse().expect("its Cargo.toml parses");
        let packager = parsed
            .get("package")
            .and_then(|p| p.get("metadata"))
            .and_then(|m| m.get("packager"))
            .expect("[package.metadata.packager] in edm-gpui's Cargo.toml");
        assert!(packager.get("beforePackagingCommand").is_some());
    }

    /// `cargo xtask` is found from the repository's root too, not only from the repository's root.
    #[test]
    fn cargo_xtask_is_found_from_the_root() {
        let root = crate::root();
        let config = std::fs::read_to_string(root.join(".cargo").join("config.toml"))
            .expect("the root's .cargo/config.toml");
        let parsed: toml::Table = config.parse().expect("it parses");
        let alias = parsed
            .get("alias")
            .and_then(|a| a.get("xtask"))
            .and_then(|x| x.as_str())
            .expect("an xtask alias");
        assert!(alias.contains("-p xtask"), "{alias}");
    }
}
