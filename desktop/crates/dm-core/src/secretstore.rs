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
mod tools {
    use std::io::Write;
    use std::process::{Command, Output, Stdio};

    use base64::Engine as _;

    use super::SecretStore;

    pub struct ToolStore {
        /// macOS: the keychain file every command names, where it is not the account's default
        /// (a test's own). Unused on Linux, where the session's Secret Service is the store.
        keychain: Option<String>,
    }

    impl ToolStore {
        /// The account's own store.
        pub const SYSTEM: ToolStore = ToolStore { keychain: None };
    }

    #[cfg(target_os = "macos")]
    const TOOL: &str = "security";
    #[cfg(target_os = "linux")]
    const TOOL: &str = "secret-tool";

    /// Whether the tool can be started at all (a lookup of a name nobody keeps).
    pub fn present() -> bool {
        run(&ToolStore::SYSTEM.lookup_args("dm:probe"), None).is_ok()
    }

    impl ToolStore {
        /// `args`, then the keychain named, if one is.
        #[cfg(target_os = "macos")]
        fn in_keychain(&self, args: &[&str]) -> Vec<String> {
            let mut all: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            all.extend(self.keychain.clone());
            all
        }

        #[cfg(target_os = "macos")]
        fn lookup_args(&self, name: &str) -> Vec<String> {
            self.in_keychain(&[
                "find-generic-password",
                "-a",
                "dm",
                "-s",
                &keyed(name),
                "-w",
            ])
        }
        #[cfg(target_os = "linux")]
        fn lookup_args(&self, name: &str) -> Vec<String> {
            let _ = &self.keychain;
            ["lookup", "service", "dm", "name", name]
                .map(String::from)
                .to_vec()
        }
    }

    /// A word `security -i` cannot mis-split: no space, line break or quote.
    #[cfg(target_os = "macos")]
    fn plain(word: &str) -> bool {
        !word
            .chars()
            .any(|c| c.is_whitespace() || "\"'\\".contains(c))
    }

