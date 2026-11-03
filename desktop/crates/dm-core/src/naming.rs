//! Choosing a file name for a download.
//!
//! Reference A names a file from the page's title when the address or
//! Content-Disposition yields nothing meaningful. The order of preference:
//! 1. A `Content-Disposition` filename (already resolved elsewhere and
//!    passed in as `from_header`).
//! 2. The address's last path segment, unless `is_meaningless` says it
//!    carries no information.
//! 3. The page's title made safe, with the type's extension appended.
//! 4. The address's name as a fallback (always has something).

/// Whether a file name, as a stem (the part before the last `.`), carries
/// no information and the page title should be preferred.
///
/// True when:
/// - The stem is empty.
/// - The stem is one of the generic single-word names common in CDN URLs.
/// - The stem is all digits.
/// - The stem is hex-only or a UUID of 16 or more characters.
/// - The stem is 12 or more characters with no word separator, mixes
///   letters and digits, and 25–75 % of its letters are uppercase —
///   the fingerprint of a content hash masquerading as a filename.
pub fn is_meaningless(name: &str) -> bool {
    let stem = match name.rfind('.') {
        Some(pos) => &name[..pos],
        None => name,
    };
    if stem.is_empty() {
        return true;
    }
    const GENERIC: &[&str] = &[
        "video", "audio", "download", "file", "playback", "media", "index", "stream", "master",
        "manifest", "segment",
    ];
    if GENERIC.iter().any(|g| stem.eq_ignore_ascii_case(g)) {
        return true;
    }
    if stem.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    if stem.len() >= 16 {
        let lower = stem.to_ascii_lowercase();
        // UUID: 8-4-4-4-12 hex groups separated by hyphens.
        if is_uuid(&lower) {
            return true;
        }
        // All hex (no hyphens, no underscores).
        if lower.chars().all(|c| c.is_ascii_hexdigit()) {
            return true;
        }
    }
    if stem.len() >= 12 {
        let no_sep = !stem
            .chars()
            .any(|c| c == ' ' || c == '.' || c == '-' || c == '_');
        let has_letter = stem.chars().any(|c| c.is_ascii_alphabetic());
        let has_digit = stem.chars().any(|c| c.is_ascii_digit());
        if no_sep && has_letter && has_digit {
            let letters: Vec<char> = stem.chars().filter(|c| c.is_ascii_alphabetic()).collect();
            let uppers = letters.iter().filter(|c| c.is_uppercase()).count();
            let ratio = uppers as f64 / letters.len() as f64;
            if (0.25..=0.75).contains(&ratio) {
                return true;
            }
        }
    }
    false
}

