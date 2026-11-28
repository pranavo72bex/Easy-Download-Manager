// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What a release says about itself, so a person can check it (gaps row W21).
//!
//! - `cargo xtask crates-list <file>` writes every crate the release's programs are built
//!   from: the app (`edm-gpui`) and the browser extension (`edm-catcher`), every system's
//!   together, with the crates that build them (build scripts, macros) and without the ones
//!   only tests use (`cargo tree` says which a build compiles, `cargo metadata` where each
//!   comes from). Each with its version and where it comes from (crates.io with the
//!   digest `Cargo.lock` pins, a git repository, a copy vendored here, or EDM's own).
//! - `cargo xtask sign-release <dir>` writes `SHA256SUMS` (the SHA-256 of every file in the
//!   release's folder, in `sha256sum`'s own format) and `SHA256SUMS.sig`, its signature by
//!   the update manifest's key (F6, `update_key.rs`), refused unless that is the key EDM
//!   carries.
//! - `cargo xtask verify-release <dir>` checks a downloaded release: the signature against
//!   the key EDM carries, then each file present against its digest.

use crate::update_key;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::io::Read as _;
use std::path::Path;

/// The release's list of digests, and its signature beside it.
pub const SUMS: &str = "SHA256SUMS";
pub const SUMS_SIG: &str = "SHA256SUMS.sig";

/// The crates whose programs a release ships: the app's window and the browser extension.
pub const SHIPPED: [&str; 2] = ["edm-gpui", "edm-catcher"];

const CRATES_IO: [&str; 2] = [
    "registry+https://github.com/rust-lang/crates.io-index",
    "sparse+https://index.crates.io/",
];

/// The `cargo tree` that names what a build of the shipped crates compiles: no dev-dependency,
/// and features resolved as a build without tests resolves them (`cargo metadata` alone
/// unifies the features tests turn on, and listed a benchmark harness and a fuzzer), for
/// every system at once.
pub const TREE: [&str; 13] = [
    "tree",
    "--locked",
    "-e",
    "normal,build",
    "--target",
    "all",
    "-p",
    SHIPPED[0],
    "-p",
    SHIPPED[1],
    "--prefix",
    "none",
    "--format={p}",
];

/// Each crate and version [`TREE`] printed: lines `name vX.Y.Z`, then perhaps its folder or
/// repository in brackets, `(proc-macro)` and `(*)` for one already shown.
pub fn built(tree: &str) -> BTreeSet<(String, String)> {
    tree.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let name = words.next()?;
            let version = words.next()?.strip_prefix('v')?;
            Some((name.to_string(), version.to_string()))
        })
        .collect()
}

