// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A finished file the system would run (gaps row H1): which ones, and the question Open asks
//! once before opening one, as a browser asks before it runs a program it downloaded.

use crate::i18n::{t, tf};

/// The endings of files the system runs, or installs, when they are opened: Windows' programs,
/// installers, screen savers, shortcuts and scripts; macOS's disk images, installers and
/// scripts; Android's apps; Java's archives.
const RUNS: [&str; 20] = [
    "exe", "msi", "scr", "lnk", "com", "bat", "cmd", "ps1", "vbs", "js", "jse", "wsf", "hta",
    "apk", "dmg", "pkg", "jar", "sh", "command", "app",
];

/// Whether the file named `name` would run when opened, by its ending, any letter case; and a
/// name whose ending is hidden behind a right-to-left mark (`invoice` U+202E `fdp.exe`) is read with
/// the marks taken out, as the system reads it.
pub fn runs(name: &str) -> bool {
    plain(name)
        .rsplit_once('.')
        .is_some_and(|(_, ending)| RUNS.contains(&ending.to_ascii_lowercase().as_str()))
}

/// `name` with the marks that change the direction of text taken out (right-to-left overrides,
/// embeddings and isolates): one hid a program's ending ("invoice", U+202E, "fdp.exe" reads as
/// "invoiceexe.pdf"), and the list shows a name as the system reads it (gaps row H1).
pub fn plain(name: &str) -> String {
    name.chars()
        .filter(|c| {
            !matches!(
                c,
                '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
            )
        })
        .collect()
}

/// What Open asks before it opens a program: its title, and its sentence naming the file.
pub fn open_question(name: &str) -> (&'static str, String) {
    (
        t("Open a program?"),
        tf(
            "{} is a program: it runs with everything you can do on this computer. Open it only if you trust the site it came from.",
            &[&name],
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A program, installer or script is told apart from a document, whatever its letters'
    /// case, and with a right-to-left mark hiding its ending (gaps row H1).
    #[test]
    fn a_file_that_would_run_is_told_apart() {
        for name in [
            "setup.exe",
            "Setup.EXE",
            "invoice.pdf.exe",
            "app.dmg",
            "dm.apk",
            "run.sh",
        ] {
            assert!(runs(name), "{name}");
        }
        for name in ["invoice.pdf", "movie.mp4", "archive.zip", "noending", "exe"] {
            assert!(!runs(name), "{name}");
        }
        assert!(runs("invoice\u{202e}fdp.exe"), "a mark hid the ending");
        assert_eq!(plain("invoice\u{202e}fdp.exe"), "invoicefdp.exe");
        assert_eq!(
            plain("שלום.pdf"),
            "שלום.pdf",
            "right-to-left letters themselves stay"
        );
    }
}
