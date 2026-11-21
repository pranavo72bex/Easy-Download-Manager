// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's own checks, run as `cargo xtask <job>` from the repository's root.
//!
//! - `scorecard` writes `docs/scorecard.md`: what is done, partial and
//!   missing, counted from the parity matrix, the server zoo and the fuzz
//!   tests.
//! - `layers` holds the crates to the shape they are meant to have: the
//!   engine knows no window, the extension has no sockets, nothing leans on
//!   the window; and no file or function grows past its size (`sizes.rs`), which
//!   `size-baseline` lowers the baselines to as they shrink, and no crate comes in more
//!   versions than `doubles-baseline.txt` lists (`doubles.rs`), which `doubles-baseline`
//!   shortens it to as they fall.
//! - `privacy` holds dm to being free and private: no analytics or crash
//!   reporting crate anywhere in the build, and no address in shipped code
//!   but the project's own release page, which is fetched only when asked.
//!
//! - `headers` holds every source file of dm's own to its copyright and
//!   licence header (`headers.rs`); `headers fix` writes the missing ones.
//!
//! Each is also a test, so `cargo test --workspace` runs them.
//!
//! - `check` runs everything that needs no window, the app's and the extension's, in one command
//!   (`check.rs`): formatting, lints, licences, the tests (and through them the checks above).
//! - `i18n` says, per language, which strings its catalog lacks and which no native speaker
//!   has read yet (`i18n.rs`, `docs/guide/translating.md`); `i18n --issue <code>` writes the
//!   issue a reader of that language takes.
//! - `crates-list`, `sign-release` and `verify-release` say what a release is built from and
//!   sign the digest of each of its files, so anyone can check one (`release.rs`, gaps row W21).
//! - `help-check` fails where a sentence a person can see (a row's note, a failure the engine
//!   says, a word of Options) has no entry in the help (`help.rs`, `docs/guide/`).
//! - `package` makes this system's installers with cargo-packager, run where its settings are
//!   (`package.rs`); it and the others work from the repository's root too
//!   (`.cargo/config.toml` there).
//! - `build`, `test` and `qa` build dm and its extension, run the tests and run
//!   `scripts/human-qa.py`, the same on every system, as `package` makes the installers
//!   (`tasks.rs`, gaps row X7).

#[cfg(test)]
mod baseline;
mod check;
mod doubles;
mod env_check;
mod headers;
mod help;
mod i18n;
mod package;
mod release;
mod sizes;
mod tasks;
mod update_key;
mod vendor;
mod vendor_marks;

use std::path::{Path, PathBuf};

fn main() {
    let job = std::env::args().nth(1).unwrap_or_default();
    let result = match job.as_str() {
        "check" => check::run(&root()),
        "help-check" => help::run(&root()),
        "i18n" => i18n::run(&root(), &std::env::args().skip(2).collect::<Vec<_>>()),
        "package" => package::run(&root(), &std::env::args().skip(2).collect::<Vec<_>>()),
        "build" => tasks::build(&root(), &std::env::args().skip(2).collect::<Vec<_>>()),
        "test" => tasks::test(&root(), &std::env::args().skip(2).collect::<Vec<_>>()),
        "qa" => tasks::qa(&root(), &std::env::args().skip(2).collect::<Vec<_>>()),
        "vendor" => vendor::run(&root(), &std::env::args().skip(2).collect::<Vec<_>>()),
        "scorecard" => write_scorecard(),
        "layers" => layers().map(|()| "the crates are layered as they should be".to_string()),
        "size-baseline" => sizes::lower(&root()),
        "doubles-baseline" => {
            doubles::rewrite(&root(), &std::env::args().skip(2).collect::<Vec<_>>())
        }
        "privacy" => privacy().map(|()| "nothing phones home".to_string()),
        "parity" => {
            parity_evidence().map(|()| "every test docs/parity.md cites exists".to_string())
        }
        "secrets" => secrets().map(|()| "no credential is written into a tracked file".to_string()),
        "update-key" => update_key::update_key(),
        "sign-manifest" => update_key::sign_manifest(&root(), std::env::args().nth(2).as_deref()),
        "crates-list" => release::write_crates_list(&root(), std::env::args().nth(2).as_deref()),
        "sign-release" => release::sign_release(&root(), std::env::args().nth(2).as_deref()),
        "verify-release" => release::verify_release(&root(), std::env::args().nth(2).as_deref()),
        "headers" if std::env::args().nth(2).as_deref() == Some("fix") => headers::fix(&root())
            .map(|n| format!("a copyright and licence header written into {n} files")),
        "headers" => {
            licence_headers().map(|()| "every source file carries its licence header".to_string())
        }
        _ => Err(tasks::USAGE.to_string()),
    };
    match result {
        Ok(said) => println!("{said}"),
        Err(why) => {
            eprintln!("{why}");
            std::process::exit(1);
        }
    }
}

/// Every source file of dm's own carries the copyright and licence header
/// (`headers.rs`), so a copy of any one of them says whose it is and on what terms.
fn licence_headers() -> Result<(), String> {
    let missing = headers::missing(&root())?;
    if missing.is_empty() {
        return Ok(());
    }
    let names: Vec<_> = missing.iter().map(|(n, _)| n.as_str()).collect();
    Err(format!(
        "{} source files have no copyright and licence header (run `cargo xtask headers fix` from the repository's root):\n{}",
        names.len(),
        names.join("\n")
    ))
}

/// The repository root.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("../.."))
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/* ------------------------------------------------------------- scorecard */

/// The statuses of the parity matrix, per section, in order.
fn parity(text: &str) -> Vec<(String, [usize; 5])> {
    let mut sections: Vec<(String, [usize; 5])> = Vec::new();
    for line in text.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            sections.push((title.trim().to_string(), [0; 5]));
            continue;
        }
        let Some((_, counts)) = sections.last_mut() else {
            continue;
        };
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        let Some(status) = cells.get(2) else {
            continue;
        };
        let slot = match *status {
            "PASS" => 0,
            "PARTIAL" => 1,
            "NOT VERIFIED" => 2,
            "BLOCKED" => 3,
            "N/A" => 4,
            _ => continue,
        };
        if let Some(n) = counts.get_mut(slot) {
            *n += 1;
        }
    }
    sections
}