/// The list `crates-list` writes: the crates [`TREE`] printed (`tree`), each said with its
/// source from `cargo metadata`'s JSON (`metadata`) and, for crates.io's, the digest
/// `Cargo.lock` (`lock`) pins. `rust` is the toolchain it is built with.
pub fn crates_list(metadata: &Value, tree: &str, lock: &str, rust: &str) -> Result<String, String> {
    let packages = metadata["packages"]
        .as_array()
        .ok_or("cargo metadata gave no packages")?;
    let by_id: HashMap<&str, &Value> = packages
        .iter()
        .filter_map(|p| Some((p["id"].as_str()?, p)))
        .collect();
    let members: BTreeSet<&str> = metadata["workspace_members"]
        .as_array()
        .ok_or("cargo metadata gave no workspace members")?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let nodes: HashMap<&str, &Value> = metadata["resolve"]["nodes"]
        .as_array()
        .ok_or("cargo metadata gave no resolved graph (was it run with --no-deps?)")?
        .iter()
        .filter_map(|n| Some((n["id"].as_str()?, n)))
        .collect();

    // From each shipped crate, along every edge that is not a test's alone, and of those
    // the ones a build compiles.
    let built = built(tree);
    let mut todo = Vec::new();
    for name in SHIPPED {
        let id = members
            .iter()
            .find(|id| by_id.get(*id).is_some_and(|p| p["name"] == name))
            .ok_or(format!("{name} is not a member of the workspace"))?;
        todo.push(*id);
    }
    let mut seen = BTreeSet::new();
    while let Some(id) = todo.pop() {
        if !seen.insert(id) {
            continue;
        }
        let node = nodes
            .get(id)
            .ok_or(format!("{id} is not in the resolved graph"))?;
        for dep in node["deps"].as_array().into_iter().flatten() {
            let shipped = dep["dep_kinds"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|k| k["kind"].as_str() != Some("dev"));
            let Some(pkg) = dep["pkg"].as_str() else {
                continue;
            };
            let compiled = by_id.get(pkg).is_some_and(|p| {
                let key = (
                    p["name"].as_str().unwrap_or_default().to_string(),
                    p["version"].as_str().unwrap_or_default().to_string(),
                );
                built.contains(&key)
            });
            if shipped && compiled {
                todo.push(pkg);
            }
        }
    }

    let checksums = lock_checksums(lock)?;
    // Paths are said from the repository's root, which is the workspace's.
    let repository = slashes(metadata["workspace_root"].as_str().unwrap_or_default());
    let (mut lines, mut listed) = (BTreeSet::new(), BTreeSet::new());
    for id in seen {
        let p = by_id.get(id).ok_or(format!("{id} is in no package"))?;
        let (name, version) = (
            p["name"].as_str().unwrap_or_default(),
            p["version"].as_str().unwrap_or_default(),
        );
        let from = match p["source"].as_str() {
            Some(s) if CRATES_IO.contains(&s) => {
                match checksums.get(&(name.to_string(), version.to_string(), s.to_string())) {
                    Some(sum) => format!("crates.io {sum}"),
                    None => "crates.io".to_string(),
                }
            }
            Some(s) => match s.strip_prefix("git+") {
                Some(git) => format!("git {git}"),
                None => format!("registry {s}"),
            },
            None => {
                let manifest = slashes(p["manifest_path"].as_str().unwrap_or_default());
                let folder = manifest.strip_suffix("/Cargo.toml").unwrap_or(&manifest);
                let here = folder
                    .strip_prefix(&format!("{repository}/"))
                    .unwrap_or(name);
                let what = if members.contains(&id) {
                    "EDM"
                } else {
                    "vendored"
                };
                format!("{what} {here}")
            }
        };
        lines.insert(format!("{name} {version} {from}"));
        listed.insert((name.to_string(), version.to_string()));
    }
    // The two of cargo's answers agree, or the list is not believed.
    let missing: Vec<String> = built
        .difference(&listed)
        .map(|(n, v)| format!("{n} {v}"))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "cargo tree builds crates the dependency graph does not reach: {}",
            missing.join(", ")
        ));
    }
    let version = by_id
        .values()
        .find(|p| p["name"] == SHIPPED[0] && p["source"].is_null())
        .and_then(|p| p["version"].as_str())
        .unwrap_or("?");
    let mut out = format!(
        "# EDM {version}: every crate in the app ({}) and the browser extension ({}), every\n\
         # system's together, with the crates that build them (build scripts, macros); those\n\
         # only tests use, and the features only tests turn on, are left out. Made by\n\
         # `cargo xtask crates-list` from Cargo.lock (`cargo {}`);\n\
         # built with Rust {rust} (rust-toolchain.toml). Each line: name, version, where it is\n\
         # from (crates.io and the SHA-256 Cargo.lock pins for it; git; vendored or EDM's own,\n\
         # with its folder in the repository).\n",
        SHIPPED[0],
        SHIPPED[1],
        TREE.join(" ")
    );
    for line in lines {
        out.push_str(&line);
        out.push('\n');
    }
    Ok(out)
}

