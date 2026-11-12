// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Where dm's secrets can be kept besides a file of its own: the operating system's store.
//!
//! On Windows that is the Credential Manager. A small secret (a password) goes there as it is. A
//! file of secrets (a row's login and its browser cookies, `secrets.json`) is larger than the
//! Credential Manager keeps (2560 bytes a credential), so the file stays where it is and is sealed
//! with a random key that the store holds: a copy of the whole home carries the file without the
//! key. On macOS (the Keychain, through `security`) and Linux (the Secret Service, through
//! `secret-tool`) the same is written, proved against a real store by the tests of `tools` below;
//! the installed app's own use is checked at the final QA (gaps row 39). Where
//! there is no store or it cannot be used, the file is written in clear as it always was, and the
//! queue says so once.
//!
//! `DM_NO_SECRET_STORE` in the environment switches the store off (tests and scripted runs that use
//! a throw-away home set it, so that they leave nothing in the person's own store).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use base64::Engine as _;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};

/// The name of the key that seals the queue's file of secrets.
pub const QUEUE_KEY: &str = "dm:queue-secrets-key";

/// The edition of the sealed file's envelope.
const ENVELOPE_VERSION: u32 = 1;

/// A place secrets can be kept by name. Errors are said in words: what a caller can do about one is
/// to keep the secret in its file as before and say so.
pub trait SecretStore: Send + Sync {
    /// The secret kept under `name`, or `None` where there is none.
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, String>;
    /// Keeps `value` under `name`, replacing what was there.
    fn put(&self, name: &str, value: &[u8]) -> Result<(), String>;
    /// Forgets `name`; forgetting what is not there is not an error.
    fn delete(&self, name: &str) -> Result<(), String>;
}

/// A store in memory: for tests, and for any caller that wants the sealing without a system store.
#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<String, Vec<u8>>>);

impl SecretStore for MemoryStore {
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(crate::lock(&self.0).get(name).cloned())
    }

    fn put(&self, name: &str, value: &[u8]) -> Result<(), String> {
        crate::lock(&self.0).insert(name.to_string(), value.to_vec());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        crate::lock(&self.0).remove(name);
        Ok(())
    }
}

/// The store this system keeps the key in, as a person knows it: the one a sealed file needs.
const STORE_NAME: &str = if cfg!(windows) {
    "the Windows Credential Manager"
} else if cfg!(target_os = "macos") {
    "the Keychain"
} else {
    "the Secret Service"
};

/// The operating system's store, where there is one and it is not switched off.
pub fn system() -> Option<&'static dyn SecretStore> {
    if cfg!(test) || std::env::var_os("DM_NO_SECRET_STORE").is_some() {
        return None;
    }
    #[cfg(windows)]
    {
        static STORE: credentials::CredentialStore = credentials::CredentialStore;
        Some(&STORE)
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        static STORE: tools::ToolStore = tools::ToolStore::SYSTEM;
        static PRESENT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        // Where the tool is not installed there is no store: the file stays as it was.
        PRESENT
            .get_or_init(tools::present)
            .then_some(&STORE as &dyn SecretStore)
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// The macOS Keychain (`security`) and the Linux Secret Service (`secret-tool`), reached through
/// their command-line tools, kept small on purpose. Proved against a real store on each system by
/// the tests below (`DM_SECRET_STORE_TEST=1`; on macOS a keychain of the test's own, never the
/// person's login keychain); the installed app's own use is checked at the final QA (gaps row
/// 39). A value is kept as base64
/// text, because both tools keep text. A tool that is absent means no store (`present`); one that
/// fails (a locked keychain, no Secret Service running) is an error, which the callers answer by
/// keeping the file as before and saying so.
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod tools;

/// A short name for the place `path` is in, so that two homes (a person's own and a throw-away one)
/// do not share the names of their secrets. Stable: it is part of what is looked up later.
pub fn namespace(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(path.to_string_lossy().as_bytes());
    crate::hex::encode(&digest[..4])
}

/// What an attempt to open a file of secrets found.
#[derive(Debug, PartialEq, Eq)]
pub enum Opened {
    /// Not sealed: the file as it always was.
    Plain,
    /// Sealed, and these are its bytes.
    Opened(Vec<u8>),
    /// Sealed, and could not be opened, for this reason.
    Failed(String),
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Envelope {
    dm_secret_store: u32,
    alg: String,
    key: String,
    nonce: String,
    ct: String,
}

fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|e| format!("no random numbers to seal with ({e})"))?;
    Ok(bytes)
}