/// The first bold "N of M" in a generated document.
fn headline(text: &str) -> Option<(usize, usize)> {
    text.split("**").skip(1).step_by(2).find_map(|bold| {
        let (a, b) = bold.split_once(" of ")?;
        Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
    })
}

/// How many property tests the fuzz files hold.
fn fuzz_targets(root: &Path) -> Vec<(String, usize)> {
    let files = [
        "crates/dm-core/tests/fuzz.rs",
        "crates/dm-shell/tests/fuzz.rs",
        "crates/dm-protocol/tests/fuzz.rs",
        "crates/dm-desktop-base/tests/fuzz.rs",
        "crates/dm-gpui/src/window_tests/fuzz.rs",
        "crates/dm-catcher/tests/fuzz.rs",
    ];
    files
        .iter()
        .map(|f| {
            let n = std::fs::read_to_string(root.join(f))
                .map(|t| t.matches("#[test]").count())
                .unwrap_or(0);
            (f.to_string(), n)
        })
        .collect()
}

/// The budgets a test holds, and the test that holds each.
const BUDGETS: &[(&str, &str, &str)] = &[
    (
        "A download never needs twice its own size on disk",
        "crates/dm-core/tests/download.rs",
        "a_download_never_needs_twice_its_own_size_on_disk",
    ),
    (
        "Resident memory stays flat across repeated downloads",
        "crates/dm-core/tests/memory.rs",
        "fn ",
    ),
    (
        "500 rows draw within a 100 ms frame",
        "crates/dm-gpui/src/window_tests/layout.rs",
        "five_hundred_rows_draw_inside_a_frame_budget",
    ),
    (
        "A pause on a stalled connection stops within the read timeout",
        "crates/dm-core/tests/download.rs",
        "a_pause_on_a_stalled_connection_is_honoured_within_the_read_timeout",
    ),
];

fn scorecard() -> Result<String, String> {
    let root = root();
    let parity_text = read(&root.join("docs/parity.md"))?;
    let zoo = headline(&read(&root.join("docs/zoo.md"))?).ok_or("the zoo has no headline")?;
    let sections = parity(&parity_text);

    let mut out = String::from(
        "# Scorecard\n\n\
         Generated by `cargo xtask scorecard` (from the repository's root) — do not edit by hand. \
         Counted from [the behaviour checklist](parity.md), \
         [the server zoo](zoo.md) and the fuzz tests; a test fails when this is out of date.\n\n",
    );
    out.push_str(&format!(
        "| Measure | Score |\n|---|---|\n\
         | Server behaviours handled as they must be | **{} of {}** (floor only rises) |\n",
        zoo.0, zoo.1
    ));
    let totals = sections.iter().fold([0usize; 5], |mut acc, (_, c)| {
        for (a, b) in acc.iter_mut().zip(c) {
            *a += b;
        }
        acc
    });
    out.push_str(&format!(
        "| Behaviours proven by a test or a run | **{} of {}** ({} partial, {} not verified, {} blocked, {} out of scope) |\n",
        totals[0],
        totals.iter().sum::<usize>(),
        totals[1],
        totals[2],
        totals[3],
        totals[4]
    ));
    let fuzz = fuzz_targets(&root);
    out.push_str(&format!(
        "| Readers of outside input under fuzzing | **{}** property tests in {} files |\n",
        fuzz.iter().map(|(_, n)| n).sum::<usize>(),
        fuzz.iter().filter(|(_, n)| *n > 0).count()
    ));
    out.push_str(&format!(
        "| Budgets held by a test | **{}** |\n\n",
        BUDGETS.len()
    ));

    out.push_str("## By area\n\n| Area | Pass | Partial | Not verified | Blocked | Out of scope |\n|---|---|---|---|---|---|\n");
    for (title, c) in &sections {
        if c.iter().sum::<usize>() == 0 {
            continue;
        }
        out.push_str(&format!(
            "| {title} | {} | {} | {} | {} | {} |\n",
            c[0], c[1], c[2], c[3], c[4]
        ));
    }
    out.push_str("\n## Budgets\n\n| Budget | Held by |\n|---|---|\n");
    for (what, file, test) in BUDGETS {
        let present = read(&root.join(file))
            .map(|t| t.contains(test))
            .unwrap_or(false);
        if !present {
            return Err(format!(
                "the budget \"{what}\" names a test that is gone: {file} {test}"
            ));
        }
        out.push_str(&format!("| {what} | `{file}` |\n"));
    }
    out.push_str("\n## Fuzz tests\n\n| File | Property tests |\n|---|---|\n");
    for (file, n) in &fuzz {
        out.push_str(&format!("| `{file}` | {n} |\n"));
    }
    out.push_str(
        "\nA deep hunt: `PROPTEST_CASES=200000 cargo test --release --test fuzz` in any of those crates.\n",
    );
    Ok(out)
}

fn write_scorecard() -> Result<String, String> {
    let card = scorecard()?;
    let path = root().join("docs/scorecard.md");
    std::fs::write(&path, &card).map_err(|e| e.to_string())?;
    Ok(format!("wrote {}", path.display()))
}

/* ---------------------------------------------------------------- layers */

/// A crate's manifest, and which of its dependencies are dm's own.
struct Crate {
    name: String,
    deps: Vec<String>,
    /// Dependencies of any kind (tests' and the build's too) whose version or path the crate
    /// writes itself rather than taking the workspace's (`x.workspace = true`, gaps row X8).
    own_versions: Vec<String>,
}

/// The dependencies in `value` (a manifest) not written as `workspace = true`.
fn own_versions(value: &toml::Table) -> Vec<String> {
    let mut tables = vec![value];
    if let Some(targets) = value.get("target").and_then(|t| t.as_table()) {
        tables.extend(targets.values().filter_map(|t| t.as_table()));
    }
    let mut own = Vec::new();
    for table in tables {
        for kind in ["dependencies", "dev-dependencies", "build-dependencies"] {
            let Some(deps) = table.get(kind).and_then(|d| d.as_table()) else {
                continue;
            };
            for (name, spec) in deps {
                let inherited = spec.get("workspace").and_then(|w| w.as_bool()) == Some(true);
                if !inherited {
                    own.push(name.clone());
                }
            }
        }
    }
    own
}

