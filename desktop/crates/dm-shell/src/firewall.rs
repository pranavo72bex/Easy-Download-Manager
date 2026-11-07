// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Windows' firewall and dm's torrents (gaps rows 101, 107). The first torrent dm runs listens
//! for peers, and Windows then asks, once, in a window of its own that can sit behind others,
//! whether dm may use public and private networks. dm cannot answer it and does not try: it says
//! on the row that Windows may be asking, until Windows has an answer for dm's program.
//!
//! Windows keeps each answer (allowed or blocked) as a firewall rule naming the program, readable
//! without administrator rights; no rule for dm's program means the question is still to come.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Where Windows keeps its firewall rules.
pub const RULES: &str =
    r"HKLM\SYSTEM\CurrentControlSet\Services\SharedAccess\Parameters\FirewallPolicy\FirewallRules";

/// Whether the rules (`reg query`'s lines, each rule's fields joined by `|`) hold one for
/// `program`: `App=` its path, compared as Windows compares paths, whatever case.
pub fn answered_in(rules: &str, program: &Path) -> bool {
    let want = format!("app={}", program.display()).to_lowercase();
    rules.lines().any(|line| {
        line.to_lowercase()
            .split('|')
            .any(|field| field.trim() == want)
    })
}

/// Whether Windows has an answer for `program` yet; `None` where its rules cannot be read.
#[cfg(target_os = "windows")]
pub fn answered(program: &Path) -> Option<bool> {
    let out = dm_core::no_window(&mut std::process::Command::new("reg"))
        .args(["query", RULES])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| answered_in(&String::from_utf8_lossy(&out.stdout), program))
}

/// Elsewhere there is no such question.
#[cfg(not(target_os = "windows"))]
pub fn answered(_program: &Path) -> Option<bool> {
    Some(true)
}

/// Not read yet, answered, not answered.
const UNKNOWN: u8 = 0;
const YES: u8 = 1;
const NO: u8 = 2;
static STATE: AtomicU8 = AtomicU8::new(UNKNOWN);
static READING: AtomicBool = AtomicBool::new(false);
static LAST: Mutex<Option<Instant>> = Mutex::new(None);

/// How often the rules are read again while asked: the answer arrives while a torrent waits.
const AGAIN: Duration = Duration::from_secs(4);

/// Whether Windows may be asking now, or will be once a torrent listens: no answer for dm's own
/// program yet. Cheap enough for every frame: the rules are read on a thread of their own, again
/// every few seconds while this is asked, and the last reading is said meanwhile (none yet:
/// not asking). Once answered it stays answered.
pub fn may_ask() -> bool {
    let state = STATE.load(Ordering::Relaxed);
    if state == YES {
        return false;
    }
    let due = dm_core::lock(&LAST).is_none_or(|at| at.elapsed() >= AGAIN);
    if due && !READING.swap(true, Ordering::AcqRel) {
        *dm_core::lock(&LAST) = Some(Instant::now());
        std::thread::spawn(|| {
            let found = std::env::current_exe().ok().and_then(|exe| answered(&exe));
            match found {
                Some(true) | None => STATE.store(YES, Ordering::Relaxed),
                Some(false) => STATE.store(NO, Ordering::Relaxed),
            }
            READING.store(false, Ordering::Release);
        });
    }
    state == NO
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rule_naming_the_program_is_an_answer_whatever_its_case() {
        let rules = "\
HKEY_LOCAL_MACHINE\\SYSTEM\\...\\FirewallRules
    TCP Query User{1D97}C:\\Apps\\dm\\dm-desktop.exe    REG_SZ    v2.10|Action=Block|Active=TRUE|Dir=In|Protocol=6|Profile=Public|App=C:\\Apps\\dm\\DM-Desktop.exe|Name=dm-desktop|
    Other    REG_SZ    v2.10|Action=Allow|App=C:\\Apps\\dm\\dm-desktop.exe.old|Name=x|
";
        assert!(answered_in(rules, Path::new(r"C:\Apps\dm\dm-desktop.exe")));
        // Another program, or one whose path only starts the same: no answer.
        assert!(!answered_in(rules, Path::new(r"C:\Apps\dm\dm.exe")));
        assert!(!answered_in(
            "    X    REG_SZ    v2.10|App=C:\\Apps\\dm\\dm-desktop.exe.old|",
            Path::new(r"C:\Apps\dm\dm-desktop.exe")
        ));
        assert!(!answered_in("", Path::new(r"C:\Apps\dm\dm-desktop.exe")));
    }
}
