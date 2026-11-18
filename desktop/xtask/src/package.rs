// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
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

use std::path::{Path, PathBuf};
use std::process::Command;

/// The package made: the window dm ships.
const PACKAGE: &str = "dm-gpui";

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
    root.join("desktop").join("crates").join(PACKAGE)
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
    if std::env::consts::OS != "windows" && std::env::var_os("CARGO_BUILD_RUSTFLAGS").is_none() {
        // Without the building computer's paths in what ships (scripts/release-flags.sh).
        // Windows keeps its own target flags (desktop/.cargo/config.toml), which these would not
        // replace, so it is not asked there.
        let flags = Command::new("sh")
            .arg(root.join("scripts").join("release-flags.sh"))
            .output()
            .map_err(|e| format!("sh scripts/release-flags.sh: {e}"))?;
        if !flags.status.success() {
            return Err("sh scripts/release-flags.sh failed".to_string());
        }
        cargo.env(
            "CARGO_BUILD_RUSTFLAGS",
            String::from_utf8_lossy(&flags.stdout).trim(),
        );
    }
    println!("== in {}: cargo {}", dir.display(), args.join(" "));
    let status = cargo
        .status()
        .map_err(|e| format!("could not start cargo: {e}"))?;
    if !status.success() {
        return Err(format!("cargo packager failed ({status})"));
    }
    let out = match (target_dir, given(extra, "--out-dir", "-o")) {
        (Some(dir), false) => dir.join("release").join("bundle"),
        _ => root
            .join("desktop")
            .join("target")
            .join("release")
            .join("bundle"),
    };
    Ok(format!(
        "{PACKAGE}'s packages made (in {} unless --out-dir said otherwise)",
        out.display()
    ))
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
                    "dm-gpui",
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
    fn with_a_target_dir_the_packages_go_beside_the_build() {
        let args = arguments("windows", &[], Some(Path::new("D:/dm-target")));
        let at = args
            .iter()
            .position(|a| a == "--out-dir")
            .expect("--out-dir");
        assert_eq!(
            PathBuf::from(&args[at + 1]),
            Path::new("D:/dm-target").join("release").join("bundle")
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
            .expect("[package.metadata.packager] in dm-gpui's Cargo.toml");
        assert!(packager.get("beforePackagingCommand").is_some());
    }

    /// `cargo xtask` is found from the repository's root too, not only from `desktop/`.
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
        assert!(
            alias.contains("--manifest-path desktop/Cargo.toml") && alias.contains("-p xtask"),
            "{alias}"
        );
    }
}
