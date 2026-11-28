// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window's one error type (gaps row A9): why something the window asked of the system (the
//! tray, the control port, moving or picturing a window) did not happen, in the sentence a
//! person or a script sees, kept word for word.

use std::fmt;

/// Why the system did not do what the window asked, in words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

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
