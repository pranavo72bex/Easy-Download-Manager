// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `cargo xtask vendor [--version <version>] [<crate>…]` (gaps rows P3, A17, X5 and N16): makes
//! the crates in `vendor/` again, the six of [`CRATES`] or those named. Each is the
//! crate published on crates.io, or for one crates.io has not (gpui-android, from gpui-mobile)
//! GitHub's archive of a commit, fetched with `curl` (Windows 10 and later have it), checked
//! against the SHA-256 held here (ring's), unpacked with `tar`, with the files dm's build does
//! not use removed and `vendor/patches/<crate>.patch` applied with `git apply`, then put
//! in place of the copy. Nothing from the crates is run. The crates are unpacked in
//! `DM_VENDOR_WORK`, a folder of the caller's, or else in a new folder of the system's own,
//! removed after.
//!
//! `--version` makes the named crates from another published version instead, which has no
//! SHA-256 held: the check is skipped, with a warning. The weekly job that tries the next GPUI
//! snapshot uses it (`.github/workflows/vendor-next.yml`); it is not how a version is moved to.
//!
//! To move to another version: change its version and SHA-256 below (crates.io's API lists them,
//! `https://crates.io/api/v1/crates/<name>/versions`, as `checksum`), run this, mend any hunk
//! `git apply` refuses, and write the patch again with `git diff` between the published crate
//! (with the same files removed) and the mended copy, keeping the text at the patch's head. Every
//! `dm(<tag>):` mark is a row of the table in `vendor/README.md` (`vendor_marks.rs`).

use std::path::{Path, PathBuf};
use std::process::Command;

/// One crate dm builds from a patched copy in `vendor/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Vendored {
    /// Its name on crates.io (for a copy made from GitHub, the name dm gives it), which is also
    /// its folder in `vendor/` and its patch's name in `vendor/patches/`.
    pub(crate) name: &'static str,
    /// The published version the copy is made from (from GitHub, the version its manifest says).
    pub(crate) version: &'static str,
    /// The SHA-256 of that version's `.crate`, in lower-case hex, as crates.io lists it; or of
    /// GitHub's archive of the commit.
    pub(crate) sha256: &'static str,
    /// What dm's build does not use, removed from the unpacked crate (paths relative to it).
    pub(crate) remove: &'static [&'static str],
    /// Where it is fetched from.
    pub(crate) source: Source,
}

/// Where a copy's crate comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    /// The `.crate` crates.io serves at its version.
    CratesIo,
    /// GitHub's archive (`.tar.gz`) of one commit of a repository, for a crate crates.io has
    /// not: the archive's folder is `<repository's name>-<commit>`.
    GitHub {
        /// `owner/name`.
        repository: &'static str,
        /// The commit's full SHA-1.
        commit: &'static str,
    },
}

/// The version of librqbit and its SHA-1 wrapper the copies are made from.
pub(crate) const LIBRQBIT_VERSION: &str = "9.1.0";

/// The version of the GPUI copies: the one dm-gpui pins (`gpui-pre =0.3.12` in
/// `Cargo.toml`).
pub(crate) const GPUI_VERSION: &str = "0.3.12";

/// Removed from the librqbit crates: their own lock file (dm's is `Cargo.lock`), cargo's
/// marks, the web interface (the `webui` feature), test torrents, examples and the manifest as
/// written before publishing.
const LIBRQBIT_REMOVE: &[&str] = &[
    "Cargo.lock",
    ".cargo_vcs_info.json",
    ".cargo-ok",
    "webui",
    "resources",
    "examples",
    "Cargo.toml.orig",
];

/// Removed from the GPUI crates: their own lock file and cargo's marks.
const GPUI_REMOVE: &[&str] = &["Cargo.lock", ".cargo_vcs_info.json", ".cargo-ok"];

/// Removed from gpui-mobile for gpui-android (gaps row N16): all but its Android platform. Its
/// iOS platform, components, packages (and their no-op features), platform views and the
/// host-driven entry, the example app and its screenshots, the iOS frame demand and the target
/// list, its CI, notes and README (`Cargo.toml` and `src/lib.rs` say where it came from).
const GPUI_ANDROID_REMOVE: &[&str] = &[
    ".cargo",
    ".github",
    ".gitignore",
    "PACKAGES-TO-IMPL.md",
    "README.md",
    "TODO.md",
    "example",
    "screenshots",
    "src/android/host.rs",
    "src/android/platform_view.rs",
    "src/components",
    "src/frame_demand.rs",
    "src/ios",
    "src/packages",
    "src/platform_view.rs",
    "src/target_platform.rs",
];

