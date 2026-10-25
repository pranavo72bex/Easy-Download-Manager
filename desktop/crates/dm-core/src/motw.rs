//! The Mark of the Web: on Windows, a downloaded file carries where it
//! came from in a `Zone.Identifier` stream beside it, as a browser's
//! downloads do, so SmartScreen and Office still treat it as from the
//! internet. On macOS the same is the `com.apple.quarantine` attribute,
//! which Gatekeeper reads. A download manager that left it off would be
//! a way round those warnings.

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

/// The attribute's value: downloaded (flag 0083), when in hex seconds, and
/// by whom.
pub fn quarantine_text(seconds: u64) -> String {
    format!("0083;{seconds:x};dm;")
}

/// Marks a finished file as from the internet. On Windows and macOS only,
/// only a file (a torrent's folder is left as it is), and a file system
/// without streams or attributes (FAT32) is said nothing about: the
/// download is not at fault.
pub fn mark(path: &Path, url: &str, referrer: &str) {
    if !path.is_file() {
        return;
    }
    #[cfg(windows)]
    {
        let mut stream = path.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        let _ = std::fs::write(stream, zone_text(url, referrer));
    }
    #[cfg(target_os = "macos")]
    {
        let _ = (url, referrer);
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let _ = set_quarantine(path, &quarantine_text(seconds));
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = (url, referrer);
}

#[cfg(target_os = "macos")]
fn set_quarantine(path: &Path, value: &str) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let name = c"com.apple.quarantine";
    // SAFETY: both names are NUL-terminated and the value's length is passed.
    let done = unsafe {
        libc::setxattr(
            path.as_ptr(),
            name.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
            0,
        )
    };
    if done == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
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

    /// Run on a Mac: the attribute is read back from the file.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_finished_file_on_macos_carries_the_quarantine_attribute() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("setup.dmg");
        std::fs::write(&file, b"x").unwrap();
        mark(&file, "https://files.example/setup.dmg", "");
        let out = std::process::Command::new("xattr")
            .args(["-p", "com.apple.quarantine"])
            .arg(&file)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.starts_with("0083;"), "{text}");
        assert_eq!(std::fs::read(&file).unwrap(), b"x");
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
