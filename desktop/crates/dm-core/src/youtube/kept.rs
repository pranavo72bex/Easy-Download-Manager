// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! YouTube's player scripts and their cut programs, kept: for the session, and the cut programs
//! on the disk between sessions (gaps row Y13), so a playlist's videos, and the next day's
//! first video, do not fetch and cut the same player again.

use std::path::Path;

use super::player;

/// How many player scripts, and their cut programs, are kept: the one in use and the one
/// before it, a few megabytes each.
const PLAYERS_KEPT: usize = 2;

/// Player scripts fetched this session, by their address, newest last (gaps row Y13).
static SCRIPTS: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

/// The cut programs of player scripts this session, by the script's own hash, newest last.
static PROGRAMS: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

pub(super) fn kept_script(url: &str) -> Option<String> {
    crate::lock(&SCRIPTS)
        .iter()
        .find(|(kept, _)| kept == url)
        .map(|(_, text)| text.clone())
}

pub(super) fn keep_script(url: &str, text: &str) {
    let mut kept = crate::lock(&SCRIPTS);
    kept.retain(|(old, _)| old != url);
    kept.push((url.to_string(), text.to_string()));
    let over = kept.len().saturating_sub(PLAYERS_KEPT);
    kept.drain(..over);
}

/// `player_js` cut down to what answers YouTube's checks (`player::program`), cut once for
/// each player rather than for every video: kept for the session, and on the disk where dm
/// says where ([`set_kept_players`]), so the next session's first video does not cut it again
/// (gaps row Y13).
pub(super) fn cut_program(player_js: &str) -> Result<String, String> {
    let key = key_of(player_js);
    if let Some((_, program)) = crate::lock(&PROGRAMS).iter().find(|(k, _)| *k == key) {
        return Ok(program.clone());
    }
    let on_disk = crate::lock(&KEPT_PLAYERS)
        .as_ref()
        .map(|dir| dir.join(format!("{key}.js")));
    let read = on_disk
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .filter(|program| !program.is_empty());
    let program = match read {
        Some(program) => program,
        None => {
            let program = player::program(player_js)?;
            if let Some(path) = &on_disk {
                keep_on_disk(path, &program);
            }
            program
        }
    };
    let mut kept = crate::lock(&PROGRAMS);
    kept.push((key, program.clone()));
    let over = kept.len().saturating_sub(PLAYERS_KEPT);
    kept.drain(..over);
    Ok(program)
}

/// A player's name for its cut program: the script's own digest and this dm's version, since a
/// later dm cuts differently.
fn key_of(player_js: &str) -> String {
    use sha2::Digest as _;
    let digest = sha2::Sha256::new()
        .chain_update(env!("CARGO_PKG_VERSION").as_bytes())
        .chain_update(player_js.as_bytes())
        .finalize();
    digest.iter().take(16).map(|b| format!("{b:02x}")).collect()
}

/// Where cut player programs are kept between sessions; none, only in memory.
static KEPT_PLAYERS: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);

/// The folder cut player programs are kept in between sessions (dm's own, under its home), or
/// none. Set once at start.
pub fn set_kept_players(dir: Option<std::path::PathBuf>) {
    *crate::lock(&KEPT_PLAYERS) = dir;
}

/// `program` written to `path`, whole or not at all (a temporary name, then moved), and the
/// folder trimmed to the newest few: a player changes every week or two.
fn keep_on_disk(path: &Path, program: &str) {
    let Some(dir) = path.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let partial = path.with_extension("partial");
    if std::fs::write(&partial, program).is_err() || std::fs::rename(&partial, path).is_err() {
        let _ = std::fs::remove_file(&partial);
        return;
    }
    let mut kept: Vec<(std::time::SystemTime, std::path::PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "js"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    kept.sort();
    let over = kept.len().saturating_sub(2 * PLAYERS_KEPT);
    for (_, old) in kept.into_iter().take(over) {
        let _ = std::fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A player cut in an earlier session is read from the disk, not cut again; the folder keeps
    /// the newest four (gaps row Y13).
    #[test]
    fn a_cut_player_is_kept_on_the_disk_between_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let players = dir.path().join("players");
        std::fs::create_dir_all(&players).unwrap();
        // Not a player that could be cut: only what the disk keeps answers for it.
        let script = "var not_a_player = 1; // a disk test's";
        std::fs::write(
            players.join(format!("{}.js", key_of(script))),
            "kept program",
        )
        .unwrap();
        set_kept_players(Some(players.clone()));
        let read = cut_program(script);
        set_kept_players(None);
        assert_eq!(read.as_deref(), Ok("kept program"));

        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        for n in 0..6 {
            let path = players.join(format!("old{n}.js"));
            std::fs::write(&path, "old").unwrap();
            let file = std::fs::File::options().write(true).open(&path).unwrap();
            file.set_modified(old + std::time::Duration::from_secs(n))
                .unwrap();
        }
        let newest = players.join("newest.js");
        keep_on_disk(&newest, "new program");
        assert_eq!(std::fs::read_to_string(&newest).unwrap(), "new program");
        let left = std::fs::read_dir(&players).unwrap().count();
        assert_eq!(left, 2 * PLAYERS_KEPT, "the oldest were not let go");
    }
}