/// Every crate dm keeps a patched copy of, in the order they are made when none is named.
pub(crate) const CRATES: [Vendored; 6] = [
    Vendored {
        name: "librqbit",
        version: LIBRQBIT_VERSION,
        sha256: "4e35a06e481e854f69141928d067406397b3ad7a47bfdc095bc31325f44f5d13",
        remove: LIBRQBIT_REMOVE,
        source: Source::CratesIo,
    },
    Vendored {
        name: "librqbit-sha1-wrapper",
        version: LIBRQBIT_VERSION,
        sha256: "4633bfa4c77dd7153be92584947d669a0ff8f61b82bb8bf283ff5f11ddee75be",
        remove: LIBRQBIT_REMOVE,
        source: Source::CratesIo,
    },
    Vendored {
        name: "gpui-pre-linux",
        version: GPUI_VERSION,
        sha256: "54bab418d0b7c027af9495f278619b46887362b3bd6446e246c87c5b0fa167df",
        remove: GPUI_REMOVE,
        source: Source::CratesIo,
    },
    Vendored {
        name: "gpui-pre-windows",
        version: GPUI_VERSION,
        sha256: "7201762420222d5b5ce5bdf0056996199212a3349bfb2457a3a0238c688681f3",
        remove: GPUI_REMOVE,
        source: Source::CratesIo,
    },
    Vendored {
        name: "gpui-pre-wgpu",
        version: GPUI_VERSION,
        sha256: "dc76608d385f6d0dcbad44504b1bf669dac3b7d7ffa2885acdb7c1c358caf852",
        remove: GPUI_REMOVE,
        source: Source::CratesIo,
    },
    Vendored {
        name: "gpui-android",
        version: "0.1.0",
        sha256: "fb062e5ac28e866edf98018020e657d2b4e92677dbba1e84c9e975f8a18656ac",
        remove: GPUI_ANDROID_REMOVE,
        source: Source::GitHub {
            repository: "itsbalamurali/gpui-mobile",
            commit: "5fd54f4554263ae06753e90472c7f2e2ff5d7495",
        },
    },
];

/// The copies and their patches, from the repository's root.
const VENDOR: &str = "vendor";

/// What `cargo xtask vendor` was asked to do.
#[derive(Debug, PartialEq, Eq)]
struct Request {
    /// The version named with `--version`, in place of each crate's own.
    version: Option<String>,
    /// The crates to make again.
    crates: Vec<Vendored>,
}

/// `cargo xtask vendor`: `args` are the words after `vendor`; `root` is the repository's root.
pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, String> {
    let request = parse(args)?;
    let root = crate::tasks::plain(root);
    let (work, own) = work_dir()?;
    let mut result = Ok(());
    for c in &request.crates {
        result = remake(&root, &work, c, request.version.as_deref());
        if result.is_err() {
            break;
        }
    }
    if own {
        let _ = std::fs::remove_dir_all(&work);
    }
    result?;
    Ok(format!(
        "{} of vendor made again from crates.io or GitHub: `git status` shows what changed",
        request.crates.len()
    ))
}

/// `args` read: `--version <version>` (or `--version=<version>`) and the crates' names.
fn parse(args: &[String]) -> Result<Request, String> {
    let mut version = None;
    let mut crates = Vec::new();
    let mut words = args.iter();
    while let Some(word) = words.next() {
        let value = if word == "--version" {
            Some(
                words
                    .next()
                    .ok_or("--version needs a version after it")?
                    .clone(),
            )
        } else {
            word.strip_prefix("--version=").map(str::to_string)
        };
        if let Some(value) = value {
            if !plain_version(&value) {
                return Err(format!(
                    "--version {value}: not a version as crates.io writes one"
                ));
            }
            version = Some(value);
        } else if word.starts_with('-') {
            return Err(format!("vendor: no option {word}\n{}", usage()));
        } else {
            let found = CRATES.iter().find(|c| c.name == word.as_str());
            crates.push(*found.ok_or_else(|| format!("vendor: no crate {word}\n{}", usage()))?);
        }
    }
    if crates.is_empty() {
        if version.is_some() {
            return Err(format!(
                "--version names the crates it is for: the six are not one version\n{}",
                usage()
            ));
        }
        crates = CRATES.to_vec();
    }
    if version.is_some() {
        if let Some(c) = crates.iter().find(|c| c.source != Source::CratesIo) {
            return Err(format!(
                "--version: {} is made from a commit, not a published version",
                c.name
            ));
        }
    }
    Ok(Request { version, crates })
}