/// `Cargo.lock`'s checksum of each package, by name, version and source.
fn lock_checksums(lock: &str) -> Result<HashMap<(String, String, String), String>, String> {
    let lock: toml::Table = lock.parse().map_err(|e| format!("Cargo.lock: {e}"))?;
    let packages = lock
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or("Cargo.lock has no packages")?;
    let field = |p: &toml::Value, key: &str| p.get(key)?.as_str().map(str::to_string);
    Ok(packages
        .iter()
        .filter_map(|p| {
            let key = (field(p, "name")?, field(p, "version")?, field(p, "source")?);
            Some((key, field(p, "checksum")?))
        })
        .collect())
}

/// `path` with forward slashes and without Windows' `\\?\` prefix, which `cargo metadata`
/// gives some paths and not others when the workspace is named by such a path.
fn slashes(path: &str) -> String {
    let path = path.replace('\\', "/");
    let path = path.strip_prefix("//?/").unwrap_or(&path);
    path.trim_end_matches('/').to_string()
}

/// `cargo xtask crates-list <file>`: the list for this checkout, from `cargo metadata --locked`.
pub fn write_crates_list(root: &Path, file: Option<&str>) -> Result<String, String> {
    let file = file.ok_or("usage: cargo xtask crates-list <file>")?;
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = std::process::Command::new(&cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--manifest-path",
        ])
        .arg(root.join("Cargo.toml"))
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let metadata: Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("cargo metadata: {e}"))?;
    let tree = std::process::Command::new(&cargo)
        .args(TREE)
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .output()
        .map_err(|e| format!("cargo tree: {e}"))?;
    if !tree.status.success() {
        return Err(format!(
            "cargo tree failed: {}",
            String::from_utf8_lossy(&tree.stderr)
        ));
    }
    let tree = String::from_utf8_lossy(&tree.stdout);
    let lock = crate::read(&root.join("Cargo.lock"))?;
    let toolchain = crate::read(&root.join("rust-toolchain.toml"))?;
    let rust = toolchain
        .parse::<toml::Table>()
        .ok()
        .and_then(|t| {
            t.get("toolchain")?
                .get("channel")?
                .as_str()
                .map(str::to_string)
        })
        .ok_or("rust-toolchain.toml names no channel")?;
    let list = crates_list(&metadata, &tree, &lock, &rust)?;
    std::fs::write(file, &list).map_err(|e| format!("{file}: {e}"))?;
    let count = list.lines().filter(|l| !l.starts_with('#')).count();
    Ok(format!("{count} crates listed in {file}"))
}