/// The key kept under `name` in `store`, made and kept there if there is none yet.
fn key_in(store: &dyn SecretStore, name: &str) -> Result<[u8; 32], String> {
    match store.get(name)? {
        Some(kept) if kept.len() == 32 => {
            let mut key = [0u8; 32];
            key.copy_from_slice(&kept);
            Ok(key)
        }
        Some(kept) => Err(format!(
            "the key kept for the secrets is {} bytes, not 32",
            kept.len()
        )),
        None => {
            let key = random::<32>()?;
            store.put(name, &key)?;
            Ok(key)
        }
    }
}

/// `plain`, sealed under the key `key_name` of `store`: the bytes of a file that says what it is and
/// carries no secret in clear. The key's name is authenticated with the text, so an envelope cannot
/// be pointed at another key.
pub fn seal_with(store: &dyn SecretStore, key_name: &str, plain: &[u8]) -> Result<Vec<u8>, String> {
    let key = key_in(store, key_name)?;
    let nonce = random::<12>()?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    let sealed = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plain,
                aad: key_name.as_bytes(),
            },
        )
        .map_err(|_| "the secrets could not be sealed".to_string())?;
    let b64 = base64::engine::general_purpose::STANDARD;
    let envelope = Envelope {
        dm_secret_store: ENVELOPE_VERSION,
        alg: "chacha20poly1305".to_string(),
        key: key_name.to_string(),
        nonce: b64.encode(nonce),
        ct: b64.encode(sealed),
    };
    serde_json::to_vec_pretty(&envelope).map_err(|e| e.to_string())
}

/// Whether `bytes` are a sealed file (as opposed to a plain list of secrets).
pub fn is_sealed(bytes: &[u8]) -> bool {
    serde_json::from_slice::<Envelope>(bytes).is_ok()
}

/// What `bytes` hold: the file as it was, or the secrets opened with the key `store` keeps.
pub fn open_with(store: &dyn SecretStore, bytes: &[u8]) -> Opened {
    let Ok(envelope) = serde_json::from_slice::<Envelope>(bytes) else {
        return Opened::Plain;
    };
    if envelope.dm_secret_store != ENVELOPE_VERSION || envelope.alg != "chacha20poly1305" {
        return Opened::Failed(
            "the secrets were sealed by a newer dm, which this one cannot open".to_string(),
        );
    }
    let b64 = base64::engine::general_purpose::STANDARD;
    let (Ok(nonce), Ok(sealed)) = (b64.decode(&envelope.nonce), b64.decode(&envelope.ct)) else {
        return Opened::Failed("the sealed secrets are not readable".to_string());
    };
    if nonce.len() != 12 {
        return Opened::Failed("the sealed secrets are not readable".to_string());
    }
    let key = match store.get(&envelope.key) {
        Ok(Some(kept)) if kept.len() == 32 => kept,
        Ok(_) => {
            return Opened::Failed(
                "the key that seals the secrets is not in this computer's store".to_string(),
            )
        }
        Err(why) => return Opened::Failed(format!("the store could not be read ({why})")),
    };
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    match cipher.decrypt(
        Nonce::from_slice(&nonce),
        Payload {
            msg: &sealed,
            aad: envelope.key.as_bytes(),
        },
    ) {
        Ok(plain) => Opened::Opened(plain),
        Err(_) => Opened::Failed(
            "the sealed secrets do not match their key (changed, or sealed by another store)"
                .to_string(),
        ),
    }
}

/// A notice for the queue to say once: the store was there and could not be used.
static NOTE: Mutex<Option<String>> = Mutex::new(None);
static NOTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What the queue says about the store since the last time it asked, at most once in a run.
pub fn take_note() -> Option<String> {
    crate::lock(&NOTE).take()
}

fn note_once(why: String) {
    if !NOTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        *crate::lock(&NOTE) = Some(why);
    }
}

/// The bytes to write for a file of secrets: sealed under the system store's key where there is a
/// store that works, else `plain` as it is (and a notice, once, when the store was there and did not
/// work).
pub fn sealed(plain: Vec<u8>) -> Vec<u8> {
    let Some(store) = system() else {
        return plain;
    };
    match seal_with(store, QUEUE_KEY, &plain) {
        Ok(bytes) => bytes,
        Err(why) => {
            note_once(format!(
                "the operating system's store for secrets could not be used ({why}): the saved \
                 logins and cookies are kept in their file, as before"
            ));
            plain
        }
    }
}

/// What a file of secrets holds: as it is, or opened with the system store's key.
pub fn opened(bytes: &[u8]) -> Opened {
    match system() {
        Some(store) => open_with(store, bytes),
        // This system's store by name: macOS and Linux were told of Windows' (security
        // review, 2026-11-07, gaps row 159 f).
        None if is_sealed(bytes) => Opened::Failed(format!(
            "the secrets are sealed under a key in {STORE_NAME}, which is not available here"
        )),
        None => Opened::Plain,
    }
}

#[cfg(windows)]
mod credentials;

#[cfg(test)]
mod tests;
