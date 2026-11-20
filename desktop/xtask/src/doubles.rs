// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The doubles ratchet (gaps row A18), part of `cargo xtask layers`: a crate built in more
//! than one version costs size and build time, so the number of such crates only ever falls.
//!
//! - Every package of `desktop/Cargo.lock` is counted by its name: every system's, the build
//!   scripts' and the tests'. A name the lock holds in more than one version is a double.
//! - The doubles when the ratchet came in are named, with their versions, in
//!   `desktop/xtask/doubles-baseline.txt`. A double the baseline does not name fails, and so
//!   does one in more versions than its entry lists; one in fewer, or in one version now, is
//!   said, and `cargo xtask doubles-baseline` shortens or removes its entry. Only
//!   `cargo xtask doubles-baseline --grow` adds an entry or lengthens one.
//! - What is held is how many versions, not which: an update that moves a double's versions,
//!   as many as before, fails nothing, and the rewrite records the versions it moved to.
//!
//! `desktop/deny.toml` says the same doubles as warnings (`multiple-versions = "warn"`); this
//! keeps their list from growing.

use std::collections::BTreeMap;
use std::path::Path;

/// The lock the doubles are counted from, from the repository's root.
const LOCK: &str = "desktop/Cargo.lock";

/// The doubles allowed, from the repository's root.
const BASELINE: &str = "desktop/xtask/doubles-baseline.txt";

/// What heads [`BASELINE`]: what it is and how it changes.
const HEADER: &str = "\
# The doubles ratchet's baseline (gaps row A18; `cargo xtask layers`, xtask/src/doubles.rs).
# Each crate name desktop/Cargo.lock holds in more than one version, with those versions:
# `<name> <version> <version>...`, the names in order, each one's versions oldest first. Every
# package of the lock is counted: every system's, the build scripts' and the tests'. No name
# may be added and none may gain a version: an entry is only ever shortened or removed, which
# `cargo xtask doubles-baseline` does from the lock as it is now. Only `--grow` adds or
# lengthens one, for a reason its commit gives.
";

/// What `cargo xtask doubles-baseline` says when given anything but `--grow`.
const USAGE: &str = "usage: cargo xtask doubles-baseline [--grow]";

/// Each crate's versions, oldest first, by its name: what the lock holds, or what the baseline
/// allows.
pub(crate) type Versions = BTreeMap<String, Vec<String>>;

/// What the ratchet finds: what fails it, and what only asks for the baseline to be shortened.
#[derive(Default, Debug)]
pub(crate) struct Verdict {
    /// A double new, or in more versions than allowed: each fails `cargo xtask layers`.
    pub(crate) wrong: Vec<String>,
    /// An entry that can be shortened, removed or brought up to date; nothing fails for it.
    pub(crate) notes: Vec<String>,
}

/// The ratchet as `cargo xtask layers` runs it: each double too many, a sentence. What only
/// asks for the baseline to be shortened is printed to the error stream and fails nothing.
pub(crate) fn check(root: &Path) -> Result<Vec<String>, String> {
    let now = doubles(locked(&crate::read(&root.join(LOCK))?)?);
    let allowed = parse(&crate::read(&root.join(BASELINE))?)?;
    let verdict = judge(&now, &allowed);
    for note in &verdict.notes {
        eprintln!("note: {note}");
    }
    Ok(verdict.wrong)
}

/// `cargo xtask doubles-baseline [--grow]`: the baseline written again from the lock. Without
/// `--grow` an entry is only shortened, brought up to date or removed, and a double new or
/// grown is left as the baseline had it (so `layers` still fails for it); with `--grow` the
/// baseline is the lock's doubles as they are.
pub(crate) fn rewrite(root: &Path, args: &[String]) -> Result<String, String> {
    let grow = match args {
        [] => false,
        [flag] if flag == "--grow" => true,
        _ => return Err(USAGE.to_string()),
    };
    let now = doubles(locked(&crate::read(&root.join(LOCK))?)?);
    let path = root.join(BASELINE);
    let allowed = parse(&crate::read(&path)?)?;
    let (written, grown) = if grow {
        (now, Vec::new())
    } else {
        shrunk(&now, &allowed)
    };
    std::fs::write(&path, render(&written)).map_err(|e| format!("{}: {e}", path.display()))?;
    let count = |v: &Versions| (v.len(), v.values().map(Vec::len).sum::<usize>());
    let ((names_then, versions_then), (names_now, versions_now)) =
        (count(&allowed), count(&written));
    let mut said = format!(
        "{BASELINE} names {names_now} crates in {versions_now} versions (was {names_then} in \
         {versions_then})"
    );
    if grown.is_empty() {
        said.push_str("; no entry was added or lengthened");
    } else {
        said.push_str(&format!(
            "; left as they were, as they grew (`--grow` takes them in): {}",
            grown.join(", ")
        ));
    }
    Ok(said)
}