/// The SHA-256 of `path`'s bytes, in lower-case hex.
fn sha256(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut context = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = vec![0; 1 << 16];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        context.update(&buffer[..n]);
    }
    Ok(context
        .finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// The names of the release's files in `dir`, sorted, without the digests and their
/// signature. A folder inside it is refused: a release is one flat folder of files, and a
/// file below it would not be covered.
fn release_files(dir: &Path) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !entry.path().is_file() {
            return Err(format!("{name} in {} is not a file", dir.display()));
        }
        if name != SUMS && name != SUMS_SIG {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// `SHA256SUMS`'s text for `dir`: `<digest>  <name>` a line, as `sha256sum` writes and
/// `sha256sum -c` reads.
pub fn sums(dir: &Path) -> Result<String, String> {
    let mut out = String::new();
    for name in release_files(dir)? {
        out.push_str(&format!("{}  {name}\n", sha256(&dir.join(&name))?));
    }
    if out.is_empty() {
        return Err(format!("{} has no files", dir.display()));
    }
    Ok(out)
}

/// `SHA256SUMS` and `SHA256SUMS.sig` written into `dir`, signed by `sign`.
fn write_sums(
    dir: &Path,
    sign: impl FnOnce(&[u8]) -> Result<String, String>,
) -> Result<String, String> {
    let text = sums(dir)?;
    let signature = sign(text.as_bytes())?;
    std::fs::write(dir.join(SUMS), &text).map_err(|e| format!("{SUMS}: {e}"))?;
    std::fs::write(dir.join(SUMS_SIG), signature).map_err(|e| format!("{SUMS_SIG}: {e}"))?;
    Ok(format!(
        "{} files' digests in {}, signed in {SUMS_SIG}",
        text.lines().count(),
        dir.join(SUMS).display()
    ))
}

/// `cargo xtask sign-release <dir>`, with the key in `EDM_UPDATE_SIGNING_KEY`.
pub fn sign_release(root: &Path, dir: Option<&str>) -> Result<String, String> {
    let dir = dir.ok_or("usage: cargo xtask sign-release <folder of the release's files>")?;
    write_sums(Path::new(dir), |text| update_key::sign_with_env(root, text))
}

/// Checks the release in `dir` against `key` (base64 Ed25519): `SHA256SUMS` signed by it,
/// every file it lists that is here of that digest, at least one of them here, and
/// `latest.json` signed by it when both it and its signature are here. Files it does not
/// list are named but not judged (a person's folder may hold other things).
pub fn verify(dir: &Path, key: &str) -> Result<String, String> {
    let read = |name: &str| {
        std::fs::read(dir.join(name)).map_err(|e| format!("{name}: {e} (in {})", dir.display()))
    };
    let text = read(SUMS)?;
    let signature = String::from_utf8_lossy(&read(SUMS_SIG)?).into_owned();
    if !update_key::signed_by(key, &text, &signature) {
        return Err(format!(
            "{SUMS} is not signed by the project's key ({key}): this is not the release as \
             published, or its list was changed"
        ));
    }
    let text = String::from_utf8(text).map_err(|_| format!("{SUMS} is not text"))?;
    let mut listed = BTreeSet::new();
    let (mut checked, mut absent, mut wrong) = (Vec::new(), Vec::new(), Vec::new());
    for line in text.lines() {
        let (digest, name) = line
            .split_once("  ")
            .filter(|(d, n)| {
                d.len() == 64
                    && d.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    && !n.is_empty()
                    && !n.contains(['/', '\\'])
                    && *n != "."
                    && *n != ".."
            })
            .ok_or(format!(
                "{SUMS} has a line that is not `<sha-256>  <name>`: {line}"
            ))?;
        if !listed.insert(name) {
            return Err(format!("{SUMS} lists {name} twice"));
        }
        let path = dir.join(name);
        if !path.is_file() {
            absent.push(name);
        } else if sha256(&path)? == digest {
            checked.push(name);
        } else {
            wrong.push(name);
        }
    }
    if !wrong.is_empty() {
        return Err(format!(
            "not the published bytes (the digest differs): {}",
            wrong.join(", ")
        ));
    }
    if checked.is_empty() {
        return Err(format!(
            "none of the files {SUMS} lists is in {}",
            dir.display()
        ));
    }
    let present = |n: &str| checked.contains(&n);
    if present("latest.json") && present("latest.json.sig") {
        let signature = String::from_utf8_lossy(&read("latest.json.sig")?).into_owned();
        if !update_key::signed_by(key, &read("latest.json")?, &signature) {
            return Err("latest.json.sig is not the project's signature of latest.json".into());
        }
    }
    let unlisted: Vec<String> = release_files(dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|n| !listed.contains(n.as_str()))
        .collect();
    let mut out = format!(
        "{SUMS} is signed by the project's key; {} file(s) are the published bytes: {}",
        checked.len(),
        checked.join(", ")
    );
    if !absent.is_empty() {
        out.push_str(&format!(
            "\nnot here, so not checked: {}",
            absent.join(", ")
        ));
    }
    if !unlisted.is_empty() {
        out.push_str(&format!(
            "\nnot part of the release (not in {SUMS}): {}",
            unlisted.join(", ")
        ));
    }
    Ok(out)
}

/// `cargo xtask verify-release <dir>`: [`verify`] against the key this checkout of EDM carries.
pub fn verify_release(root: &Path, dir: Option<&str>) -> Result<String, String> {
    let dir = dir.ok_or("usage: cargo xtask verify-release <folder of the release's files>")?;
    verify(Path::new(dir), &update_key::carried(root)?)
}

#[cfg(test)]
mod tests;