fn manifest(path: &Path) -> Result<Crate, String> {
    let text = read(path)?;
    // A whole document is a table: since toml 1.0, parsing text as a `Value` reads a single
    // value, and a manifest is refused at its first line.
    let value: toml::Table = text
        .parse()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let own_versions = own_versions(&value);
    let name = value
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or_default()
        .to_string();
    let mut deps = Vec::new();
    // Normal dependencies, including per-target ones; dev-dependencies are
    // tests' and may reach further.
    if let Some(table) = value.get("dependencies").and_then(|d| d.as_table()) {
        deps.extend(table.keys().cloned());
    }
    if let Some(targets) = value.get("target").and_then(|t| t.as_table()) {
        for target in targets.values() {
            if let Some(table) = target.get("dependencies").and_then(|d| d.as_table()) {
                deps.extend(table.keys().cloned());
            }
        }
    }
    Ok(Crate {
        name,
        deps,
        own_versions,
    })
}

const GUI: &[&str] = &[
    "eframe",
    "egui",
    "egui_extras",
    "winit",
    "tray-icon",
    "gtk",
    "rfd",
    "notify-rust",
    "arboard",
    // GPUI, under the names dm's manifests give it (docs/gaps.md G6).
    "gpui",
    "gpui_platform",
    "gpui-component",
    "gpui-base",
    // GPUI's Android platform, dm's copy of gpui-mobile's (dm-android alone, gaps row N16).
    "gpui-android",
];
const SOCKETS: &[&str] = &["tokio", "reqwest", "librqbit", "hyper"];