/// What `vendor` takes, said when it is given something else.
fn usage() -> String {
    let names: Vec<&str> = CRATES.iter().map(|c| c.name).collect();
    format!(
        "usage: cargo xtask vendor [--version <version>] [<crate>...], the crates of {}",
        names.join(", ")
    )
}

/// Whether `version` is written as crates.io writes one (`0.3.12`, `1.0.0-rc.1+build`): it goes
/// into an address and a file name.
fn plain_version(version: &str) -> bool {
    !version.is_empty()
        && version.starts_with(|c: char| c.is_ascii_digit())
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
}

/// Where `c` is fetched from at `version`, and the name of the file it is saved as.
fn fetched(c: &Vendored, version: &str) -> (String, String) {
    match c.source {
        Source::CratesIo => (
            crate_url(c.name, version),
            format!("{}-{version}.crate", c.name),
        ),
        Source::GitHub { repository, commit } => (
            github_url(repository, commit),
            format!("{}-{commit}.tar.gz", c.name),
        ),
    }
}

/// The folder `c`'s archive holds, at `version`.
fn unpacked(c: &Vendored, version: &str) -> String {
    match c.source {
        Source::CratesIo => format!("{}-{version}", c.name),
        Source::GitHub { repository, commit } => {
            let name = repository.rsplit('/').next().unwrap_or(repository);
            format!("{name}-{commit}")
        }
    }
}

/// Where GitHub serves its archive of `repository` (`owner/name`) at `commit`.
pub(crate) fn github_url(repository: &str, commit: &str) -> String {
    format!("https://codeload.github.com/{repository}/tar.gz/{commit}")
}

/// Where crates.io serves `name`'s `.crate` at `version`.
pub(crate) fn crate_url(name: &str, version: &str) -> String {
    format!("https://static.crates.io/crates/{name}/{name}-{version}.crate")
}

/// The SHA-256 of `bytes`, in lower-case hex.
fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest.as_ref() {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// The folder the crates are unpacked in, and whether it is this run's own (removed after):
/// `DM_VENDOR_WORK`, made absolute, or a new one in the system's temporary folder.
fn work_dir() -> Result<(PathBuf, bool), String> {
    let (dir, own) = match std::env::var_os("DM_VENDOR_WORK").filter(|d| !d.is_empty()) {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            let here = std::env::current_dir().map_err(|e| format!("this folder: {e}"))?;
            (here.join(dir), false)
        }
        None => {
            let dir = std::env::temp_dir().join(format!("dm-vendor-{}", std::process::id()));
            remove_path(&dir)?;
            (dir, true)
        }
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok((dir, own))
}

/// Makes `c` again in `root`'s `vendor/`, from `version` where one is named (unchecked)
/// or else from its own (checked), unpacking it in `work`.
fn remake(root: &Path, work: &Path, c: &Vendored, version: Option<&str>) -> Result<(), String> {
    let version = version.unwrap_or(c.version);
    let checked = version == c.version;
    if !checked {
        println!(
            "warning: {} {version} is not the version held ({}): no SHA-256 is held for it, \
             so it is not checked",
            c.name, c.version
        );
    }
    let (url, file) = fetched(c, version);
    println!("== {url}");
    succeed(
        Command::new("curl")
            .args(["-sSfL", &url, "-o", &file])
            .current_dir(work),
        &format!("curl {url}"),
    )?;
    let at = work.join(&file);
    let bytes = std::fs::read(&at).map_err(|e| format!("{}: {e}", at.display()))?;
    let got = sha256_hex(&bytes);
    if checked && got != c.sha256 {
        return Err(format!(
            "{} {version}: SHA-256 {got}, expected {}",
            c.name, c.sha256
        ));
    }
    if !checked {
        println!("({} {version}: SHA-256 {got})", c.name);
    }
    let folder = unpacked(c, version);
    let dir = work.join(&folder);
    remove_path(&dir)?;
    // A name relative to `work`, never a drive's path: GNU tar (Git Bash's) reads `C:` as a host.
    succeed(
        Command::new("tar").args(["-xzf", &file]).current_dir(work),
        &format!("tar -xzf {file}"),
    )?;
    if !dir.is_dir() {
        return Err(format!("{file} held no folder {folder}"));
    }
    for path in c.remove {
        remove_path(&dir.join(path))?;
    }
    let patch = root
        .join(VENDOR)
        .join("patches")
        .join(format!("{}.patch", c.name));
    // The ceiling keeps git from taking a repository above `work` for the patch's own (it would
    // then apply only the paths under this folder).
    succeed(
        Command::new("git")
            .args(["-c", "core.autocrlf=false", "apply", "--whitespace=nowarn"])
            .arg(&patch)
            .env("GIT_CEILING_DIRECTORIES", work)
            .current_dir(&dir),
        &format!("git apply {}", patch.display()),
    )?;
    let copy = root.join(VENDOR).join(c.name);
    remove_path(&copy)?;
    copy_tree(&dir, &copy)?;
    match c.source {
        Source::CratesIo => println!("{VENDOR}/{}: published crate {version}, patched", c.name),
        Source::GitHub { repository, commit } => {
            println!("{VENDOR}/{}: {repository} at {commit}, patched", c.name)
        }
    }
    Ok(())
}

/// Runs `command`, its output the terminal's; `what` names it where it fails.
fn succeed(command: &mut Command, what: &str) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|e| format!("could not start {what}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what} failed ({status})"))
    }
}

