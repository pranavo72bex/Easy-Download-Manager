// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The crate list, and a release's signed digests checked (gaps row W21).

use super::*;
use crate::tests::Scratch;
use serde_json::json;

const IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

fn package(name: &str, version: &str, source: Value, manifest: &str) -> Value {
    json!({
        "name": name, "version": version, "id": format!("{name} {version}"),
        "source": source, "manifest_path": manifest,
    })
}

fn edge(pkg: &str, kinds: &[Option<&str>]) -> Value {
    let kinds: Vec<Value> = kinds.iter().map(|k| json!({ "kind": k })).collect();
    json!({ "pkg": pkg, "dep_kinds": kinds })
}

/// The app and the extension, a crates.io crate both use, one only a build script uses,
/// one only tests use, one the graph reaches only through a feature tests turn on, a
/// vendored copy, a git crate and a member nothing shipped needs.
fn metadata() -> Value {
    let w = r"C:\src\EDM";
    json!({
        "workspace_root": w,
        "workspace_members": ["edm-gpui 0.10.0", "edm-catcher 3.5.0", "edm-test-support 0.1.0", "xtask 0.10.0"],
        "packages": [
            package("edm-gpui", "0.10.0", Value::Null, &format!(r"{w}\crates\edm-gpui\Cargo.toml")),
            package("edm-catcher", "3.5.0", Value::Null, &format!(r"{w}\crates\edm-catcher\Cargo.toml")),
            package("edm-test-support", "0.1.0", Value::Null, &format!(r"{w}\crates\edm-test-support\Cargo.toml")),
            package("xtask", "0.10.0", Value::Null, &format!(r"{w}\tooling\xtask\Cargo.toml")),
            package("serde", "1.0.200", json!(IO), "x"),
            package("cc", "1.2.0", json!(IO), "x"),
            package("wiremock", "0.6.0", json!(IO), "x"),
            package("proptest", "1.12.0", json!(IO), "x"),
            // As cargo gives a path met through a workspace named by its `\\?\` path.
            package("librqbit", "8.1.1", Value::Null, &format!(r"\\?\{w}\vendor\librqbit\Cargo.toml")),
            package("gitdep", "0.2.0", json!("git+https://example.org/g.git?rev=abc#abcdef0"), "x"),
        ],
        "resolve": { "nodes": [
            { "id": "edm-gpui 0.10.0", "deps": [
                edge("serde 1.0.200", &[None]),
                edge("cc 1.2.0", &[Some("build")]),
                edge("edm-test-support 0.1.0", &[Some("dev")]),
                edge("librqbit 8.1.1", &[None]),
                edge("proptest 1.12.0", &[None]),
            ]},
            { "id": "edm-catcher 3.5.0", "deps": [
                edge("serde 1.0.200", &[None, Some("dev")]),
                edge("gitdep 0.2.0", &[None]),
            ]},
            { "id": "edm-test-support 0.1.0", "deps": [edge("wiremock 0.6.0", &[None])] },
            { "id": "xtask 0.10.0", "deps": [] },
            { "id": "serde 1.0.200", "deps": [] },
            { "id": "cc 1.2.0", "deps": [] },
            { "id": "wiremock 0.6.0", "deps": [] },
            { "id": "proptest 1.12.0", "deps": [] },
            { "id": "librqbit 8.1.1", "deps": [] },
            { "id": "gitdep 0.2.0", "deps": [] },
        ]},
    })
}