fn layers() -> Result<(), String> {
    let root = root();
    let crates = [
        "crates/dm-assets/Cargo.toml",
        "crates/dm-protocol/Cargo.toml",
        "crates/dm-safari/Cargo.toml",
        "crates/dm-media/Cargo.toml",
        "crates/dm-torrent/Cargo.toml",
        "crates/dm-net/Cargo.toml",
        "crates/dm-core/Cargo.toml",
        "crates/dm-shell/Cargo.toml",
        "crates/dm-test-support/Cargo.toml",
        "crates/dm-desktop-base/Cargo.toml",
        "crates/dm-gpui/Cargo.toml",
        "crates/dm-android/Cargo.toml",
        "crates/dm-catcher/Cargo.toml",
    ]
    .iter()
    .map(|p| manifest(&root.join(p)))
    .collect::<Result<Vec<_>, _>>()?;

    let mut wrong = Vec::new();
    // Every version once, at the workspace's root (gaps row X8): a crate of the desktop
    // workspace (the extension's too, gaps row A15) names a dependency with `x.workspace = true`
    // and adds only its own features.
    for member in [
        "crates/dm-assets/Cargo.toml",
        "crates/dm-protocol/Cargo.toml",
        "crates/dm-safari/Cargo.toml",
        "crates/dm-js/Cargo.toml",
        "crates/dm-media/Cargo.toml",
        "crates/dm-torrent/Cargo.toml",
        "crates/dm-net/Cargo.toml",
        "crates/dm-core/Cargo.toml",
        "crates/dm-shell/Cargo.toml",
        "crates/dm-test-support/Cargo.toml",
        "crates/dm-desktop-base/Cargo.toml",
        "crates/dm-gpui/Cargo.toml",
        "crates/dm-android/Cargo.toml",
        "tooling/xtask/Cargo.toml",
        "crates/dm-catcher/Cargo.toml",
    ] {
        for d in manifest(&root.join(member))?.own_versions {
            wrong.push(format!(
                "{member} writes {d}'s version itself: put it in Cargo.toml's \
                 [workspace.dependencies] and write `{d}.workspace = true`"
            ));
        }
    }
    for c in &crates {
        let own: Vec<&str> = c
            .deps
            .iter()
            .map(String::as_str)
            .filter(|d| d.starts_with("dm-"))
            .collect();
        let (may, no_gui, no_sockets): (&[&str], bool, bool) = match c.name.as_str() {
            // The words both sides speak; nothing else.
            "dm-protocol" => (&[], true, true),
            // dm's files that are not code, as bytes (gaps row X9): nothing else.
            "dm-assets" => (&[], true, true),
            // dm's own JavaScript: the language alone, no window and no sockets (gaps row F15).
            "dm-js" => (&[], true, true),
            // dm's own media code (gaps row A6): files in, files out; nothing of dm's, no window,
            // no sockets and no async runtime.
            "dm-media" => (&[], true, true),
            // BitTorrent's engine behind dm's words (gaps rows A6, P3): the one crate that names
            // librqbit; its sockets are the swarm's, and nothing of dm's.
            "dm-torrent" => (&[], true, false),
            // dm's network (gaps row A6): how a server is reached, whatever is fetched; dm-js
            // runs the proxy script. No window.
            "dm-net" => (&["dm-js"], true, false),
            // What the tests share (gaps row X10): nothing of dm's, and no crate may ship it, as
            // none lists it among those it may depend on (only `[dev-dependencies]` name it).
            "dm-test-support" => (&[], true, true),
            // The engine: no window, no browser.
            "dm-core" => (&["dm-js", "dm-media", "dm-net", "dm-torrent"], true, false),
            // Around the window but not the window.
            "dm-shell" => (&["dm-assets", "dm-core", "dm-protocol"], true, false),
            // What the desktop program does that is not drawing: any window
            // may use it, so it uses none.
            "dm-desktop-base" => (
                &["dm-assets", "dm-core", "dm-protocol", "dm-shell"],
                true,
                false,
            ),
            // The window.
            "dm-gpui" => (
                &[
                    "dm-assets",
                    "dm-core",
                    "dm-protocol",
                    "dm-shell",
                    "dm-desktop-base",
                ],
                false,
                false,
            ),
            // The phone's app (gaps rows N1, N3, N16): dm-gpui's window on gpui-android, with
            // the phone's services handed to dm-desktop-base's seam, so a window toolkit is
            // allowed here; dm-shell for the running dm's inbox, which a notification's Retry
            // and Stop All are left in (N6).
            "dm-android" => (
                &["dm-core", "dm-desktop-base", "dm-gpui", "dm-shell"],
                false,
                false,
            ),
            // The extension: only the protocol, and no sockets of its own.
            "dm-catcher" => (&["dm-protocol"], true, true),
            // Safari's extension's program (gaps row R4): it starts dm's host for each message
            // and links nothing of dm's, no window and no sockets.
            "dm-safari" => (&[], true, true),
            other => {
                wrong.push(format!("{other} is not a crate the layering knows"));
                continue;
            }
        };
        for d in &own {
            if !may.contains(d) {
                wrong.push(format!("{} may not depend on {d}", c.name));
            }
        }
        for d in &c.deps {
            if no_gui && GUI.contains(&d.as_str()) {
                wrong.push(format!("{} may not use a window toolkit ({d})", c.name));
            }
            if no_sockets && SOCKETS.contains(&d.as_str()) {
                wrong.push(format!("{} may not open connections ({d})", c.name));
            }
            // BitTorrent's engine is named in one crate (gaps rows A6, P3); a test may still
            // run a peer of its own with it.
            if d == "librqbit" && c.name != "dm-torrent" {
                wrong.push(format!(
                    "{} may not use librqbit: BitTorrent's engine is dm-torrent's",
                    c.name
                ));
            }
        }
    }
    // Each product source file's layout and inline tests; then, test files (`tests.rs`,
    // `*_tests.rs`) aside, what it may write.
    for dir in sizes::PRODUCT_SOURCES {
        for file in walk(&root.join(dir)) {
            if file.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            let shown = file.strip_prefix(&root).unwrap_or(&file).display();
            // One layout (gaps row A13): a module with children is `foo.rs` beside `foo/`.
            if file.file_name().is_some_and(|n| n == "mod.rs") {
                wrong.push(format!(
                    "{shown}: a module with children is `foo.rs` beside `foo/`, not `foo/mod.rs`"
                ));
            }
            for (name, lines) in long_inline_tests(&text) {
                wrong.push(format!(
                    "{shown}: its test module `{name}` is {lines} lines inline: move it to a file \
                     of its own (`mod {name};`), as the limit is {MAX_INLINE_TEST_LINES}"
                ));
            }
            if is_test_file(&file) {
                continue;
            }
            // The window's spacing is a step of `widgets/space.rs` (gaps row A19).
            if dir == "crates/dm-gpui/src" {
                for (line, written) in spacing_literals(&text) {
                    wrong.push(format!(
                        "{shown}:{line} writes {written}: a gap, padding or margin on the scale is \
                         named in dm-gpui's widgets/space.rs (gaps row A19)"
                    ));
                }
            }
            // Where a person's files live is worked out in one place (gaps row X1).
            if !is_folders_module(&file) {
                for (line, what) in folder_reads(&text) {
                    wrong.push(format!(
                        "{shown}:{line} reads {what}: where a person's files live is \
                         `dm_core::folders`'s to say (gaps row X1)"
                    ));
                }
            }
        }
    }
    // A file that grows into a monolith, or a function into a page, is split before it is
    // changed again (gaps row A12, `sizes.rs`): `dialogs/`, `app/`, `download/` and `queue/`
    // were each one file of thousands of lines.
    wrong.extend(sizes::check(&root)?);
    // A crate built in more than one version only becomes rarer (gaps row A18, `doubles.rs`):
    // no new double in Cargo.lock, and none in more versions than its baseline lists.
    wrong.extend(doubles::check(&root)?);
    // No `#[path]` out of a crate's folder (gaps row X10): what tests share is the
    // `dm-test-support` crate, named in `[dev-dependencies]`.
    for krate in [
        "crates/dm-assets",
        "crates/dm-js",
        "crates/dm-media",
        "crates/dm-torrent",
        "crates/dm-net",
        "crates/dm-protocol",
        "crates/dm-safari",
        "crates/dm-core",
        "crates/dm-shell",
        "crates/dm-test-support",
        "crates/dm-desktop-base",
        "crates/dm-gpui",
        "crates/dm-android",
        "tooling/xtask",
        "crates/dm-catcher",
    ] {
        let home = root.join(krate);
        for sub in ["src", "tests", "benches", "examples"] {
            for file in walk(&home.join(sub)) {
                if file.extension().is_none_or(|e| e != "rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&file).unwrap_or_default();
                for (line, target) in paths_out_of(&text, &file, &home) {
                    let shown = file.strip_prefix(&root).unwrap_or(&file).display();
                    wrong.push(format!(
                        "{shown}:{line} includes {target} by `#[path]` from outside {krate}: \
                         put what is shared in `dm-test-support` (gaps row X10)"
                    ));
                }
            }
        }
    }
    // The window's move to GPUI is over (gaps rows G9, A20): no comment or name in a crate says
    // the earlier toolkit or the migration plan's phases. xtask itself is not read: its list of
    // window crates names them to refuse them.
    for krate in [
        "crates/dm-assets",
        "crates/dm-js",
        "crates/dm-media",
        "crates/dm-torrent",
        "crates/dm-net",
        "crates/dm-protocol",
        "crates/dm-safari",
        "crates/dm-core",
        "crates/dm-shell",
        "crates/dm-test-support",
        "crates/dm-desktop-base",
        "crates/dm-gpui",
        "crates/dm-android",
        "crates/dm-catcher",
    ] {
        let home = root.join(krate);
        let mut files = vec![home.join("Cargo.toml")];
        for sub in ["src", "tests", "benches", "examples"] {
            files.extend(
                walk(&home.join(sub))
                    .into_iter()
                    .filter(|f| f.extension().is_some_and(|e| e == "rs")),
            );
        }
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            let shown = file.strip_prefix(&root).unwrap_or(&file).display();
            for (line, word) in migration_words(&text) {
                wrong.push(format!(
                    "{shown}:{line} says {word}: say what the code does and why now, and leave \
                     the window's migration to docs/decisions.md and git (gaps row A20)"
                ));
            }
        }
    }
    // Every `DM_` switch is read through `dm_core::env`'s table (gaps row A11).
    wrong.extend(env_check::problems(&root));
    // Every `dm(<tag>):` change in vendor/ is a row of its README's table (A17, X5).
    wrong.extend(vendor_marks::problems(&root));
    if wrong.is_empty() {
        Ok(())
    } else {
        Err(wrong.join("\n"))
    }
}