/// Removes the file or folder at `path`, as `rm -rf` does: nothing there is not a failure.
fn remove_path(path: &Path) -> Result<(), String> {
    let removed = match std::fs::symlink_metadata(path) {
        Err(_) => return Ok(()),
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
    };
    removed.map_err(|e| format!("{}: {e}", path.display()))
}

/// Copies the folder `from`, whole, to `to`.
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let entries = std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", from.display()))?;
        let kind = entry
            .file_type()
            .map_err(|e| format!("{}: {e}", entry.path().display()))?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)
                .map_err(|e| format!("{}: {e}", entry.path().display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &[&str]) -> Vec<String> {
        line.iter().map(|w| w.to_string()).collect()
    }

    /// The repository's root, the workspace's.
    fn desktop() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn the_six_copies_are_held_each_with_its_version_and_sha256() {
        let names: Vec<&str> = CRATES.iter().map(|c| c.name).collect();
        let wanted = [
            "librqbit",
            "librqbit-sha1-wrapper",
            "gpui-pre-linux",
            "gpui-pre-windows",
            "gpui-pre-wgpu",
            "gpui-android",
        ];
        assert_eq!(names, wanted);
        for c in CRATES {
            assert_eq!(c.sha256.len(), 64, "{}", c.name);
            assert!(
                c.sha256
                    .chars()
                    .all(|ch| ch.is_ascii_digit() || ('a'..='f').contains(&ch)),
                "{}",
                c.name
            );
            assert!(plain_version(c.version), "{}", c.name);
            let patch = desktop()
                .join("vendor")
                .join("patches")
                .join(format!("{}.patch", c.name));
            assert!(patch.is_file(), "{} has no patch", c.name);
        }
        assert_eq!(CRATES[0].version, LIBRQBIT_VERSION);
        assert_eq!(CRATES[1].version, LIBRQBIT_VERSION);
        assert_eq!(CRATES[2].version, GPUI_VERSION);
        assert_eq!(CRATES[3].version, GPUI_VERSION);
        assert_eq!(CRATES[4].version, GPUI_VERSION);
        for c in &CRATES[..5] {
            assert_eq!(c.source, Source::CratesIo, "{}", c.name);
        }
        let Source::GitHub { repository, commit } = CRATES[5].source else {
            panic!("gpui-android is made from GitHub");
        };
        assert_eq!(repository, "itsbalamurali/gpui-mobile");
        assert_eq!(commit.len(), 40);
        assert!(commit.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    /// The copies are of the versions dm's manifest asks for, or `[patch.crates-io]` would leave
    /// them unused.
    #[test]
    fn the_versions_held_are_those_desktop_cargo_toml_pins() {
        let text = std::fs::read_to_string(desktop().join("Cargo.toml")).expect("Cargo.toml");
        let manifest: toml::Table = text.parse().expect("it parses");
        let deps = manifest
            .get("workspace")
            .and_then(|w| w.get("dependencies"))
            .and_then(|d| d.as_table())
            .expect("workspace dependencies");
        let version = |name: &str| {
            deps.get(name)
                .and_then(|d| d.get("version"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        };
        assert_eq!(version("gpui"), Some(format!("={GPUI_VERSION}")));
        assert_eq!(version("gpui_platform"), Some(format!("={GPUI_VERSION}")));
        assert_eq!(version("librqbit"), Some(LIBRQBIT_VERSION.to_string()));
    }

    #[test]
    fn what_is_removed_is_what_the_build_does_not_use() {
        let common = ["Cargo.lock", ".cargo_vcs_info.json", ".cargo-ok"];
        for c in CRATES {
            for path in common {
                if c.source == Source::CratesIo {
                    assert!(c.remove.contains(&path), "{} keeps {path}", c.name);
                }
            }
            for path in c.remove {
                assert!(!path.contains(".."), "{path}");
                assert!(!Path::new(path).is_absolute(), "{path}");
            }
        }
        for path in ["webui", "resources", "examples", "Cargo.toml.orig"] {
            assert!(CRATES[0].remove.contains(&path) && CRATES[1].remove.contains(&path));
            assert!(!CRATES[2].remove.contains(&path) && !CRATES[3].remove.contains(&path));
        }
    }

    #[test]
    fn a_crate_is_fetched_from_crates_ios_static_address() {
        assert_eq!(
            crate_url("gpui-pre-linux", "0.3.12"),
            "https://static.crates.io/crates/gpui-pre-linux/gpui-pre-linux-0.3.12.crate"
        );
    }

    #[test]
    fn a_commit_is_fetched_as_githubs_archive_and_unpacked_in_its_folder() {
        let android = CRATES[5];
        let (url, file) = fetched(&android, android.version);
        assert_eq!(
            url,
            "https://codeload.github.com/itsbalamurali/gpui-mobile/tar.gz/\
             5fd54f4554263ae06753e90472c7f2e2ff5d7495"
        );
        assert_eq!(
            file,
            "gpui-android-5fd54f4554263ae06753e90472c7f2e2ff5d7495.tar.gz"
        );
        assert_eq!(
            unpacked(&android, android.version),
            "gpui-mobile-5fd54f4554263ae06753e90472c7f2e2ff5d7495"
        );
        let linux = CRATES[2];
        assert_eq!(unpacked(&linux, "0.3.12"), "gpui-pre-linux-0.3.12");
        assert!(android.remove.contains(&"src/ios") && android.remove.contains(&"example"));
    }

    #[test]
    fn the_digest_is_sha256_in_lower_case_hex() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn no_crate_named_makes_all_six_and_names_make_those() {
        let all = parse(&[]).expect("all");
        assert_eq!(all.crates, CRATES.to_vec());
        assert_eq!(all.version, None);
        let two = parse(&words(&["gpui-pre-windows", "librqbit"])).expect("two");
        assert_eq!(two.crates, vec![CRATES[3], CRATES[0]]);
        assert!(parse(&words(&["gpui-pre"])).is_err());
        assert!(parse(&words(&["--force"])).is_err());
    }

    #[test]
    fn another_version_is_for_the_crates_named_and_written_as_crates_io_writes_one() {
        let next = parse(&words(&["--version", "0.3.13", "gpui-pre-linux"])).expect("next");
        assert_eq!(next.version.as_deref(), Some("0.3.13"));
        assert_eq!(next.crates, vec![CRATES[2]]);
        let joined = parse(&words(&["gpui-pre-windows", "--version=0.4.0-rc.1"])).expect("rc");
        assert_eq!(joined.version.as_deref(), Some("0.4.0-rc.1"));
        assert!(parse(&words(&["--version", "0.3.13"])).is_err());
        assert!(parse(&words(&["--version", "0.2.0", "gpui-android"])).is_err());
        assert!(parse(&words(&["gpui-pre-linux", "--version"])).is_err());
        for bad in ["../x", "0.3/1", "", "v1", "1 2", "1.0?x=1"] {
            let line = words(&["--version", bad, "gpui-pre-linux"]);
            assert!(parse(&line).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_missing_path_is_not_a_failure_to_remove() {
        let gone = std::env::temp_dir().join(format!("dm-vendor-none-{}", std::process::id()));
        assert!(remove_path(&gone).is_ok());
    }
}
