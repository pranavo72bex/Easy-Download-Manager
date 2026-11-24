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
    /// Text whose links are added (the window finds them in it, as in a dropped link).
    Text(String),
    /// A file's `content://` address from another app: a `.torrent`, copied into dm's folder
    /// and added as `file://`.
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
                // Never a file of dm's own: another app's `content://` alone (gaps row H16).
                "content" if !own_provider(data) => Handed::File(data.to_owned()),
                "magnet" | "http" | "https" | "ftp" => Handed::Text(data.to_owned()),
                // Not a `file://` address (gaps row H16): Android 7 and up refuse to hand one
                // between apps, so only an app reaching for a path in dm's own folders, to be
                // read with dm's rights, would send it.
                _ => Handed::Nothing,
            }
        }
        _ => Handed::Nothing,
    }
}

/// dm's own package, the start of any content authority of its own.
const OWN_PACKAGE: &str = "com.dm.android";

/// Whether `content` names a provider of dm's own (`content://com.dm.android…/`): what dm
/// keeps for itself is not read back as something another app shared.
fn own_provider(content: &str) -> bool {
    let rest = content
        .split_once("://")
        .map_or("", |(_, rest)| rest)
        .to_ascii_lowercase();
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    // An authority may carry a user part (`x@host`); the host is what names the provider.
    let host = authority.rsplit('@').next().unwrap_or("");
    host == OWN_PACKAGE || host.starts_with(&format!("{OWN_PACKAGE}."))
}

/// Keeps a shared `.torrent`'s `bytes` in `torrents` under `name`, or `name (1)` and on where
/// that is taken, never over a file already there: a row reads its `.torrent` again from its
/// path each time it starts, so another app sharing one of the same name had that row fetch,
/// and share, the other's torrent (gaps row H10, audit of the Android app).
pub(crate) fn keep_torrent(
    torrents: &std::path::Path,
    name: &str,
    bytes: &[u8],
) -> Result<std::path::PathBuf, String> {
    use std::io::Write;
    std::fs::create_dir_all(torrents)
        .map_err(|e| format!("dm could not make its folder for torrents: {e}"))?;
    // A name free now, claimed by creating it: another share between the look and the write
    // takes the next one rather than being written over.
    for _ in 0..16 {
        let path = torrents.join(dm_core::free_name(torrents, name));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                return file
                    .write_all(bytes)
                    .map(|()| path)
                    .map_err(|e| format!("dm could not keep the .torrent: {e}"));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("dm could not keep the .torrent: {e}")),
        }
    }
    Err("dm could not find a free name for the .torrent".into())
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
        // Not a `file://` address, nor a provider of dm's own (gaps row H16).
        for refused in [
            "FILE:///sdcard/a.torrent",
            "file:///data/data/com.dm.android/files/settings.json",
            "content://com.dm.android.files/home/settings.json",
            "content://COM.DM.ANDROID/x.torrent",
            "content://evil@com.dm.android.provider/x.torrent",
        ] {
            assert_eq!(
                handed(Some(VIEW), None, Some(refused)),
                Handed::Nothing,
                "{refused}"
            );
        }
        assert_eq!(
            handed(
                Some(VIEW),
                None,
                Some("content://com.dm.androidx.files/a.torrent")
            ),
            Handed::File("content://com.dm.androidx.files/a.torrent".into())
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

    #[test]
    fn a_shared_torrent_never_replaces_one_already_kept() {
        let dir = tempfile::tempdir().unwrap();
        let first = keep_torrent(dir.path(), "ubuntu.torrent", b"the person's").unwrap();
        let second = keep_torrent(dir.path(), "ubuntu.torrent", b"another app's").unwrap();
        assert_ne!(first, second);
        assert_eq!(std::fs::read(&first).unwrap(), b"the person's");
        assert_eq!(std::fs::read(&second).unwrap(), b"another app's");
        assert_eq!(second.file_name().unwrap(), "ubuntu (1).torrent");
    }
}