/// The `#[path = "..."]` attributes of `file` (whose text is `text`) naming a file outside the
/// crate folder `home`, each with its line (from 1) and the path written. The path is taken
/// against `file`'s folder and `..` resolved by its words, without asking the disk.
fn paths_out_of(text: &str, file: &Path, home: &Path) -> Vec<(usize, String)> {
    let base = file.parent().unwrap_or(Path::new(""));
    let mut found = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let Some(target) = line
            .trim()
            .strip_prefix("#[path = \"")
            .and_then(|rest| rest.strip_suffix("\"]"))
        else {
            continue;
        };
        let joined = base.join(target);
        let mut at: Vec<std::path::Component> = Vec::new();
        for part in joined.components() {
            match part {
                std::path::Component::ParentDir => {
                    at.pop();
                }
                std::path::Component::CurDir => {}
                other => at.push(other),
            }
        }
        let resolved: PathBuf = at.iter().collect();
        if !resolved.starts_with(home) {
            found.push((i + 1, target.to_string()));
        }
    }
    found
}

/// The lines of `text` that name the window's earlier toolkit (`egui`, `eframe`) or a phase of
/// the migration plan (`Phase B`), case aside, by line number. Words are split at anything but
/// a letter or digit, so `egui_extras` and a test named `…_egui_…` are found, and a word that
/// only holds the letters (`DecodeFrame`, an installer's `dmReplaceEguiWindow`) is not.
fn migration_words(text: &str) -> Vec<(usize, &'static str)> {
    let mut found = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let lower = line.to_ascii_lowercase();
        let words: Vec<&str> = lower
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        if words.iter().any(|w| *w == "egui" || *w == "eframe") {
            found.push((n + 1, "the earlier window toolkit"));
        } else if words
            .windows(2)
            .any(|pair| pair[0] == "phase" && matches!(pair[1], "a" | "b" | "c" | "d" | "e"))
        {
            found.push((n + 1, "a phase of the migration"));
        }
    }
    found
}

/// Whether a file is a test file of its own: `tests.rs`, or `<what>_tests.rs` beside it
/// (gaps row A13).
fn is_test_file(file: &Path) -> bool {
    file.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == "tests.rs" || n.ends_with("_tests.rs"))
}

/// What only `dm-core/src/folders.rs` may read (gaps row X1): the `dirs` crate, and the
/// variables and paths that say where a person's folders are. AppRun's `XDG_DATA_DIRS`
/// (`processes.rs`) names a variable a child is given, not a folder read, and is not here.
const FOLDER_READS: &[&str] = &[
    "dirs::",
    "\"HOME\"",
    "\"USERPROFILE\"",
    "\"APPDATA\"",
    "\"LOCALAPPDATA\"",
    "\"XDG_CONFIG_HOME\"",
    "\"XDG_DATA_HOME\"",
    "\"Library/Application Support\"",
    "\"Library/LaunchAgents\"",
];

/// The methods that space a window's parts: gaps, paddings, margins.
const SPACING_METHODS: &[&str] = &[
    "gap", "gap_x", "gap_y", "p", "px", "py", "pt", "pb", "pl", "pr", "m", "mx", "my", "mt", "mb",
    "ml", "mr",
];

/// dm-gpui's spacing scale (`widgets/space.rs`), as a literal is written.
const SPACING_SCALE: &[&str] = &[
    "2.0", "4.0", "6.0", "8.0", "10.0", "12.0", "16.0", "20.0", "24.0",
];

/// The lines of `text` (from 1) that space a part by a literal on the scale, with what each
/// writes. Comments are passed over; a value off the scale is a control's own measure.
fn spacing_literals(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        for method in SPACING_METHODS {
            for px in ["px(", "crate::window::zoom::px(", "gpui::px("] {
                for value in SPACING_SCALE {
                    let written = format!(".{method}({px}{value}))");
                    if line.contains(&written) {
                        found.push((i + 1, written));
                    }
                }
            }
        }
    }
    found
}

/// Whether `file` is `dm_core::folders` itself, or its tests.
fn is_folders_module(file: &Path) -> bool {
    let shown = file.to_string_lossy().replace('\\', "/");
    shown.ends_with("dm-core/src/folders.rs")
        || shown.contains("dm-core/src/folders/")
        // Safari's extension's program reads no folder: it gives the host it starts the
        // person's own `HOME`, which the extension's sandbox replaced, and the host then asks
        // `dm_core::folders` as it always does (gaps row R4).
        || shown.ends_with("dm-safari/src/relay.rs")
}

/// The lines of `text` (numbered from 1) that read one of [`FOLDER_READS`], with what each
/// reads. Comments are passed over: they may name a folder.
fn folder_reads(text: &str) -> Vec<(usize, &'static str)> {
    let mut found = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let code = line.trim_start();
        if code.starts_with("//") {
            continue;
        }
        if let Some(what) = FOLDER_READS.iter().find(|w| code.contains(**w)) {
            found.push((i + 1, *what));
        }
    }
    found
}

/// The most lines a test module may have written inside a product file (gaps row A13).
const MAX_INLINE_TEST_LINES: usize = 100;

/// The `#[cfg(test)] mod name { ... }` blocks of `text` longer than
/// [`MAX_INLINE_TEST_LINES`], each with its length. A block is one opened at the start of a line
/// and closed by the first `}` alone at the start of a line, as rustfmt writes a module.
fn long_inline_tests(text: &str) -> Vec<(String, usize)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(name) = line
            .strip_prefix("mod ")
            .and_then(|rest| rest.strip_suffix(" {"))
        else {
            continue;
        };
        if i == 0 || lines[i - 1].trim() != "#[cfg(test)]" {
            continue;
        }
        let length = lines[i + 1..]
            .iter()
            .position(|l| *l == "}")
            .unwrap_or(lines.len() - i - 1);
        if length > MAX_INLINE_TEST_LINES {
            found.push((name.to_string(), length));
        }
    }
    found
}

/* --------------------------------------------------------------- privacy */

/// Crates whose job is to send what happens on a machine somewhere else.
const REPORTERS: &[&str] = &[
    "sentry",
    "sentry-core",
    "opentelemetry",
    "posthog",
    "posthog-rs",
    "mixpanel",
    "segment",
    "bugsnag",
    "rollbar",
    "crashpad",
    "minidump-writer",
    "breakpad",
    "datadog",
    "newrelic",
    "amplitude",
    "honeycomb",
    "logrocket",
    "analytics",
    "telemetry",
    "appsignal",
];

