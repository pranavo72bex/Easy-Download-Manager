// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Every note a download may put on its row ([`super::Update::Note`]), each by its name, so a
//! place that sends or reads one names the sentence it means rather than a place in [`NOTES`].

/// Higher qualities passed over as copy-protected (gaps row Y19).
pub const PROTECTED_ABOVE: &str =
    "higher qualities are copy-protected, so the best one that is not was taken";

/// Pieces of a live stream the playlist moved past before dm fetched them, `{}` how many (gaps
/// row Y23).
pub const LIVE_PIECES_GONE: &str =
    "{} pieces of the live stream were gone before dm could fetch them";

/// The note of a file checked against a checksum file found beside it (gaps row F5).
pub const CHECKED_BESIDE: &str = "checked against the checksum file published beside it";

/// Parts of a stream in another format (an advert) left out, `{}` how many (gaps row Y18).
pub const PARTS_LEFT_OUT: &str =
    "{} parts of the stream in another format, such as an advert, were left out";

/// "Unpack when done" unpacked the archive and kept it (gaps row W14).
pub const UNPACKED: &str = "unpacked into a folder of its name";

/// "Unpack when done" unpacked the archive and sent it to the Trash (gaps row W14).
pub const UNPACKED_AND_TRASHED: &str =
    "unpacked into a folder of its name, and the archive sent to the Trash";

/// "Unpack when done" unpacked the archive, which could not be sent to the Trash (gaps row W14).
pub const UNPACKED_NOT_TRASHED: &str =
    "unpacked into a folder of its name, but the archive could not be sent to the Trash";

/// "Unpack when done" left an archive with a password packed (gaps row W14).
pub const NOT_UNPACKED_PASSWORD: &str = "not unpacked: the archive has a password";

/// "Unpack when done" left the archive packed for another reason (gaps row W14).
pub const NOT_UNPACKED_REFUSED: &str = "not unpacked: Extract here on its menu says why";

/// What "Unpack when done" made of a finished archive (dm-shell's `app::unpack`): the last five
/// of [`NOTES`], in its order.
pub const UNPACK_NOTES: [&str; 5] = [
    UNPACKED,
    UNPACKED_AND_TRASHED,
    UNPACKED_NOT_TRASHED,
    NOT_UNPACKED_PASSWORD,
    NOT_UNPACKED_REFUSED,
];

/// Every note a download may put on its row, so the window has each in every language; a note
/// with a number says it as `{}`. The last five are [`UNPACK_NOTES`].
pub const NOTES: [&str; 9] = [
    PROTECTED_ABOVE,
    LIVE_PIECES_GONE,
    CHECKED_BESIDE,
    PARTS_LEFT_OUT,
    UNPACKED,
    UNPACKED_AND_TRASHED,
    UNPACKED_NOT_TRASHED,
    NOT_UNPACKED_PASSWORD,
    NOT_UNPACKED_REFUSED,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The unpack notes are the last five, and no two notes say the same.
    #[test]
    fn the_notes_are_named_once_each_in_order() {
        assert_eq!(NOTES[4..], UNPACK_NOTES[..]);
        for (i, note) in NOTES.iter().enumerate() {
            assert!(!NOTES[i + 1..].contains(note), "{note}");
        }
    }
}
