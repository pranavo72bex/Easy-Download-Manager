// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! EDM's store for secrets on a phone (gaps row N11): each secret in a file of its own under EDM's
//! home (`<home>/secrets/<name>.sealed`, readable by the app alone), sealed by a [`Sealer`]: on
//! the phone an AES-256-GCM key that never leaves the Android Keystore (`android/keystore.rs`).
//! The Keystore keeps keys, not values of a caller's choosing, so what it keeps is the key that
//! seals the file, and what the file holds is unreadable without the phone it was sealed on.
//!
//! What a file holds and where it is are plain code, tested on the computer with a stand-in
//! sealer; only the sealing itself is Android's.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use edm_core::secretstore::SecretStore;

/// The length of a GCM nonce the Keystore chooses: 12 bytes.
pub(crate) const IV_LEN: usize = 12;
/// The length of the tag GCM puts at the end of what it seals: 128 bits.
const TAG_LEN: usize = 16;

/// What seals a secret and opens it again. `name` is bound to the sealed bytes, so a file moved
/// to another name does not open.
pub(crate) trait Sealer: Send + Sync {
    /// `plain` sealed: the nonce chosen ([`IV_LEN`] bytes) and the sealed bytes with their tag.
    fn seal(&self, name: &str, plain: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String>;
    /// What [`Sealer::seal`] sealed with `iv`, opened; an error where it does not open.
    fn open(&self, name: &str, iv: &[u8], sealed: &[u8]) -> Result<Vec<u8>, String>;
}

/// Secrets kept in files under a folder, each sealed by `sealer`.
pub(crate) struct FileStore<S> {
    folder: PathBuf,
    sealer: S,
}

impl<S: Sealer> FileStore<S> {
    /// The store in `home`'s folder `secrets`, which is made at the first secret kept.
    pub(crate) fn new(home: &Path, sealer: S) -> Self {
        FileStore {
            folder: home.join("secrets"),
            sealer,
        }
    }

    fn path(&self, name: &str) -> Result<PathBuf, String> {
        Ok(self.folder.join(file_name(name)?))
    }
}

/// The file a secret's `name` is kept in: letters, digits, `-`, `_` and `.` as they are, every
/// other byte as `%` and two hex digits, so that two names never share a file and none reaches
/// outside the folder.
pub(crate) fn file_name(name: &str) -> Result<String, String> {
    if name.is_empty() {
        return Err("a secret needs a name".into());
    }
    let mut file = String::with_capacity(name.len() + 7);
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
            file.push(char::from(byte));
        } else {
            file.push_str(&format!("%{byte:02X}"));
        }
    }
    file.push_str(".sealed");
    Ok(file)
}

/// A sealed secret as its file's text: `iv || sealed`, in base64.
pub(crate) fn encode(iv: &[u8], sealed: &[u8]) -> Result<String, String> {
    if iv.len() != IV_LEN {
        return Err(format!(
            "the secret was sealed with a nonce of {} bytes, not {IV_LEN}",
            iv.len()
        ));
    }
    let mut bytes = Vec::with_capacity(iv.len() + sealed.len());
    bytes.extend_from_slice(iv);
    bytes.extend_from_slice(sealed);
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// A file's text as the nonce and the sealed bytes, or why it is not one EDM wrote.
pub(crate) fn decode(text: &str) -> Result<(Vec<u8>, Vec<u8>), String> {
    let unreadable = || "the sealed secret's file is not readable".to_string();
    let mut bytes = base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .map_err(|_| unreadable())?;
    if bytes.len() < IV_LEN + TAG_LEN {
        return Err(unreadable());
    }
    let sealed = bytes.split_off(IV_LEN);
    Ok((bytes, sealed))
}

/// Writes `text` to `path` readable by EDM alone, through a file beside it renamed over it, so a
/// secret is never half written.
fn write_private(folder: &Path, path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(folder)?;
    let mut staged = path.as_os_str().to_owned();
    staged.push(".new");
    let staged = PathBuf::from(staged);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let written = options.open(&staged).and_then(|mut file| {
        file.write_all(text.as_bytes())?;
        file.sync_all()
    });
    let kept = written.and_then(|()| std::fs::rename(&staged, path));
    if kept.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    kept
}

impl<S: Sealer> SecretStore for FileStore<S> {
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, String> {
        let path = self.path(name)?;
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("EDM could not read its sealed secret: {e}")),
        };
        let (iv, sealed) = decode(&text)?;
        self.sealer.open(name, &iv, &sealed).map(Some)
    }

    fn put(&self, name: &str, value: &[u8]) -> Result<(), String> {
        let path = self.path(name)?;
        let (iv, sealed) = self.sealer.seal(name, value)?;
        let text = encode(&iv, &sealed)?;
        write_private(&self.folder, &path, &text)
            .map_err(|e| format!("EDM could not keep its sealed secret: {e}"))
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        match std::fs::remove_file(self.path(name)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("EDM could not forget its sealed secret: {e}")),
        }
    }
}

#[cfg(test)]
mod tests;