/// The only addresses shipped code may hold: the project's own release
/// page (the update check and Help, both only when asked), and the two
/// stores dm's extension is listed in, opened in the person's browser when
/// they click "add dm to your browser".
const ALLOWED_HOSTS: &[&str] = &[
    "github.com",
    // Asked only for a YouTube video a person asked for (`youtube.rs`).
    "youtube.com",
    // Asked only for a Google Drive share address a person gave, as Drive's own Download
    // button asks it (`dm-core/src/filehost.rs`, gaps row W7).
    "drive.usercontent.google.com",
    "microsoftedge.microsoft.com",
    "addons.mozilla.org",
];

fn privacy() -> Result<(), String> {
    let root = root();
    let mut wrong = Vec::new();
    // One lock file for the app and the extension (gaps row A15).
    let lock = "Cargo.lock";
    for line in read(&root.join(lock))?.lines() {
        if let Some(name) = line.strip_prefix("name = ").map(|n| n.trim_matches('"')) {
            if REPORTERS.contains(&name) {
                wrong.push(format!("{lock} pulls in {name}, which reports to someone"));
            }
        }
    }
    for dir in [
        "crates/dm-assets/src",
        "crates/dm-core/src",
        "crates/dm-media/src",
        "crates/dm-net/src",
        "crates/dm-torrent/src",
        "crates/dm-shell/src",
        "crates/dm-protocol/src",
        "crates/dm-safari/src",
        "crates/dm-desktop-base/src",
        "crates/dm-gpui/src",
        "crates/dm-android/src",
        "crates/dm-catcher/src",
        "extension",
    ] {
        for file in walk(&root.join(dir)) {
            // A module's tests live in its `tests.rs` or a `*_tests.rs` (gaps row A13), or start
            // at #[cfg(test)]; the window's tests and their harness are test modules of their
            // own (`dm-gpui/src/window_tests`, `harness`), whose files hold no #[cfg(test)].
            let test_only = is_test_file(&file)
                || file.components().any(|c| {
                    [
                        "tests.rs",
                        "window_tests",
                        "window_tests.rs",
                        "harness",
                        "harness.rs",
                    ]
                    .iter()
                    .any(|t| c.as_os_str() == *t)
                });
            if test_only {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            // Shipped code only.
            let shipped = text.split("#[cfg(test)]").next().unwrap_or("");
            for host in hosts_in(shipped) {
                let allowed = ALLOWED_HOSTS
                    .iter()
                    .any(|a| host == *a || host.ends_with(&format!(".{a}")));
                let local = host == "127.0.0.1" || host == "localhost";
                if !allowed && !local {
                    wrong.push(format!("{} reaches out to {host}", file.display()));
                }
            }
        }
    }
    if wrong.is_empty() {
        Ok(())
    } else {
        Err(wrong.join("\n"))
    }
}

/// Every tracked file is free of anything a secret scanner takes for a
/// leaked credential: a login written into an address, a Basic header's
/// token, a private key, a cloud or GitHub token. dm's tests use made-up
/// logins, and one written out whole raised a "Basic Auth String" incident
/// on the published repository; they are now put together where they are
/// used, and this keeps it so.
fn secrets() -> Result<(), String> {
    let root = root();
    let listed = std::process::Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(&root)
        .output()
        .map_err(|e| format!("git ls-files: {e}"))?;
    let mut wrong = Vec::new();
    for name in listed.stdout.split(|b| *b == 0).filter(|n| !n.is_empty()) {
        let name = String::from_utf8_lossy(name);
        if name.ends_with(".lock") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(&*name)) else {
            continue;
        };
        for (line, found) in secrets_in(&text) {
            wrong.push(format!("{name}:{line}: {found}"));
        }
    }
    if wrong.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "credentials written out (build them where they are used instead):\n{}",
            wrong.join("\n")
        ))
    }
}

/// What in `text` reads as a credential, by line number.
fn secrets_in(text: &str) -> Vec<(usize, &'static str)> {
    // Spelled in pieces, so this file is not itself a finding.
    let key_header = format!("{} PRIVATE {}", "BEGIN", "KEY-----");
    let rsa_header = format!("{} RSA PRIVATE {}", "BEGIN", "KEY-----");
    let mut found = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let n = n + 1;
        if login_in_address(line) {
            found.push((n, "a login written into an address"));
        }
        if basic_token(line) {
            found.push((n, "a Basic authorization token"));
        }
        if line.contains(&key_header) || line.contains(&rsa_header) {
            found.push((n, "a private key"));
        }
        if token_after(line, "AKIA", 16, |c| {
            c.is_ascii_uppercase() || c.is_ascii_digit()
        }) || ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"]
            .iter()
            .any(|p| token_after(line, p, 36, |c| c.is_ascii_alphanumeric()))
        {
            found.push((n, "an access token"));
        }
    }
    found
}

/// A scheme, `://`, a user, a colon, a password written out (not a `{…}`
/// placeholder filled in at run time) and `@`.
fn login_in_address(line: &str) -> bool {
    let user_char = |c: char| c.is_ascii_alphanumeric() || "%._-".contains(c);
    line.match_indices("://").any(|(at, _)| {
        let scheme = line[..at]
            .chars()
            .rev()
            .take_while(char::is_ascii_alphabetic)
            .count();
        if scheme < 2 {
            return false;
        }
        let rest = &line[at + 3..];
        let user = rest.chars().take_while(|c| user_char(*c)).count();
        let Some(after_user) = rest.get(user..).filter(|_| user > 0) else {
            return false;
        };
        let Some(after_colon) = after_user.strip_prefix(':') else {
            return false;
        };
        let password: String = after_colon
            .chars()
            .take_while(|c| !"@/ \"'`{\\".contains(*c))
            .collect();
        !password.is_empty() && after_colon[password.len()..].starts_with('@')
    })
}

/// `Basic ` and a base64 token: eight or more of its characters, holding a
/// digit or padding — which "Basic realm" and words in a sentence do not.
fn basic_token(line: &str) -> bool {
    line.match_indices("Basic ").any(|(at, word)| {
        let token: String = line[at + word.len()..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || "+/=".contains(*c))
            .collect();
        token.len() >= 8 && token.chars().any(|c| c.is_ascii_digit() || c == '=')
    })
}

/// `prefix` followed by at least `len` characters that `ok` accepts.
fn token_after(line: &str, prefix: &str, len: usize, ok: impl Fn(char) -> bool) -> bool {
    line.match_indices(prefix).any(|(at, _)| {
        line[at + prefix.len()..]
            .chars()
            .take_while(|c| ok(*c))
            .count()
            >= len
    })
}

