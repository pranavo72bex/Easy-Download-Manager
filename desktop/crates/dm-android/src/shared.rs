// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What another app hands dm (gaps row N5), read from the activity's intent by
//! `android/intent.rs` and decided here, where the host's tests reach it: shared text, a
//! magnet or a web address opened with dm, or a `.torrent` file to copy into dm's folder first.

/// `Intent.ACTION_SEND`: text from the system's share sheet.
pub(crate) const SEND: &str = "android.intent.action.SEND";
/// `Intent.ACTION_VIEW`: an address opened with dm.
pub(crate) const VIEW: &str = "android.intent.action.VIEW";

/// What an intent handed over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Handed {
    /// Text whose links are added (`Engine::add` finds them in it).
    Text(String),
    /// A file's `content://` or `file://` address: a `.torrent`, copied into dm's folder and
    /// added as `file://`.
    File(String),
    /// Nothing for dm: the launcher's start, or an intent already taken.
    Nothing,
}

/// What an intent with `action`, `EXTRA_TEXT` `text` and data address `data` hands over.
pub(crate) fn handed(action: Option<&str>, text: Option<&str>, data: Option<&str>) -> Handed {
    match action {
        Some(SEND) => match text.map(str::trim) {
            Some(text) if !text.is_empty() => Handed::Text(text.to_owned()),
            _ => Handed::Nothing,
        },
        Some(VIEW) => {
            let Some(data) = data.map(str::trim).filter(|d| !d.is_empty()) else {
                return Handed::Nothing;
            };
            let scheme = data
                .split_once(':')
                .map(|(scheme, _)| scheme.to_ascii_lowercase())
                .unwrap_or_default();
            match scheme.as_str() {
                "content" | "file" => Handed::File(data.to_owned()),
                "magnet" | "http" | "https" | "ftp" => Handed::Text(data.to_owned()),
                _ => Handed::Nothing,
            }
        }
        _ => Handed::Nothing,
    }
}

/// The name a shared `.torrent` is kept under in dm's folder: the name the sending app gave
/// (`display`), else the address's last part, made safe for a file name and ending `.torrent`.
pub(crate) fn torrent_name(display: Option<&str>, last_part: Option<&str>) -> String {
    let given = display
        .or(last_part)
        .map(|name| name.rsplit(['/', ':', '\\']).next().unwrap_or(name))
        .unwrap_or_default();
    let safe: String = given
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " .-_()[]".contains(c) {
                c
            } else {
                '_'
            }
        })
        .take(120)
        .collect();
    let safe = safe.trim_matches(|c: char| c == '.' || c.is_whitespace());
    let stem = if safe.is_empty() { "shared" } else { safe };
    if stem.to_ascii_lowercase().ends_with(".torrent") {
        stem.to_owned()
    } else {
        format!("{stem}.torrent")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_text_and_opened_addresses_are_told_apart() {
        assert_eq!(
            handed(
                Some(SEND),
                Some("  A film https://example.com/a.mp4 "),
                None
            ),
            Handed::Text("A film https://example.com/a.mp4".into())
        );
        assert_eq!(handed(Some(SEND), Some("  "), None), Handed::Nothing);
        assert_eq!(
            handed(Some(VIEW), None, Some("magnet:?xt=urn:btih:abc")),
            Handed::Text("magnet:?xt=urn:btih:abc".into())
        );
        assert_eq!(
            handed(Some(VIEW), None, Some("content://downloads/1")),
            Handed::File("content://downloads/1".into())
        );
        assert_eq!(
            handed(Some(VIEW), None, Some("FILE:///sdcard/a.torrent")),
            Handed::File("FILE:///sdcard/a.torrent".into())
        );
        assert_eq!(handed(Some(VIEW), None, Some("tel:123")), Handed::Nothing);
        assert_eq!(
            handed(Some("android.intent.action.MAIN"), None, None),
            Handed::Nothing
        );
        assert_eq!(handed(None, None, None), Handed::Nothing);
    }

    #[test]
    fn a_torrent_is_kept_under_a_safe_name() {
        assert_eq!(
            torrent_name(Some("Big Buck Bunny.torrent"), None),
            "Big Buck Bunny.torrent"
        );
        assert_eq!(
            torrent_name(None, Some("primary:Download/linux.iso.TORRENT")),
            "linux.iso.TORRENT"
        );
        assert_eq!(torrent_name(Some("a/b\\c?d"), None), "c_d.torrent");
        assert_eq!(torrent_name(Some(" .. "), None), "shared.torrent");
        assert_eq!(torrent_name(None, None), "shared.torrent");
        assert!(torrent_name(Some(&"x".repeat(500)), None).len() <= 128);
    }
}