/// Every package `lock` (a `Cargo.lock`'s text) holds, its versions by its name, oldest first.
pub(crate) fn locked(lock: &str) -> Result<Versions, String> {
    let lock: toml::Table = lock.parse().map_err(|e| format!("{LOCK}: {e}"))?;
    let packages = lock
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| format!("{LOCK} has no packages"))?;
    let mut versions = Versions::new();
    for package in packages {
        let field = |key: &str| package.get(key).and_then(toml::Value::as_str);
        let (Some(name), Some(version)) = (field("name"), field("version")) else {
            return Err(format!("{LOCK}: a package without a name or a version"));
        };
        versions
            .entry(name.to_string())
            .or_default()
            .push(version.to_string());
    }
    for list in versions.values_mut() {
        list.sort_by_key(|v| order(v));
    }
    Ok(versions)
}

/// The names of `all` in more than one version, with those versions.
pub(crate) fn doubles(all: Versions) -> Versions {
    all.into_iter().filter(|(_, v)| v.len() > 1).collect()
}

/// What a version is sorted by: its numbers (`1.2.3`), each compared as a number, then what
/// follows them (`-zed`, `+spec-1.1.0`) as text. A part that is not a number goes last.
fn order(version: &str) -> (Vec<u64>, String) {
    let (core, rest) = version.split_at(version.find(['-', '+']).unwrap_or(version.len()));
    let numbers: Vec<u64> = core
        .split('.')
        .map(|n| n.parse().unwrap_or(u64::MAX))
        .collect();
    (numbers, rest.to_string())
}

/// The baseline from its text: `<name> <version> <version>...` a line, with blank lines and
/// those starting with `#` passed over.
pub(crate) fn parse(text: &str) -> Result<Versions, String> {
    let mut allowed = Versions::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace().map(str::to_string);
        let name = words.next().unwrap_or_default();
        let versions: Vec<String> = words.collect();
        if versions.len() < 2 {
            return Err(format!(
                "{BASELINE}:{}: `{line}` is not a crate's name and two versions or more",
                n + 1
            ));
        }
        if allowed.insert(name, versions).is_some() {
            return Err(format!(
                "{BASELINE}:{}: `{line}` names a crate a line above names too",
                n + 1
            ));
        }
    }
    Ok(allowed)
}

/// The baseline's text: its header, then a line for each double in order.
pub(crate) fn render(doubles: &Versions) -> String {
    let mut text = HEADER.to_string();
    for (name, versions) in doubles {
        text.push_str(&format!("{name} {}\n", versions.join(" ")));
    }
    text
}

/// What is doubled in `now` beyond what `allowed` lists, and which entries can be shortened,
/// removed or brought up to date.
pub(crate) fn judge(now: &Versions, allowed: &Versions) -> Verdict {
    let mut verdict = Verdict::default();
    let does = "`cargo xtask doubles-baseline` does it";
    let who = "`cargo tree -d --target all` in desktop/ says which crates ask for each";
    for (name, versions) in now {
        let (count, listed) = (versions.len(), versions.join(", "));
        match allowed.get(name) {
            None => verdict.wrong.push(format!(
                "{name} is in {LOCK} in {count} versions ({listed}), and {BASELINE} does not name \
                 it: no crate may come in a second version, so ask for the version already there \
                 ({who}); only if it cannot be helped, `cargo xtask doubles-baseline --grow`, \
                 the reason in the commit"
            )),
            Some(was) if count > was.len() => verdict.wrong.push(format!(
                "{name} is in {LOCK} in {count} versions ({listed}), more than the {} {BASELINE} \
                 lists: a crate's versions may only become fewer, so ask for one already there \
                 ({who}); only if it cannot be helped, `cargo xtask doubles-baseline --grow`, \
                 the reason in the commit",
                was.len()
            )),
            Some(was) if count < was.len() => verdict.notes.push(format!(
                "{name} is down to {count} versions ({listed}) from {}: shorten its entry in \
                 {BASELINE} ({does})",
                was.len()
            )),
            Some(was) if was != versions => verdict.notes.push(format!(
                "{name} is in versions {listed} now, where {BASELINE} lists {}: bring its entry \
                 up to date ({does})",
                was.join(", ")
            )),
            Some(_) => {}
        }
    }
    for name in allowed.keys() {
        if !now.contains_key(name) {
            verdict.notes.push(format!(
                "{name} is in one version now, or gone: remove its entry from {BASELINE} \
                 ({does})"
            ));
        }
    }
    verdict
}

/// `allowed` brought down to `now`: an entry still doubled in as many versions or fewer takes
/// the versions now, one in a single version (or gone) is removed. An entry grown is kept as it
/// was and none is added; their names are given back, each with its versions now.
pub(crate) fn shrunk(now: &Versions, allowed: &Versions) -> (Versions, Vec<String>) {
    let mut written = Versions::new();
    let mut grown = Vec::new();
    for (name, was) in allowed {
        let Some(versions) = now.get(name) else {
            continue;
        };
        if versions.len() <= was.len() {
            written.insert(name.clone(), versions.clone());
        } else {
            written.insert(name.clone(), was.clone());
            grown.push(format!("{name} {}", versions.join(" ")));
        }
    }
    for (name, versions) in now {
        if !allowed.contains_key(name) {
            grown.push(format!("{name} {}", versions.join(" ")));
        }
    }
    (written, grown)
}

#[cfg(test)]
mod tests;