/// Hosts named in `https://…` or `http://…` string literals.
fn hosts_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for scheme in ["\"https://", "\"http://"] {
        let mut rest = text;
        while let Some(at) = rest.find(scheme) {
            let after = rest.get(at + scheme.len()..).unwrap_or("");
            let host: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
                .collect();
            // A placeholder like "https://{host}" or "https://" + … names no host.
            if host.contains('.') {
                found.push(host.to_ascii_lowercase());
            }
            rest = after;
        }
    }
    found
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // The built WebAssembly glue is generated, not written.
                if path.file_name().is_some_and(|n| n == "pkg") {
                    continue;
                }
                found.extend(walk(&path));
            } else if path
                .extension()
                .is_some_and(|e| e == "rs" || e == "js" || e == "json" || e == "html")
            {
                found.push(path);
            }
        }
    }
    found
}

/// Every test a row of `docs/parity.md` names as its evidence still exists:
/// a renamed test left rows citing nothing, and one that described the
/// opposite of what dm now does. A name is a backticked word of 26 or more
/// letters, digits and underscores, which is how the rows name tests.
fn parity_evidence() -> Result<(), String> {
    let parity = read(&root().join("docs/parity.md"))?;
    let mut code = String::new();
    for dir in ["crates", "scripts"] {
        for file in walk(&root().join(dir)) {
            code.push_str(&std::fs::read_to_string(&file).unwrap_or_default());
        }
        // Scripts are Python and shell, which `walk` passes over.
        if dir == "scripts" {
            if let Ok(entries) = std::fs::read_dir(root().join(dir)) {
                for entry in entries.flatten() {
                    code.push_str(&std::fs::read_to_string(entry.path()).unwrap_or_default());
                }
            }
        }
    }
    let missing: Vec<&str> = parity
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|name| {
            name.len() >= 26
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
        .filter(|name| {
            !code.contains(&format!("fn {name}")) && !code.contains(&format!("def {name}"))
        })
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "docs/parity.md cites tests that do not exist: {missing:?}"
        ))
    }
}

