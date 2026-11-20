// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The crate's one error type (gaps row A9): why a document is not one this side reads, in
//! the sentence the log shows.

use std::fmt;

/// Why something sent was not read, in words.
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

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn the_sentence_is_kept_as_it_is_shown() {
        let error = Error::from("not a document dm reads");
        assert_eq!(error.as_str(), "not a document dm reads");
        assert_eq!(error.to_string(), "not a document dm reads");
        assert_eq!(Error::from(String::from("a")), Error::from("a"));
        let said: String = error.into();
        assert_eq!(said, "not a document dm reads");
        let boxed: Box<dyn std::error::Error> = Box::new(Error::from("b"));
        assert_eq!(boxed.to_string(), "b");
    }
}
