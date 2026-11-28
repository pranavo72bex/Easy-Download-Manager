// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The passwords a person keeps for archives (gaps row C2): tried in turn on a finished archive
//! that has one, the one that opened it moved to the top. Kept in EDM's home, sealed under the
//! operating system's store for secrets where there is one, as the sign-ins are
//! (`secretstore`); never written to the settings, a log or a backup.

use std::path::Path;

use crate::secretstore::{self, Opened};

/// The passwords kept in `home`, the one tried first first; none where there are none, or the
/// file cannot be opened.
pub fn load(home: &Path) -> Vec<String> {
    let Ok(bytes) = std::fs::read(home.join(crate::paths::ARCHIVE_PASSWORDS)) else {
        return Vec::new();
    };
    let text = match secretstore::opened(&bytes) {
        Opened::Plain => String::from_utf8_lossy(&bytes).into_owned(),
        Opened::Opened(plain) => String::from_utf8_lossy(&plain).into_owned(),
        Opened::Failed(why) => {
            crate::note!(
                warn,
                "EDM: the archive passwords could not be opened: {why}"
            );
            return Vec::new();
        }
    };
    tidy(text.lines().map(str::to_string))
}

/// `passwords` kept in `home`, in their order, each once and none empty; the file goes where
/// there are none.
pub fn save(home: &Path, passwords: &[String]) -> std::io::Result<()> {
    let file = home.join(crate::paths::ARCHIVE_PASSWORDS);
    let kept = tidy(passwords.iter().cloned());
    if kept.is_empty() {
        return match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    std::fs::create_dir_all(home)?;
    let bytes = secretstore::sealed(kept.join("\n").into_bytes());
    crate::files::write_atomic(&file, &bytes)
}

/// `used`, which just opened an archive, moved to the top of the passwords kept in `home`.
pub fn used(home: &Path, used: &str) {
    let mut all = load(home);
    let Some(at) = all.iter().position(|p| p == used) else {
        return;
    };
    if at == 0 {
        return;
    }
    let first = all.remove(at);
    all.insert(0, first);
    if let Err(e) = save(home, &all) {
        crate::note!(
            warn,
            "EDM: the archive passwords' order could not be kept: {e}"
        );
    }
}

/// `passwords` each once, in their order, none empty, a line's end taken off (a password is
/// kept as typed otherwise: a space can be part of one).
fn tidy(passwords: impl Iterator<Item = String>) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for password in passwords {
        let password = password.trim_end_matches(['\r', '\n']).to_string();
        if !password.is_empty() && !kept.contains(&password) {
            kept.push(password);
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_are_kept_once_each_in_order_and_the_one_used_goes_first() {
        let home = tempfile::tempdir().unwrap();
        assert!(load(home.path()).is_empty());
        let given = ["first", "", "second one", "first", "third"].map(String::from);
        save(home.path(), &given).unwrap();
        assert_eq!(load(home.path()), ["first", "second one", "third"]);
        used(home.path(), "third");
        assert_eq!(load(home.path()), ["third", "first", "second one"]);
        // One not kept changes nothing.
        used(home.path(), "never kept");
        assert_eq!(load(home.path()), ["third", "first", "second one"]);
        save(home.path(), &[]).unwrap();
        assert!(!home.path().join(crate::paths::ARCHIVE_PASSWORDS).exists());
    }
}
