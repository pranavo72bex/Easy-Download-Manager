// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A 7z's password and the AES keys made from it (gaps row F10): each key once, as making one
//! takes half a million rounds of SHA-256, and whether it has been proved.

use std::cell::RefCell;

use super::super::crypt::{utf16le, SevenAes};
use super::header::Folder;
use super::Error;

/// The password, as a 7z key is made from it, and the keys made so far: one for each
/// rounds-and-salt an AES coder names (one in all in an archive 7-Zip wrote).
pub(super) struct Keys {
    password: Option<Vec<u8>>,
    made: RefCell<Vec<Made>>,
    /// The rounds of SHA-256 spent making keys for this archive so far.
    spent: std::cell::Cell<u64>,
}

/// The most rounds of SHA-256 one archive may have spent on its keys: eight keys of 2^24
/// rounds, a few seconds. An archive of 200,000 folders, each salted for a key of its own,
/// held the CPU for days once a saved password was tried (gaps rows H10, H35); a real one
/// has one key, or a few.
const MOST_ROUNDS: u64 = 8 << 24;

struct Made {
    cycles: u8,
    salt: Vec<u8>,
    key: [u8; 32],
    proved: bool,
}

impl Made {
    fn of(&self, aes: &SevenAes) -> bool {
        self.cycles == aes.cycles && self.salt == aes.salt
    }
}

impl Keys {
    pub(super) fn new(password: Option<&str>) -> Keys {
        Keys {
            password: password.map(utf16le),
            made: RefCell::new(Vec::new()),
            spent: std::cell::Cell::new(0),
        }
    }

    /// Whether a password was given.
    pub(super) fn given(&self) -> bool {
        self.password.is_some()
    }

    /// The key for `aes`, made once; `Ok(None)` with no password; an error once the archive's
    /// keys have taken [`MOST_ROUNDS`].
    pub(super) fn key(&self, aes: &SevenAes) -> Result<Option<[u8; 32]>, crate::extract::Error> {
        let Some(password) = self.password.as_ref() else {
            return Ok(None);
        };
        let mut made = self.made.borrow_mut();
        if let Some(k) = made.iter().find(|k| k.of(aes)) {
            return Ok(Some(k.key));
        }
        let rounds = if aes.cycles == 0x3F {
            1
        } else {
            1u64 << aes.cycles
        };
        let spent = self.spent.get().saturating_add(rounds);
        if spent > MOST_ROUNDS {
            return Err(crate::extract::Error::Unreadable(
                "the archive asks for more password keys than any archive needs".into(),
            ));
        }
        self.spent.set(spent);
        let key = aes.key(password);
        made.push(Made {
            cycles: aes.cycles,
            salt: aes.salt.clone(),
            key,
            proved: false,
        });
        Ok(Some(key))
    }

    /// Whether the key for `aes` has opened something whose checksum then added up.
    pub(super) fn proved(&self, aes: &SevenAes) -> bool {
        self.made.borrow().iter().any(|k| k.of(aes) && k.proved)
    }

    pub(super) fn prove(&self, aes: &SevenAes) {
        for k in self.made.borrow_mut().iter_mut().filter(|k| k.of(aes)) {
            k.proved = true;
        }
    }
}

/// A folder's AES settings, if it has an AES coder EDM decrypts, or why EDM does not read
/// them.
pub(super) fn aes_of(folder: &Folder) -> Result<Option<SevenAes>, Error> {
    folder
        .aes
        .as_ref()
        .map(|a| SevenAes::parse(&a.props))
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Past the most rounds an archive's keys may take, the next key is refused rather than
    /// made; one already made is still given (gaps rows H10, H35).
    #[test]
    fn an_archive_of_endless_keys_is_refused_once_they_have_taken_too_long() {
        let aes = |salt: u8| {
            let mut props = vec![0xC0 | 2, (1 << 4) | 15, salt, salt];
            props.extend([5u8; 16]);
            crate::extract::crypt::SevenAes::parse(&props).unwrap()
        };
        let keys = Keys::new(Some("pw"));
        assert!(keys.key(&aes(1)).unwrap().is_some());
        // As if many folders' keys had been made already.
        keys.spent.set(MOST_ROUNDS - 4);
        assert!(
            keys.key(&aes(1)).unwrap().is_some(),
            "a key made is not made again"
        );
        assert!(keys.key(&aes(2)).unwrap().is_some(), "4 rounds more fit");
        assert!(
            keys.key(&aes(3)).is_err(),
            "past the most, a new key is refused"
        );
        assert!(Keys::new(None).key(&aes(4)).unwrap().is_none());
    }
}