#[cfg(test)]
use dm_test_support::secrets as test_secrets;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_parity_rows_cite_tests_that_exist() {
        if let Err(why) = parity_evidence() {
            panic!("{why}");
        }
    }

    /// The app's and the extension's tables of YouTube's picture formats name the same formats
    /// at the same heights (gaps row Y14): the panel's label and the download agree.
    #[test]
    fn the_youtube_format_tables_agree() {
        use std::collections::BTreeMap;
        let app =
            std::fs::read_to_string(root().join("crates/dm-core/src/youtube/itags.rs")).unwrap();
        let mut ours = BTreeMap::new();
        for line in app.lines() {
            let line = line.trim();
            if let Some((itag, rest)) = line.split_once(" => video(") {
                let height = rest.split(',').next().unwrap().trim();
                ours.insert(
                    itag.trim().parse::<u32>().unwrap(),
                    height.parse::<u32>().unwrap(),
                );
            }
        }
        let extension =
            std::fs::read_to_string(root().join("crates/dm-catcher/src/decide/streams.rs"))
                .unwrap();
        let table = extension
            .split("fn itag_height")
            .nth(1)
            .and_then(|rest| rest.split("_ => return None").next())
            .unwrap();
        let mut theirs = BTreeMap::new();
        for line in table.lines() {
            let Some((itags, height)) = line.trim().split_once(" => ") else {
                continue;
            };
            let Ok(height) = height.trim_end_matches(',').trim().parse::<u32>() else {
                continue;
            };
            for itag in itags.split('|') {
                if let Ok(itag) = itag.trim().parse::<u32>() {
                    if height > 0 {
                        theirs.insert(itag, height);
                    }
                }
            }
        }
        assert!(!ours.is_empty() && !theirs.is_empty());
        assert_eq!(ours, theirs, "the two tables of picture formats differ");
    }

    #[test]
    fn the_scorecard_is_up_to_date() {
        let card = scorecard().unwrap();
        // Line endings aside: a Windows checkout may turn \n into \r\n.
        let on_disk = std::fs::read_to_string(root().join("docs/scorecard.md"))
            .unwrap_or_default()
            .replace("\r\n", "\n");
        assert!(
            on_disk == card,
            "docs/scorecard.md is out of date: run `cargo xtask scorecard` from the repository's root"
        );
    }

    #[test]
    fn the_crates_are_layered_as_they_should_be() {
        if let Err(why) = layers() {
            panic!("{why}");
        }
    }

    #[test]
    fn the_windows_migration_is_not_told_in_the_crates() {
        let text = "// as in the egui window\n\
                    fn the_egui_windows_menus() {}\n\
                    // egui_extras's table\n\
                    // Phase B check 1\n\
                    fn decode() { VTDecompressionSessionDecodeFrame(); }\n\
                    Section \"dmReplaceEguiWindow\"\n\
                    // a phase by phase plan; Phase F\n";
        let lines: Vec<usize> = migration_words(text).into_iter().map(|(n, _)| n).collect();
        assert_eq!(lines, [1, 2, 3, 4]);
    }

    #[test]
    fn spacing_on_the_scale_is_named() {
        let text =
            "    .gap(px(4.0))\n    .px(px(space::L))\n    // .p(px(8.0))\n    .pl(px(9.0))\n";
        assert_eq!(spacing_literals(text), [(1, ".gap(px(4.0))".to_string())]);
    }

    #[test]
    fn a_path_attribute_stays_inside_its_crate() {
        let home = Path::new("/r/crates/dm-core");
        let file = home.join("tests/live.rs");
        let text = "#[path = \"common/mp4_samples.rs\"]\nmod a;\n\
                    #[path = \"../../../test-support/secrets.rs\"]\nmod b;\n";
        assert_eq!(
            paths_out_of(text, &file, home),
            [(3, "../../../test-support/secrets.rs".to_string())]
        );
        let inside = home.join("src/media.rs");
        assert!(paths_out_of("#[path = \"media/tests.rs\"]", &inside, home).is_empty());
    }

    #[test]
    fn a_folder_is_read_in_dm_core_folders_alone() {
        let text = "// dirs::home_dir() in a comment is passed over\n\
                    let a = 1;\n\
                    let h = std::env::var_os(\"HOME\");\n\
                    let d = dirs::download_dir();\n\
                    let x = std::env::var_os(\"XDG_DATA_DIRS\");\n";
        assert_eq!(folder_reads(text), [(3, "\"HOME\""), (4, "dirs::")]);
        assert!(is_folders_module(Path::new(
            r"C:\dm\crates/dm-core/src\folders.rs"
        )));
        assert!(!is_folders_module(Path::new(
            "crates/dm-shell/src/settings.rs"
        )));
    }

    #[test]
    fn nothing_phones_home() {
        if let Err(why) = privacy() {
            panic!("{why}");
        }
    }

    #[test]
    fn every_source_file_carries_its_licence_header() {
        if let Err(why) = licence_headers() {
            panic!("{why}");
        }
    }

    #[test]
    fn no_credential_is_written_into_a_tracked_file() {
        if let Err(why) = secrets() {
            panic!("{why}");
        }
    }

    #[test]
    fn a_written_out_credential_would_be_caught() {
        let login = ["https://me:", &test_secrets::secret("password"), "@e.com/f"].concat();
        assert_eq!(secrets_in(&login), [(1, "a login written into an address")]);
        let placeholder = ["format!(\"http://me:", "{}", "@{addr}/f\")"].concat();
        assert!(secrets_in(&placeholder).is_empty());
        let header = ["Authorization: Basic ", &test_secrets::secret("token")].concat();
        assert_eq!(secrets_in(&header), [(1, "a Basic authorization token")]);
        assert!(secrets_in("Basic realm=\"x\"; Basic authentication").is_empty());
        let key = ["-----BEGIN ", "PRIVATE KEY-----"].concat();
        assert_eq!(secrets_in(&key), [(1, "a private key")]);
        let token = ["ghp_", &"a".repeat(36)].concat();
        assert_eq!(secrets_in(&token), [(1, "an access token")]);
        assert!(secrets_in("see http://[::1]:9/a and ftp://ftp.e.com/x").is_empty());
    }

    #[test]
    fn a_reporter_or_a_stray_address_would_be_caught() {
        assert_eq!(
            hosts_in(r#"let u = "https://stats.example.org/x";"#),
            ["stats.example.org"]
        );
        assert!(hosts_in(r#"format!("https://{host}/")"#).is_empty());
        assert!(REPORTERS.contains(&"sentry"));
    }

    /// A folder of its own under the system's temporary one, removed when
    /// dropped (xtask has no crate for it).
    pub(crate) struct Scratch(pub(crate) PathBuf);

    impl Scratch {
        pub(crate) fn new(what: &str) -> Scratch {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let dir =
                std::env::temp_dir().join(format!("xtask-{what}-{}-{nanos}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_walk_finds_source_files_and_passes_over_built_glue() {
        let dir = Scratch::new("walk");
        let root = &dir.0;
        std::fs::create_dir_all(root.join("pkg")).unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        for file in [
            "a.rs",
            "b.txt",
            "pkg/glue.js",
            "sub/c.json",
            "sub/d.html",
            "sub/e.md",
        ] {
            std::fs::write(root.join(file), "").unwrap();
        }
        let mut found: Vec<String> = walk(root)
            .iter()
            .map(|p| {
                // Windows names them with `\`.
                let rel = p.strip_prefix(root).unwrap();
                rel.iter()
                    .map(|part| part.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .collect();
        found.sort();
        assert_eq!(found, ["a.rs", "sub/c.json", "sub/d.html"]);
        assert!(walk(&root.join("not there")).is_empty());
    }

    #[test]
    fn a_long_inline_test_module_is_found_and_a_short_one_or_a_file_is_not() {
        let short = "fn a() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn b() {}\n}\n";
        assert!(long_inline_tests(short).is_empty());
        let body = "    // a line\n".repeat(MAX_INLINE_TEST_LINES + 1);
        let long = format!("#[cfg(test)]\nmod more_tests {{\n{body}}}\n");
        assert_eq!(
            long_inline_tests(&long),
            [("more_tests".to_string(), MAX_INLINE_TEST_LINES + 1)]
        );
        // Not a test's, or a file of its own.
        assert!(long_inline_tests(&format!("mod inner {{\n{body}}}\n")).is_empty());
        assert!(long_inline_tests("#[cfg(test)]\nmod tests;\n").is_empty());
    }

    #[test]
    fn a_manifest_names_its_crate_and_every_dependency_but_the_tests() {
        let dir = Scratch::new("manifest");
        let path = dir.0.join("Cargo.toml");
        std::fs::write(
            &path,
            "[package]\nname = \"dm-x\"\n\n[dependencies]\ndm-core = \"1\"\n\n\
             [dev-dependencies]\ntempfile = \"3\"\n\n\
             [target.'cfg(windows)'.dependencies]\nwindows-sys = \"1\"\n",
        )
        .unwrap();
        let read = manifest(&path).unwrap();
        assert_eq!(read.name, "dm-x");
        assert_eq!(read.deps, ["dm-core", "windows-sys"]);
        assert_eq!(read.own_versions, ["dm-core", "tempfile", "windows-sys"]);

        // Taken from the workspace, with or without features of its own: none written here.
        std::fs::write(
            &path,
            "[package]\nname = \"dm-x\"\n\n[dependencies]\ndm-core.workspace = true\n\
             tokio = { workspace = true, features = [\"fs\"] }\n\n\
             [build-dependencies]\nembed-resource = { version = \"3\" }\n",
        )
        .unwrap();
        assert_eq!(manifest(&path).unwrap().own_versions, ["embed-resource"]);

        std::fs::write(&path, "[package\n").unwrap();
        assert!(manifest(&path).err().unwrap().contains("Cargo.toml"));
        assert!(manifest(&dir.0.join("none.toml")).is_err());
    }

    #[test]
    fn the_fuzz_files_are_counted_by_their_tests_and_a_missing_one_is_none() {
        let dir = Scratch::new("fuzz");
        let file = dir.0.join("crates/dm-core/tests/fuzz.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "#[test]\nfn a() {}\n#[test]\nfn b() {}\n").unwrap();
        let counted = fuzz_targets(&dir.0);
        assert_eq!(counted.len(), 6);
        assert_eq!(counted[0], ("crates/dm-core/tests/fuzz.rs".into(), 2));
        assert!(counted[1..].iter().all(|(_, n)| *n == 0));
    }
}
