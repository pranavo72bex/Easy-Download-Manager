// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The crate's one error type (gaps row A9): the engine's own sentence, kept word for word.

use std::fmt;

/// Why the torrent engine did not do what it was asked, in the engine's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

impl Error {
    /// The sentence, as it is shown.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<String> for Error {
    fn from(said: String) -> Error {
        Error(said)
    }
}

impl From<&str> for Error {
    fn from(said: &str) -> Error {
        Error(said.to_string())
    }
}

impl From<Error> for String {
    fn from(error: Error) -> String {
        error.0
    }
}