    /// The name an item is kept under in the keychain: dm's own name where it is plain, and
    /// otherwise the name spelt in base64url. A Sign-in's name holds a line break between the
    /// site and the user (`settings/secrets.rs`), which `security -i` would split, so every
    /// Sign-in's password was refused and stayed in the settings file (security review,
    /// 2026-11-07). Plain names, the proxy's among them, are kept as they always were.
    #[cfg(target_os = "macos")]
    fn keyed(name: &str) -> std::borrow::Cow<'_, str> {
        if plain(name) {
            name.into()
        } else {
            format!(
                "dm:b64:{}",
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(name)
            )
            .into()
        }
    }

    /// Runs the tool, `stdin` fed to it, and gives what it printed and how it ended.
    fn run(args: &[String], stdin: Option<&[u8]>) -> Result<Output, String> {
        let mut command = Command::new(TOOL);
        command
            .args(args)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|e| format!("{TOOL}: {e}"))?;
        if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
            pipe.write_all(bytes).map_err(|e| format!("{TOOL}: {e}"))?;
        }
        child.wait_with_output().map_err(|e| format!("{TOOL}: {e}"))
    }

    fn said(output: &Output) -> String {
        let text = String::from_utf8_lossy(&output.stderr);
        let text = text.trim();
        if text.is_empty() {
            format!("{TOOL} ended with {}", output.status)
        } else {
            text.chars().take(200).collect()
        }
    }

    impl SecretStore for ToolStore {
        fn get(&self, name: &str) -> Result<Option<Vec<u8>>, String> {
            let out = run(&self.lookup_args(name), None)?;
            let text = String::from_utf8_lossy(&out.stdout);
            let text = text.trim();
            if !out.status.success() || text.is_empty() {
                // Both tools end unsuccessfully, saying nothing, for a name not kept; an
                // error message on stderr means a store that could not be used.
                return if out.stderr.iter().all(u8::is_ascii_whitespace)
                    || String::from_utf8_lossy(&out.stderr).contains("could not be found")
                {
                    Ok(None)
                } else {
                    Err(said(&out))
                };
            }
            base64::engine::general_purpose::STANDARD
                .decode(text)
                .map(Some)
                .map_err(|_| format!("{TOOL} gave back a value dm did not keep"))
        }

        fn put(&self, name: &str, value: &[u8]) -> Result<(), String> {
            let text = base64::engine::general_purpose::STANDARD.encode(value);
            // `secret-tool store` reads the secret from standard input. So does `security -i`
            // (its interactive mode reads commands from standard input), so the secret never
            // appears in a command line, where any program of the account could read it.
            #[cfg(target_os = "linux")]
            let out = run(
                &["store", "--label=dm", "service", "dm", "name", name].map(String::from),
                Some(text.as_bytes()),
            )?;
            #[cfg(target_os = "macos")]
            let out = {
                // The line is split on spaces and quotes by `security`; the name as kept
                // (`keyed`) and the base64 text have none, and a keychain path with any is
                // refused rather than mis-split.
                let name = keyed(name);
                if !self.keychain.as_deref().is_none_or(plain) {
                    return Err("a keychain path dm does not use".to_string());
                }
                let line = format!(
                    "add-generic-password -U -a dm -s {name} -w {text}{}\n",
                    self.keychain
                        .as_deref()
                        .map(|k| format!(" {k}"))
                        .unwrap_or_default()
                );
                run(&["-i".to_string()], Some(line.as_bytes()))?
            };
            if out.status.success() {
                Ok(())
            } else {
                Err(said(&out))
            }
        }

        fn delete(&self, name: &str) -> Result<(), String> {
            #[cfg(target_os = "linux")]
            let args = ["clear", "service", "dm", "name", name].map(String::from);
            #[cfg(target_os = "macos")]
            let args =
                self.in_keychain(&["delete-generic-password", "-a", "dm", "-s", &keyed(name)]);
            let out = run(&args, None)?;
            // Forgetting what is not there is not an error: the tools end unsuccessfully for it.
            let _ = out;
            Ok(())
        }
    }

    /// Against a real store, and only where asked (`DM_SECRET_STORE_TEST=1`): on Linux the
    /// session's Secret Service, which CI's machines have none of; on macOS a keychain file of the
    /// test's own, named on every command and left off the account's search list, so the
    /// person's login keychain is never read or written.
    #[cfg(test)]
    mod tests {
        use super::*;

        struct Scratch {
            store: ToolStore,
            #[cfg(target_os = "macos")]
            _dir: tempfile::TempDir,
        }

        #[cfg(target_os = "macos")]
        impl Drop for Scratch {
            fn drop(&mut self) {
                if let Some(keychain) = &self.store.keychain {
                    let _ = run(&["delete-keychain".into(), keychain.clone()], None);
                }
            }
        }

        fn scratch() -> Option<Scratch> {
            if std::env::var_os("DM_SECRET_STORE_TEST").is_none_or(|v| v.is_empty()) {
                eprintln!("not run: DM_SECRET_STORE_TEST is not set");
                return None;
            }
            #[cfg(target_os = "linux")]
            {
                Some(Scratch {
                    store: ToolStore::SYSTEM,
                })
            }
            #[cfg(target_os = "macos")]
            {
                let dir = tempfile::tempdir().unwrap();
                let keychain = dir.path().join("dm-test.keychain-db").display().to_string();
                let s = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>();
                // `create-keychain` puts the new one on the search list: the list is put
                // back as it was at once.
                let listed = run(&s(&["list-keychains", "-d", "user"]), None).unwrap();
                let before: Vec<String> = String::from_utf8_lossy(&listed.stdout)
                    .lines()
                    .map(|l| l.trim().trim_matches('"').to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
                let made = run(&s(&["create-keychain", "-p", "dm-test", &keychain]), None).unwrap();
                let mut back = s(&["list-keychains", "-d", "user", "-s"]);
                back.extend(before);
                let restored = run(&back, None).unwrap();
                assert!(made.status.success(), "{}", said(&made));
                assert!(restored.status.success(), "{}", said(&restored));
                let opened =
                    run(&s(&["unlock-keychain", "-p", "dm-test", &keychain]), None).unwrap();
                assert!(opened.status.success(), "{}", said(&opened));
                Some(Scratch {
                    store: ToolStore {
                        keychain: Some(keychain),
                    },
                    _dir: dir,
                })
            }
        }

        fn unique(what: &str) -> String {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            format!("dm:test:{what}:{}:{nanos}", std::process::id())
        }

        #[test]
        fn a_secret_is_kept_read_replaced_and_forgotten_in_the_real_store() {
            let Some(scratch) = scratch() else { return };
            let store = &scratch.store;
            let name = unique("round");
            assert_eq!(store.get(&name).unwrap(), None, "nothing there yet");
            store.put(&name, b"first").unwrap();
            assert_eq!(store.get(&name).unwrap().as_deref(), Some(&b"first"[..]));
            store.put(&name, b"second, longer").unwrap();
            assert_eq!(
                store.get(&name).unwrap().as_deref(),
                Some(&b"second, longer"[..])
            );
            store.delete(&name).unwrap();
            assert_eq!(store.get(&name).unwrap(), None, "forgotten");
            store.delete(&name).unwrap();
        }

        /// A Sign-in's name: the site and the user with a line break between, as
        /// `settings/secrets.rs` makes it, and a user name with a space and a quote.
        #[test]
        fn a_sign_ins_name_with_a_line_break_is_kept_and_forgotten_in_the_real_store() {
            let Some(scratch) = scratch() else { return };
            let store = &scratch.store;
            let name = format!("{}:example.com\nann o'neil", unique("login"));
            store.put(&name, b"pw").unwrap();
            assert_eq!(store.get(&name).unwrap().as_deref(), Some(&b"pw"[..]));
            store.delete(&name).unwrap();
            assert_eq!(store.get(&name).unwrap(), None, "forgotten");
        }

        #[cfg(target_os = "macos")]
        #[test]
        fn a_name_security_would_split_is_kept_under_a_plain_one_and_a_plain_one_as_it_is() {
            assert_eq!(keyed("dm:proxy:host:3128:me"), "dm:proxy:host:3128:me");
            let odd = keyed("dm:login:x:example.com\nann o'neil");
            assert!(odd.starts_with("dm:b64:") && plain(&odd), "{odd}");
            assert_ne!(odd, keyed("dm:login:x:example.com\nann o'neim"));
        }

        #[test]
        fn a_key_of_32_bytes_with_zeros_in_it_comes_back_whole() {
            let Some(scratch) = scratch() else { return };
            let store = &scratch.store;
            let name = unique("key");
            let key: Vec<u8> = (0..32u8).map(|n| if n % 5 == 0 { 0 } else { n }).collect();
            store.put(&name, &key).unwrap();
            assert_eq!(store.get(&name).unwrap(), Some(key));
            store.delete(&name).unwrap();
        }

        #[test]
        fn a_file_of_secrets_is_sealed_with_a_key_the_real_store_keeps() {
            let Some(scratch) = scratch() else { return };
            let store = &scratch.store;
            let key_name = unique("sealkey");
            let plain = br#"[{"id":1,"username":"me","password":"hunter2"}]"#;
            let sealed = super::super::seal_with(store, &key_name, plain).unwrap();
            assert!(!String::from_utf8_lossy(&sealed).contains("hunter2"));
            assert_eq!(
                super::super::open_with(store, &sealed),
                super::super::Opened::Opened(plain.to_vec())
            );
            store.delete(&key_name).unwrap();
            assert!(matches!(
                super::super::open_with(store, &sealed),
                super::super::Opened::Failed(_)
            ));
        }
    }
}

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
mod credentials {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };

    use super::SecretStore;

    /// `ERROR_NOT_FOUND`: there is no such credential.
    const NOT_FOUND: i32 = 1168;
    /// `CRED_MAX_CREDENTIAL_BLOB_SIZE`: the most one credential keeps.
    const MAX_BLOB: usize = 2560;

    /// The Windows Credential Manager, generic credentials of this account.
    pub struct CredentialStore;

    fn wide(text: &str) -> Vec<u16> {
        std::ffi::OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn last_error() -> i32 {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
    }

    impl SecretStore for CredentialStore {
        fn get(&self, name: &str) -> Result<Option<Vec<u8>>, String> {
            let target = wide(name);
            let mut found: *mut CREDENTIALW = std::ptr::null_mut();
            // SAFETY: `target` is a NUL-terminated UTF-16 string that lives past the call, and
            // `found` is a valid place for the call to put the pointer it allocates.
            let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut found) };
            if ok == 0 {
                let code = last_error();
                return if code == NOT_FOUND {
                    Ok(None)
                } else {
                    Err(format!(
                        "the Credential Manager could not read it (error {code})"
                    ))
                };
            }
            // SAFETY: the call succeeded, so `found` points to a credential that stays valid
            // until `CredFree`; its blob is `CredentialBlobSize` bytes, copied here before the
            // credential is freed, and `found` is not used after that.
            let value = unsafe {
                let credential = &*found;
                let bytes =
                    if credential.CredentialBlob.is_null() || credential.CredentialBlobSize == 0 {
                        Vec::new()
                    } else {
                        std::slice::from_raw_parts(
                            credential.CredentialBlob,
                            credential.CredentialBlobSize as usize,
                        )
                        .to_vec()
                    };
                CredFree(found.cast());
                bytes
            };
            Ok(Some(value))
        }

        fn put(&self, name: &str, value: &[u8]) -> Result<(), String> {
            if value.len() > MAX_BLOB {
                return Err(format!(
                    "{} bytes are more than the Credential Manager keeps in one credential ({MAX_BLOB})",
                    value.len()
                ));
            }
            let mut target = wide(name);
            let mut user = wide("dm");
            // SAFETY: `CREDENTIALW` is plain data (integers and pointers); all zero is a valid,
            // empty credential, and the fields the call reads are set below.
            let mut credential: CREDENTIALW = unsafe { std::mem::zeroed() };
            credential.Type = CRED_TYPE_GENERIC;
            credential.TargetName = target.as_mut_ptr();
            credential.UserName = user.as_mut_ptr();
            credential.CredentialBlobSize = value.len() as u32;
            credential.CredentialBlob = value.as_ptr() as *mut u8;
            // Kept for this account on this computer across restarts, and not carried to others
            // by a roaming profile: a secret of this computer's own.
            credential.Persist = CRED_PERSIST_LOCAL_MACHINE;
            // SAFETY: `credential` and what its pointers name (`target`, `user`, `value`) live
            // past the call, which only reads them and copies the blob into its own store.
            let ok = unsafe { CredWriteW(&credential, 0) };
            if ok == 0 {
                return Err(format!(
                    "the Credential Manager could not keep it (error {})",
                    last_error()
                ));
            }
            Ok(())
        }

        fn delete(&self, name: &str) -> Result<(), String> {
            let target = wide(name);
            // SAFETY: `target` is a NUL-terminated UTF-16 string that lives past the call.
            let ok = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
            if ok == 0 {
                let code = last_error();
                if code != NOT_FOUND {
                    return Err(format!(
                        "the Credential Manager could not forget it (error {code})"
                    ));
                }
            }
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A name of the test's own in the person's real Credential Manager,
        /// forgotten when the test ends, passed or failed: a failed assert
        /// before the test's own `delete` left `dm:test:key:…` there for good.
        struct Unique(String);

        impl Drop for Unique {
            fn drop(&mut self) {
                let _ = CredentialStore.delete(&self.0);
            }
        }

        impl std::ops::Deref for Unique {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0
            }
        }

        fn unique(what: &str) -> Unique {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            Unique(format!("dm:test:{what}:{}:{nanos}", std::process::id()))
        }

        #[test]
        fn a_secret_is_kept_read_replaced_and_forgotten_in_the_real_store() {
            let store = CredentialStore;
            let name = unique("round");
            assert_eq!(store.get(&name).unwrap(), None, "nothing there yet");
            store.put(&name, b"first").unwrap();
            assert_eq!(store.get(&name).unwrap().as_deref(), Some(&b"first"[..]));
            store.put(&name, b"second, longer").unwrap();
            assert_eq!(
                store.get(&name).unwrap().as_deref(),
                Some(&b"second, longer"[..])
            );
            store.delete(&name).unwrap();
            assert_eq!(store.get(&name).unwrap(), None, "forgotten");
            store.delete(&name).unwrap();
        }

        #[test]
        fn a_key_of_32_bytes_with_zeros_in_it_comes_back_whole() {
            let store = CredentialStore;
            let name = unique("key");
            let key: Vec<u8> = (0..32u8).map(|n| if n % 5 == 0 { 0 } else { n }).collect();
            store.put(&name, &key).unwrap();
            assert_eq!(store.get(&name).unwrap(), Some(key));
            store.delete(&name).unwrap();
        }

        #[test]
        fn a_secret_over_the_limit_is_refused_not_cut() {
            let store = CredentialStore;
            let name = unique("big");
            let error = store.put(&name, &vec![7u8; MAX_BLOB + 1]).unwrap_err();
            assert!(error.contains("more than"), "{error}");
            assert_eq!(store.get(&name).unwrap(), None);
        }

        #[test]
        fn a_file_of_secrets_is_sealed_with_a_key_the_real_store_keeps() {
            let store = CredentialStore;
            let key_name = unique("sealkey");
            let plain = br#"[{"id":1,"username":"me","password":"hunter2"}]"#;
            let sealed = super::super::seal_with(&store, &key_name, plain).unwrap();
            assert!(!String::from_utf8_lossy(&sealed).contains("hunter2"));
            assert_eq!(
                super::super::open_with(&store, &sealed),
                super::super::Opened::Opened(plain.to_vec())
            );
            store.delete(&key_name).unwrap();
            assert!(matches!(
                super::super::open_with(&store, &sealed),
                super::super::Opened::Failed(_)
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: &[u8] = br#"[{"id":3,"username":"me","password":"hunter2","cookies":"sid=abc"}]"#;

    fn tampered(sealed: &[u8], change: impl FnOnce(&mut Envelope)) -> Vec<u8> {
        let mut envelope: Envelope = serde_json::from_slice(sealed).unwrap();
        change(&mut envelope);
        serde_json::to_vec(&envelope).unwrap()
    }

    #[test]
    fn what_is_sealed_opens_again_and_shows_no_secret() {
        let store = MemoryStore::default();
        let sealed = seal_with(&store, QUEUE_KEY, PLAIN).unwrap();
        let text = String::from_utf8_lossy(&sealed).to_string();
        assert!(
            !text.contains("hunter2") && !text.contains("sid=abc"),
            "{text}"
        );
        assert!(is_sealed(&sealed));
        assert_eq!(open_with(&store, &sealed), Opened::Opened(PLAIN.to_vec()));
    }

    #[test]
    fn a_plain_file_is_said_to_be_plain() {
        let store = MemoryStore::default();
        assert!(!is_sealed(PLAIN));
        assert_eq!(open_with(&store, PLAIN), Opened::Plain);
        assert_eq!(open_with(&store, b"not json at all"), Opened::Plain);
    }

    #[test]
    fn a_file_larger_than_a_credential_seals_and_opens() {
        let store = MemoryStore::default();
        let big = vec![b'x'; 40_000];
        let sealed = seal_with(&store, QUEUE_KEY, &big).unwrap();
        assert_eq!(open_with(&store, &sealed), Opened::Opened(big));
    }

    #[test]
    fn the_key_is_made_once_and_kept_and_every_seal_has_its_own_nonce() {
        let store = MemoryStore::default();
        let first = seal_with(&store, QUEUE_KEY, PLAIN).unwrap();
        let key = store.get(QUEUE_KEY).unwrap().unwrap();
        assert_eq!(key.len(), 32);
        let second = seal_with(&store, QUEUE_KEY, PLAIN).unwrap();
        assert_eq!(store.get(QUEUE_KEY).unwrap().unwrap(), key, "the same key");
        assert_ne!(first, second, "another nonce, so other bytes");
    }

    #[test]
    fn a_changed_ciphertext_is_refused() {
        let store = MemoryStore::default();
        let sealed = seal_with(&store, QUEUE_KEY, PLAIN).unwrap();
        let b64 = base64::engine::general_purpose::STANDARD;
        let bad = tampered(&sealed, |e| {
            let mut bytes = b64.decode(&e.ct).unwrap();
            bytes[0] ^= 1;
            e.ct = b64.encode(bytes);
        });
        assert!(matches!(open_with(&store, &bad), Opened::Failed(_)));
    }

    #[test]
    fn a_changed_nonce_is_refused() {
        let store = MemoryStore::default();
        let sealed = seal_with(&store, QUEUE_KEY, PLAIN).unwrap();
        let b64 = base64::engine::general_purpose::STANDARD;
        let bad = tampered(&sealed, |e| e.nonce = b64.encode([0u8; 12]));
        assert!(matches!(open_with(&store, &bad), Opened::Failed(_)));
        let short = tampered(&sealed, |e| e.nonce = b64.encode([0u8; 5]));
        assert!(matches!(open_with(&store, &short), Opened::Failed(_)));
    }

    #[test]
    fn an_envelope_cannot_be_pointed_at_another_key() {
        let store = MemoryStore::default();
        let sealed = seal_with(&store, QUEUE_KEY, PLAIN).unwrap();
        // The same key bytes under another name: the name is part of what is authenticated.
        let key = store.get(QUEUE_KEY).unwrap().unwrap();
        store.put("dm:other", &key).unwrap();
        let bad = tampered(&sealed, |e| e.key = "dm:other".to_string());
        assert!(matches!(open_with(&store, &bad), Opened::Failed(_)));
    }

    #[test]
    fn a_missing_or_wrong_key_is_refused_with_a_reason() {
        let store = MemoryStore::default();
        let sealed = seal_with(&store, QUEUE_KEY, PLAIN).unwrap();
        store.delete(QUEUE_KEY).unwrap();
        match open_with(&store, &sealed) {
            Opened::Failed(why) => assert!(why.contains("not in this computer"), "{why}"),
            other => panic!("{other:?}"),
        }
        store.put(QUEUE_KEY, &[9u8; 32]).unwrap();
        assert!(matches!(open_with(&store, &sealed), Opened::Failed(_)));
        store.put(QUEUE_KEY, &[9u8; 7]).unwrap();
        assert!(matches!(open_with(&store, &sealed), Opened::Failed(_)));
    }

    #[test]
    fn an_edition_this_dm_does_not_know_is_said_not_guessed() {
        let store = MemoryStore::default();
        let sealed = seal_with(&store, QUEUE_KEY, PLAIN).unwrap();
        let newer = tampered(&sealed, |e| e.dm_secret_store = 99);
        match open_with(&store, &newer) {
            Opened::Failed(why) => assert!(why.contains("newer dm"), "{why}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn without_a_system_store_a_file_stays_as_it_was() {
        // `system()` is off under test and by the environment: the helpers pass bytes through.
        assert!(system().is_none());
        assert_eq!(sealed(PLAIN.to_vec()), PLAIN.to_vec());
        assert_eq!(opened(PLAIN), Opened::Plain);
        let store = MemoryStore::default();
        let sealed_bytes = seal_with(&store, QUEUE_KEY, PLAIN).unwrap();
        // Said of this system's own store.
        match opened(&sealed_bytes) {
            Opened::Failed(why) => {
                assert!(why.contains(STORE_NAME), "{why}");
                assert_eq!(why.contains("Windows"), cfg!(windows), "{why}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_namespace_is_short_and_stable_and_tells_homes_apart() {
        let a = namespace(Path::new(
            "C:\\Users\\one\\AppData\\Roaming\\dm\\settings.json",
        ));
        let b = namespace(Path::new(
            "C:\\Users\\two\\AppData\\Roaming\\dm\\settings.json",
        ));
        assert_eq!(a.len(), 8);
        assert_ne!(a, b);
        assert_eq!(
            a,
            namespace(Path::new(
                "C:\\Users\\one\\AppData\\Roaming\\dm\\settings.json"
            ))
        );
    }

    #[test]
    fn a_memory_store_keeps_replaces_and_forgets() {
        let store = MemoryStore::default();
        assert_eq!(store.get("a").unwrap(), None);
        store.put("a", b"1").unwrap();
        store.put("a", b"22").unwrap();
        assert_eq!(store.get("a").unwrap(), Some(b"22".to_vec()));
        store.delete("a").unwrap();
        store.delete("a").unwrap();
        assert_eq!(store.get("a").unwrap(), None);
    }
}