fn is_uuid(s: &str) -> bool {
    // 8-4-4-4-12 hex groups separated by hyphens: total 36 characters.
    if s.len() != 36 {
        return false;
    }
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 5 {
        return false;
    }
    let expected = [8usize, 4, 4, 4, 12];
    parts
        .iter()
        .zip(expected.iter())
        .all(|(p, &len)| p.len() == len && p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Windows reserved names, case-insensitive, with any extension.
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Builds a safe file name from a page title and a file extension.
///
/// Returns `None` when the title is empty or consists only of characters that
/// make a safe name impossible.
pub fn title_name(title: &str, extension: &str) -> Option<String> {
    // Trim whitespace.
    let s = title.trim();
    // Replace `:` with ` -`.
    let s = s.replace(':', " -");
    // Remove control characters and the characters forbidden in Windows names.
    let forbidden: &[char] = &['\\', '/', '*', '?', '"', '<', '>', '|'];
    let s: String = s
        .chars()
        .filter(|c| !c.is_control() && !forbidden.contains(c))
        .collect();
    // Collapse runs of spaces.
    let mut out = String::with_capacity(s.len());
    let mut last_space = false;
    for c in s.chars() {
        if c == ' ' {
            if !last_space {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(c);
            last_space = false;
        }
    }
    // Strip trailing dots and spaces.
    let out = out.trim_end_matches(['.', ' ']).to_string();
    // Cut to 120 characters at a word boundary.
    let out = if out.len() > 120 {
        let cut = &out[..120];
        // Step back to the last space.
        match cut.rfind(' ') {
            Some(pos) => cut[..pos].trim_end_matches(['.', ' ']).to_string(),
            None => cut.to_string(),
        }
    } else {
        out
    };
    if out.is_empty() {
        return None;
    }
    // Refuse a Windows reserved name.
    let stem = match out.rfind('.') {
        Some(pos) => &out[..pos],
        None => out.as_str(),
    };
    if RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r)) {
        return None;
    }
    Some(format!("{out}.{extension}"))
}

/// Chooses a file name for a download, in order of reliability.
///
/// 1. `from_header`: a `Content-Disposition` filename (already extracted).
/// 2. The address's last path segment, unless `is_meaningless`.
/// 3. The page's `title`, sanitised and extended.
/// 4. The address's name, whatever it is.
pub fn choose(
    from_header: Option<&str>,
    from_url: &str,
    title: Option<&str>,
    extension: &str,
) -> String {
    // 1. Content-Disposition header name.
    if let Some(name) = from_header.filter(|s| !s.is_empty()) {
        return name.to_string();
    }
    // 2. URL name, if meaningful.
    let url_name = last_path_segment(from_url);
    if !url_name.is_empty() && !is_meaningless(url_name) {
        return url_name.to_string();
    }
    // 3. Page title.
    if let Some(t) = title.filter(|s| !s.is_empty()) {
        let ext = if extension.is_empty() {
            // Try to get the extension from the URL name.
            match url_name.rfind('.') {
                Some(pos) => &url_name[pos + 1..],
                None => extension,
            }
        } else {
            extension
        };
        if let Some(name) = title_name(t, ext) {
            return name;
        }
    }
    // 4. Fallback: URL name.
    if !url_name.is_empty() {
        return url_name.to_string();
    }
    format!("download.{extension}")
}

/// The last non-empty path segment of a URL, decoded minimally (no
/// full percent-decoding, which can introduce path separators).
fn last_path_segment(url: &str) -> &str {
    // Strip the query and fragment.
    let path = url
        .find('?')
        .or_else(|| url.find('#'))
        .map_or(url, |p| &url[..p]);
    path.rsplit('/').find(|s| !s.is_empty()).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- is_meaningless ----

    #[test]
    fn gifpaiwya_is_meaningless() {
        assert!(is_meaningless("GiFPAi8WYAEgiBu.mp4"));
    }

    #[test]
    fn long_base64ish_hash_is_meaningless() {
        assert!(is_meaningless("o4DPxfQfIIk3KMBHYxVIEGxCCEuyk9deAAzL6j.mp4"));
    }

    #[test]
    fn generic_video_is_meaningless() {
        assert!(is_meaningless("video.mp4"));
    }

    #[test]
    fn all_digits_is_meaningless() {
        assert!(is_meaningless("123456789.mp4"));
    }

    #[test]
    fn hex_hash_is_meaningless() {
        assert!(is_meaningless("3f2a9c1e7b8d4a6f9c0e1d2b3a4c5d6e.mp4"));
    }

    #[test]
    fn uuid_is_meaningless() {
        assert!(is_meaningless("0b6d3c1a-7e2f-4a8b-9c1d-2e3f4a5b6c7d.mp4"));
    }

    #[test]
    fn setup_installer_is_not_meaningless() {
        assert!(!is_meaningless("setup-installer.exe"));
    }

    #[test]
    fn big_buck_bunny_is_not_meaningless() {
        assert!(!is_meaningless("BigBuckBunny720p.mp4"));
    }

    #[test]
    fn report_2026_is_not_meaningless() {
        assert!(!is_meaningless("Report2026_Final.pdf"));
    }

    #[test]
    fn holiday_photos_is_not_meaningless() {
        assert!(!is_meaningless("holiday photos.zip"));
    }

    #[test]
    fn nasa_is_not_meaningless() {
        assert!(!is_meaningless("NASA.mp4"));
    }

    // ---- title_name ----

    #[test]
    fn title_with_colon_and_special_chars() {
        let name = title_name(r#"NASA on X: "Everybody, wave!" / Moon"#, "mp4").unwrap();
        assert_eq!(name, "NASA on X - Everybody, wave! Moon.mp4");
    }

    #[test]
    fn con_is_reserved() {
        assert!(title_name("CON", "mp4").is_none());
    }

    #[test]
    fn long_title_cut_at_word_boundary() {
        // Build a title that is slightly over 120 characters.
        let title = "word ".repeat(30); // 150 chars
        let name = title_name(title.trim(), "mp4").unwrap();
        assert!(name.len() <= 125, "too long: {}", name.len()); // 120 + ".mp4" = 124
    }

    #[test]
    fn empty_title_gives_none() {
        assert!(title_name("", "mp4").is_none());
        assert!(title_name("   ", "mp4").is_none());
    }

    // ---- choose ----

    #[test]
    fn header_name_beats_everything() {
        let name = choose(
            Some("report.pdf"),
            "https://cdn.example.com/3f2a9c1e7b8d4a6f.pdf",
            Some("My Report"),
            "pdf",
        );
        assert_eq!(name, "report.pdf");
    }

    #[test]
    fn hash_url_name_with_title_takes_the_title() {
        let name = choose(
            None,
            "https://x.com/GiFPAi8WYAEgiBu.mp4",
            Some("NASA on X: \"Everybody, wave!\""),
            "mp4",
        );
        assert!(name.starts_with("NASA on X"), "{name}");
    }

    #[test]
    fn real_url_name_beats_the_title() {
        let name = choose(
            None,
            "https://example.com/BigBuckBunny720p.mp4",
            Some("My video"),
            "mp4",
        );
        assert_eq!(name, "BigBuckBunny720p.mp4");
    }

    #[test]
    fn no_title_keeps_url_name() {
        let name = choose(
            None,
            "https://cdn.example.com/3f2a9c1e7b8d4a6f.mp4",
            None,
            "mp4",
        );
        assert_eq!(name, "3f2a9c1e7b8d4a6f.mp4");
    }
}
