// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
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
}

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
        }
    }

    /// Whether a password was given.
    pub(super) fn given(&self) -> bool {
        self.password.is_some()
    }

    /// The key for `aes`, made once; `None` with no password.
    pub(super) fn key(&self, aes: &SevenAes) -> Option<[u8; 32]> {
        let password = self.password.as_ref()?;
        let mut made = self.made.borrow_mut();
        if let Some(k) = made.iter().find(|k| k.of(aes)) {
            return Some(k.key);
        }
        let key = aes.key(password);
        made.push(Made {
            cycles: aes.cycles,
            salt: aes.salt.clone(),
            key,
            proved: false,
        });
        Some(key)
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

/// A folder's AES settings, if it has an AES coder dm decrypts, or why dm does not read
/// them.
pub(super) fn aes_of(folder: &Folder) -> Result<Option<SevenAes>, Error> {
    folder
        .aes
        .as_ref()
        .map(|a| SevenAes::parse(&a.props).map_err(Error::Method))
        .transpose()
}
