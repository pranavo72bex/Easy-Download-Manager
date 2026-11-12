// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
            let bytes = if credential.CredentialBlob.is_null() || credential.CredentialBlobSize == 0
            {
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
