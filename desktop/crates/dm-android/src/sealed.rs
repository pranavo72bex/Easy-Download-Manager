// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's store for secrets on a phone (gaps row N11): each secret in a file of its own under dm's
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
use dm_core::secretstore::SecretStore;

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

/// A file's text as the nonce and the sealed bytes, or why it is not one dm wrote.
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

/// Writes `text` to `path` readable by dm alone, through a file beside it renamed over it, so a
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
            Err(e) => return Err(format!("dm could not read its sealed secret: {e}")),
        };
        let (iv, sealed) = decode(&text)?;
        self.sealer.open(name, &iv, &sealed).map(Some)
    }

    fn put(&self, name: &str, value: &[u8]) -> Result<(), String> {
        let path = self.path(name)?;
        let (iv, sealed) = self.sealer.seal(name, value)?;
        let text = encode(&iv, &sealed)?;
        write_private(&self.folder, &path, &text)
            .map_err(|e| format!("dm could not keep its sealed secret: {e}"))
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        match std::fs::remove_file(self.path(name)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("dm could not forget its sealed secret: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for the Keystore: XOR with a byte and the name's length, a tag of the name's
    /// bytes; enough to see that what is kept is not the secret and that a name is bound.
    struct Toy;

    impl Sealer for Toy {
        fn seal(&self, name: &str, plain: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
            let mut sealed: Vec<u8> = plain.iter().map(|b| b ^ 0x5a).collect();
            sealed.extend(tag(name));
            Ok((vec![7; IV_LEN], sealed))
        }

        fn open(&self, name: &str, iv: &[u8], sealed: &[u8]) -> Result<Vec<u8>, String> {
            assert_eq!(iv, [7; IV_LEN]);
            let (body, got) = sealed.split_at(sealed.len() - TAG_LEN);
            if got != tag(name) {
                return Err("the tag does not match".into());
            }
            Ok(body.iter().map(|b| b ^ 0x5a).collect())
        }
    }

    fn tag(name: &str) -> [u8; TAG_LEN] {
        let mut tag = [0u8; TAG_LEN];
        for (i, b) in name.bytes().enumerate() {
            tag[i % TAG_LEN] ^= b;
        }
        tag
    }

    #[test]
    fn names_become_one_file_each_inside_the_folder() {
        assert_eq!(
            file_name("dm:queue-secrets-key").unwrap(),
            "dm%3Aqueue-secrets-key.sealed"
        );
        assert_eq!(file_name("../x").unwrap(), "..%2Fx.sealed");
        assert_eq!(file_name("a/b\\c").unwrap(), "a%2Fb%5Cc.sealed");
        assert_eq!(file_name("%3A").unwrap(), "%253A.sealed");
        assert_ne!(file_name(":").unwrap(), file_name("%3A").unwrap());
        assert_eq!(file_name("é").unwrap(), "%C3%A9.sealed");
        assert!(file_name("").is_err());
    }

    #[test]
    fn a_file_is_the_nonce_then_the_sealed_bytes_in_base64() {
        let iv = [1u8; IV_LEN];
        let sealed = [2u8; 40];
        let text = encode(&iv, &sealed).unwrap();
        let (got_iv, got_sealed) = decode(&text).unwrap();
        assert_eq!(got_iv, iv);
        assert_eq!(got_sealed, sealed);
        assert_eq!(decode(&format!("{text}\n")).unwrap().0, iv);
        assert!(encode(&[0; 16], &sealed).is_err());
        assert!(decode("not base64 !").is_err());
        assert!(decode(&encode(&iv, &[0; TAG_LEN - 1]).unwrap()).is_err());
    }

    #[test]
    fn get_put_delete_as_the_trait_says() {
        let home = tempfile::tempdir().unwrap();
        let store = FileStore::new(home.path(), Toy);
        assert_eq!(store.get("dm:key").unwrap(), None, "nothing there yet");
        store.delete("dm:key").unwrap();
        store.put("dm:key", &[9; 32]).unwrap();
        assert_eq!(store.get("dm:key").unwrap(), Some(vec![9; 32]));
        store.put("dm:key", b"second, longer").unwrap();
        assert_eq!(
            store.get("dm:key").unwrap(),
            Some(b"second, longer".to_vec())
        );
        let file = home.path().join("secrets").join("dm%3Akey.sealed");
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(!text.contains("second"), "kept sealed, not in clear");
        assert!(!home.path().join("secrets/dm%3Akey.sealed.new").exists());
        store.delete("dm:key").unwrap();
        assert_eq!(store.get("dm:key").unwrap(), None, "forgotten");
        store.delete("dm:key").unwrap();
    }

    #[test]
    fn a_file_moved_to_another_name_or_damaged_is_an_error() {
        let home = tempfile::tempdir().unwrap();
        let store = FileStore::new(home.path(), Toy);
        store.put("one", b"secret").unwrap();
        let folder = home.path().join("secrets");
        std::fs::copy(folder.join("one.sealed"), folder.join("other.sealed")).unwrap();
        assert!(store.get("other").is_err());
        std::fs::write(folder.join("one.sealed"), "garbage").unwrap();
        assert!(store.get("one").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_the_apps_alone() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        FileStore::new(home.path(), Toy).put("k", b"v").unwrap();
        let folder = home.path().join("secrets");
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&folder.join("k.sealed")), 0o600);
        assert_eq!(mode(&folder), 0o700);
    }

    #[test]
    fn the_queue_key_round_trips_through_the_seal() {
        // What dm-core keeps here: the 32-byte key that seals its file of secrets.
        let home = tempfile::tempdir().unwrap();
        let store = FileStore::new(home.path(), Toy);
        let plain = b"logins and cookies".to_vec();
        let sealed =
            dm_core::secretstore::seal_with(&store, dm_core::secretstore::QUEUE_KEY, &plain)
                .unwrap();
        assert_eq!(
            dm_core::secretstore::open_with(&store, &sealed),
            dm_core::secretstore::Opened::Opened(plain)
        );
        assert!(home
            .path()
            .join("secrets")
            .join(file_name(dm_core::secretstore::QUEUE_KEY).unwrap())
            .exists());
    }
}