/// What `cargo tree` prints for [`metadata`]: not `wiremock` (a test's), nor `proptest`.
const TREE_OUT: &str = r"edm-gpui v0.10.0 (C:\src\EDM\crates\edm-gpui)
cc v1.2.0
librqbit v8.1.1 (C:\src\EDM\vendor\librqbit)
serde v1.0.200
edm-catcher v3.5.0 (C:\src\EDM\crates\edm-catcher)
gitdep v0.2.0 (https://example.org/g.git?rev=abc#abcdef0)
serde v1.0.200 (*)
";

const LOCK: &str = r#"version = 4

[[package]]
name = "cc"
version = "1.2.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "1111111111111111111111111111111111111111111111111111111111111111"

[[package]]
name = "serde"
version = "1.0.200"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "2222222222222222222222222222222222222222222222222222222222222222"
dependencies = [
 "serde_derive",
]
"#;

#[test]
fn the_crate_list_is_what_the_shipped_programs_are_built_from() {
    let list = crates_list(&metadata(), TREE_OUT, LOCK, "1.98.1").unwrap();
    let lines: Vec<&str> = list.lines().filter(|l| !l.starts_with('#')).collect();
    assert_eq!(
        lines,
        [
            "cc 1.2.0 crates.io 1111111111111111111111111111111111111111111111111111111111111111",
            "edm-catcher 3.5.0 EDM crates/edm-catcher",
            "edm-gpui 0.10.0 EDM crates/edm-gpui",
            "gitdep 0.2.0 git https://example.org/g.git?rev=abc#abcdef0",
            "librqbit 8.1.1 vendored vendor/librqbit",
            "serde 1.0.200 crates.io 2222222222222222222222222222222222222222222222222222222222222222",
        ]
    );
    assert!(list.starts_with("# EDM 0.10.0: "), "{list}");
    assert!(list.contains("Rust 1.98.1"), "{list}");
    // No path of the computer that made it.
    assert!(
        !list.contains(r"C:\src") && !list.contains("C:/src"),
        "{list}"
    );
}

#[test]
fn a_crate_list_is_refused_without_its_programs_or_their_graph() {
    let mut no_app = metadata();
    no_app["workspace_members"] = json!(["edm-catcher 3.5.0"]);
    assert!(crates_list(&no_app, TREE_OUT, LOCK, "1")
        .unwrap_err()
        .contains("edm-gpui"));
    let mut no_graph = metadata();
    no_graph["resolve"] = Value::Null;
    assert!(crates_list(&no_graph, TREE_OUT, LOCK, "1").is_err());
    assert!(crates_list(&metadata(), TREE_OUT, "", "1").is_err());
    // A crate cargo tree builds that the graph does not reach: the two disagree.
    let more = format!("{TREE_OUT}stray v0.1.0\n");
    assert!(crates_list(&metadata(), &more, LOCK, "1")
        .unwrap_err()
        .contains("stray 0.1.0"));
}

#[test]
fn the_real_lock_is_read() {
    let lock = std::fs::read_to_string(crate::root().join("Cargo.lock")).unwrap();
    let sums = lock_checksums(&lock).unwrap();
    assert!(
        sums.keys().any(|(n, _, s)| n == "ring" && s == IO),
        "ring in the lock"
    );
}

/// A release folder of three files, signed by a new key: (folder, public key, private key).
fn release() -> (Scratch, String, String) {
    let dir = Scratch::new("release");
    std::fs::write(dir.0.join("edm_0.10.0_x64-setup.exe"), b"an installer").unwrap();
    std::fs::write(dir.0.join("crates.txt"), b"serde 1.0.200 crates.io\n").unwrap();
    let manifest = br#"{"version":"0.10.0"}"#;
    std::fs::write(dir.0.join("latest.json"), manifest).unwrap();
    let (public, private) = update_key::new_pair().unwrap();
    let signature = update_key::sign(&private, &public, manifest).unwrap();
    std::fs::write(dir.0.join("latest.json.sig"), signature).unwrap();
    write_sums(&dir.0, |text| update_key::sign(&private, &public, text)).unwrap();
    (dir, public, private)
}

#[test]
fn a_signed_release_checks() {
    let (dir, public, _) = release();
    let sums = std::fs::read_to_string(dir.0.join(SUMS)).unwrap();
    assert_eq!(sums.lines().count(), 4, "{sums}");
    assert!(sums.contains("  crates.txt\n") && sums.contains("  latest.json.sig\n"));
    // sha256sum's own digest of "an installer".
    let installer = sha256(&dir.0.join("edm_0.10.0_x64-setup.exe")).unwrap();
    assert_eq!(installer.len(), 64);
    let said = verify(&dir.0, &public).unwrap();
    assert!(said.contains("4 file(s)"), "{said}");
    // A person who downloaded their installer alone, beside other things of theirs.
    for name in ["crates.txt", "latest.json", "latest.json.sig"] {
        std::fs::remove_file(dir.0.join(name)).unwrap();
    }
    std::fs::write(dir.0.join("notes.txt"), b"mine").unwrap();
    let said = verify(&dir.0, &public).unwrap();
    assert!(
        said.contains("1 file(s)") && said.contains("notes.txt"),
        "{said}"
    );
}

#[test]
fn a_changed_file_is_refused() {
    let (dir, public, _) = release();
    std::fs::write(dir.0.join("edm_0.10.0_x64-setup.exe"), b"another installer").unwrap();
    let why = verify(&dir.0, &public).unwrap_err();
    assert!(why.contains("edm_0.10.0_x64-setup.exe"), "{why}");
}

#[test]
fn a_changed_digest_line_is_refused() {
    let (dir, public, _) = release();
    // The list rewritten to the changed file's digest: its signature no longer holds.
    std::fs::write(dir.0.join("edm_0.10.0_x64-setup.exe"), b"another installer").unwrap();
    let path = dir.0.join(SUMS);
    let sums = std::fs::read_to_string(&path).unwrap();
    let new = sha256(&dir.0.join("edm_0.10.0_x64-setup.exe")).unwrap();
    let changed: String = sums
        .lines()
        .map(|l| match l.split_once("  ") {
            Some((_, "edm_0.10.0_x64-setup.exe")) => format!("{new}  edm_0.10.0_x64-setup.exe\n"),
            _ => format!("{l}\n"),
        })
        .collect();
    assert_ne!(changed, sums);
    std::fs::write(&path, changed).unwrap();
    let why = verify(&dir.0, &public).unwrap_err();
    assert!(why.contains("not signed by the project's key"), "{why}");
}

#[test]
fn another_key_is_refused() {
    let (dir, _, _) = release();
    let (other, _) = update_key::new_pair().unwrap();
    assert!(verify(&dir.0, &other)
        .unwrap_err()
        .contains("not signed by the project's key"));
    assert!(verify(&dir.0, "not a key").is_err());
    std::fs::remove_file(dir.0.join(SUMS_SIG)).unwrap();
    assert!(verify(&dir.0, &other).is_err());
}

#[test]
fn a_list_naming_a_file_outside_the_folder_is_refused() {
    let (dir, public, private) = release();
    let text = format!("{}  ../secret\n", "0".repeat(64));
    let signature = update_key::sign(&private, &public, text.as_bytes()).unwrap();
    std::fs::write(dir.0.join(SUMS), &text).unwrap();
    std::fs::write(dir.0.join(SUMS_SIG), signature).unwrap();
    assert!(verify(&dir.0, &public)
        .unwrap_err()
        .contains("not `<sha-256>  <name>`"));
}

#[test]
fn a_folder_inside_the_release_is_refused_when_signing() {
    let dir = Scratch::new("release-nested");
    std::fs::create_dir(dir.0.join("inner")).unwrap();
    assert!(sums(&dir.0).unwrap_err().contains("not a file"));
    let empty = Scratch::new("release-empty");
    assert!(sums(&empty.0).unwrap_err().contains("no files"));
}
