// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The crate's one error type (gaps row A9), and dm-desktop-base's, which re-exports it: what
//! went wrong, in the sentence a person reads, byte for byte as it was when it was a `String`.
//! It reads as that text where text is wanted (`Deref` to `str`, compared with a `str`), and
//! turns into a `String` where a caller keeps one.

use std::fmt;

/// What went wrong, said in the words a person reads. Shown (`Display`) as exactly those words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

impl Error {
    /// `e`'s words as this crate's error: an error of another crate, whichever type that crate
    /// gives it, taken as its `Display` says it.
    pub fn from_display(e: impl fmt::Display) -> Error {
        Error(e.to_string())
    }

    /// The words.
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

impl std::ops::Deref for Error {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl From<String> for Error {
    fn from(words: String) -> Error {
        Error(words)
    }
}

impl From<&str> for Error {
    fn from(words: &str) -> Error {
        Error(words.to_string())
    }
}

impl From<Error> for String {
    fn from(e: Error) -> String {
        e.0
    }
}

/// The engine's error, as its `to_string()` says it.
impl From<dm_core::Error> for Error {
    fn from(e: dm_core::Error) -> Error {
        Error(e.to_string())
    }
}

/// The application's error, its words as they are.
impl From<dm_shell::Error> for Error {
    fn from(e: dm_shell::Error) -> Error {
        Error(String::from(e))
    }
}

impl PartialEq<str> for Error {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for Error {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<String> for Error {
    fn eq(&self, other: &String) -> bool {
        &self.0 == other
    }
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn the_error_is_its_words_byte_for_byte() {
        let e = Error::from("the folder could not be made: denied");
        assert_eq!(e.to_string(), "the folder could not be made: denied");
        assert_eq!(e, "the folder could not be made: denied");
        assert!(e.contains("denied"));
        assert_eq!(String::from(e.clone()), e.as_str());
        assert_eq!(Error::from_display(404), "404");
        assert_eq!(Error::from(dm_core::Error::Internal("said".into())), "said");
    }
}
