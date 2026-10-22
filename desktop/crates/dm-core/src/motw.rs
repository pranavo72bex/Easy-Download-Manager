//! The Mark of the Web: on Windows, a downloaded file carries where it
//! came from in a `Zone.Identifier` stream beside it, as a browser's
//! downloads do, so SmartScreen and Office still treat it as from the
//! internet. A download manager that left it off would be a way round
//! those warnings.

use std::path::Path;

/// The stream's text: the internet zone, and the page and file addresses,
/// each without any sign-in written into it.
pub fn zone_text(url: &str, referrer: &str) -> String {
    let mut text = String::from("[ZoneTransfer]\r\nZoneId=3\r\n");
    if let Some(page) = plain(referrer) {
        text.push_str(&format!("ReferrerUrl={page}\r\n"));
    }
    if let Some(file) = plain(url) {
        text.push_str(&format!("HostUrl={file}\r\n"));
    }
    text
}

/// A web or FTP address without the user and password it may carry; none
/// for anything else (a magnet, a file on this computer).
fn plain(address: &str) -> Option<String> {
    let mut url = url::Url::parse(address.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https" | "ftp" | "ftps") {
        return None;
    }
    let _ = url.set_username("");
    let _ = url.set_password(None);
    let text = url.to_string();
    // A line break would start another entry in the stream.
    (!text.contains(['\r', '\n'])).then_some(text)
}

/// Marks a finished file as from the internet. Only on Windows, only a
/// file (a torrent's folder is left as it is), and a file system without
/// streams (FAT32) is said nothing about: the download is not at fault.
pub fn mark(path: &Path, url: &str, referrer: &str) {
    if !cfg!(windows) || !path.is_file() {
        return;
    }
    let mut stream = path.as_os_str().to_owned();
    stream.push(":Zone.Identifier");
    let _ = std::fs::write(stream, zone_text(url, referrer));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mark_names_the_internet_zone_and_no_sign_in() {
        let secret = crate::test_secrets::secret("motw");
        let mut signed = url::Url::parse("https://files.example/a.exe?x=1").unwrap();
        signed.set_username("me").unwrap();
        signed.set_password(Some(&secret)).unwrap();
        let text = zone_text(signed.as_str(), "https://files.example/page");
        assert!(text.starts_with("[ZoneTransfer]\r\nZoneId=3\r\n"));
        assert!(text.contains("HostUrl=https://files.example/a.exe?x=1\r\n"));
        assert!(text.contains("ReferrerUrl=https://files.example/page\r\n"));
        assert!(!text.contains(&secret) && !text.contains("me@"));
        let magnet = zone_text("magnet:?xt=urn:btih:abc", "");
        assert_eq!(magnet, "[ZoneTransfer]\r\nZoneId=3\r\n");
    }

    /// Run on CI's Windows machines: the stream is really written on NTFS.
    #[cfg(windows)]
    #[test]
    fn a_finished_file_on_windows_carries_the_mark() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("setup.exe");
        std::fs::write(&file, b"MZ").unwrap();
        mark(&file, "https://files.example/setup.exe", "");
        let mut stream = file.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        let text = std::fs::read_to_string(stream).unwrap();
        assert!(text.contains("ZoneId=3"), "{text}");
        assert_eq!(
            std::fs::read(&file).unwrap(),
            b"MZ",
            "the file itself changed"
        );
    }
}
