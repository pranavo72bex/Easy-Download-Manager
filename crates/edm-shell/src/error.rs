// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! edm-shell's one error type: what went wrong, in the sentence a person is shown.
//!
//! edm-shell's failures are told apart by no caller, only said, so the type holds that sentence
//! and nothing else (gaps row A9). It is shown exactly as written, and reads as text where the
//! window and the tests read it (`contains`, `==` against a sentence, `?` into a `String`), so
//! the sentence a person sees is the one it held as a `String`.

use std::fmt;

/// What went wrong, in the words a person is shown.
#[derive(Clone, PartialEq, Eq, Hash, Default)]
pub struct Error(String);

impl Error {
    /// The error saying `said`.
    pub fn new(said: impl Into<String>) -> Self {
        Self(said.into())
    }

    /// The sentence, as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The sentence, as an owned `String`.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// As a `String`'s: a test's or a log's `{:?}` reads as it did.
impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl std::error::Error for Error {}

/// The sentence, read as text: `contains`, `starts_with`, `&*why`.
impl std::ops::Deref for Error {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for Error {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for Error {
    fn from(said: String) -> Self {
        Self(said)
    }
}

impl From<&str> for Error {
    fn from(said: &str) -> Self {
        Self(said.to_string())
    }
}

impl From<&String> for Error {
    fn from(said: &String) -> Self {
        Self(said.clone())
    }
}

/// The engine's error, in its own words (`to_string`, as edm-shell said it).
impl From<edm_core::Error> for Error {
    fn from(e: edm_core::Error) -> Self {
        Self(e.to_string())
    }
}

/// Back to the sentence, for a caller that keeps its errors as text.
impl From<Error> for String {
    fn from(e: Error) -> Self {
        e.0
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
        self.0 == *other
    }
}

impl PartialEq<Error> for str {
    fn eq(&self, other: &Error) -> bool {
        self == other.0
    }
}

impl PartialEq<Error> for &str {
    fn eq(&self, other: &Error) -> bool {
        *self == other.0
    }
}

impl PartialEq<Error> for String {
    fn eq(&self, other: &Error) -> bool {
        *self == other.0
    }
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn it_says_exactly_its_sentence_and_compares_as_text() {
        let e = Error::from(format!("{} is not there", "x"));
        assert_eq!(e.to_string(), "x is not there");
        assert_eq!(format!("{e:?}"), format!("{:?}", "x is not there"));
        assert_eq!(e, "x is not there");
        assert!("x is not there" == e && "x is not there"[..] == e);
        assert_eq!(e, "x is not there".to_string());
        assert_eq!("x is not there".to_string(), e);
        assert!(e.contains("not there") && e.as_str().starts_with('x'));
        assert_eq!(AsRef::<str>::as_ref(&e), "x is not there");
        let back: String = e.clone().into();
        assert_eq!(back, e.into_string());
        assert_eq!(Error::from(&back), Error::new(back.as_str()));
        let engine = edm_core::Error::Changed;
        assert_eq!(Error::from(edm_core::Error::Changed), engine.to_string());
        let boxed: Box<dyn std::error::Error> = Box::new(Error::from("why"));
        assert_eq!(boxed.to_string(), "why");
    }
}
