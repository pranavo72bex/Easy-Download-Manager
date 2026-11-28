// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! EDM's one way to log (gaps row A19): what went wrong or what was chosen that no row or
//! banner says, written to standard error with its level first.
//!
//! Written as `note!(level, "words {}", values)`, the level one of `error`, `warn` or `info`,
//! the rest as `format!` takes it. edm-core re-exports it, so every crate above writes
//! `edm_core::note!` (edm-core itself `crate::note!`). A line reads `warn: EDM: …`: the level, a
//! colon, then the words as given. Nothing else: no file, no switch, no timestamp. What a
//! person is told in the window goes on a row or the banner (`Backend::note`), never here,
//! and what a command-line mode answers (`--help`, a typo) is printed by that mode itself.

use std::io::Write;

/// How much a note matters, written before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Something EDM could not do, and nothing took its place.
    Error,
    /// Something failed that EDM went on without.
    Warn,
    /// What EDM chose, for whoever looks into why.
    Info,
}

impl Level {
    /// The word a note at this level begins with.
    pub fn word(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
        }
    }
}

/// The line a note is written as: its level, a colon, then its words.
pub fn line(level: Level, words: std::fmt::Arguments<'_>) -> String {
    format!("{}: {words}", level.word())
}

/// Writes a note on standard error, where EDM's notes have always gone; what `note!` calls. A
/// standard error that cannot be written (a window program started with none) loses the note
/// rather than stopping EDM.
pub fn write(level: Level, words: std::fmt::Arguments<'_>) {
    let _ = writeln!(std::io::stderr().lock(), "{}", line(level, words));
}

/// Notes something on standard error with its level first: `note!(warn, "EDM: {x} failed")`.
///
/// The level is one of `error`, `warn` or `info` ([`log::Level`](crate::log::Level)); the
/// words and values after it are as `format!` takes them. The module `edm_net::log` says what
/// belongs here and what does not.
#[macro_export]
macro_rules! note {
    (error, $($words:tt)+) => {
        $crate::log::write($crate::log::Level::Error, ::std::format_args!($($words)+))
    };
    (warn, $($words:tt)+) => {
        $crate::log::write($crate::log::Level::Warn, ::std::format_args!($($words)+))
    };
    (info, $($words:tt)+) => {
        $crate::log::write($crate::log::Level::Info, ::std::format_args!($($words)+))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_its_level_then_its_words() {
        let path = "a.bin";
        assert_eq!(
            line(
                Level::Warn,
                format_args!("EDM: {path} was not synced to the disk: {}", 5)
            ),
            "warn: EDM: a.bin was not synced to the disk: 5"
        );
        assert_eq!(
            line(Level::Error, format_args!("edm-gpui: x")),
            "error: edm-gpui: x"
        );
        assert_eq!(line(Level::Info, format_args!("EDM: y")), "info: EDM: y");
    }

    #[test]
    fn every_level_is_written() {
        let n = 1;
        crate::note!(error, "EDM: a test's error note {n}");
        crate::note!(warn, "EDM: a test's warning note {}", n + 1);
        crate::note!(info, "EDM: a test's note");
    }
}
