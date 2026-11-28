// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Android's store for the key that seals the file of secrets (gaps row N11). A phone has no
//! keyring a library can reach and no program to run for one: its store is the Android
//! Keystore, which only Java's API reaches. So the app hands EDM a [`SecretStore`] of its own
//! ([`set_android_store`], from `edm-android`'s start, through JNI: its `sealed.rs` and
//! `android/keystore.rs`), which keeps each value in a file under EDM's home, sealed under an
//! AES key that never leaves the Keystore.
//!
//! Until the app has handed its store in, there is none ([`super::system`] answers `None`),
//! and the file of secrets is kept in clear in the app's own files folder, as on a desktop
//! without a store; no key is kept in clear in a file beside it, which would protect nothing
//! the folder does not already.
//!
//! Built on the desktop for this module's own tests alone.

use std::sync::OnceLock;

use super::SecretStore;

/// The app's store, once it has been handed in.
static STORE: OnceLock<Box<dyn SecretStore>> = OnceLock::new();

/// Keeps EDM's key in `store` from now on: the app's store over the Android Keystore. Handed
/// in once, before the engine starts; a second store is refused, since a key kept in the
/// first would not be found in it.
pub fn set_android_store(store: Box<dyn SecretStore>) -> Result<(), String> {
    STORE
        .set(store)
        .map_err(|_| "EDM's store for secrets was already given".to_string())
}

/// The app's store, where it has been handed in.
pub(super) fn store() -> Option<&'static dyn SecretStore> {
    STORE.get().map(|store| &**store)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Handed in once, the store is the one answered; a second is refused.
    #[test]
    fn the_store_handed_in_is_kept_and_a_second_is_refused() {
        set_android_store(Box::new(super::super::MemoryStore::default())).unwrap();
        let store = store().unwrap();
        store.put("k", b"v").unwrap();
        assert_eq!(store.get("k").unwrap(), Some(b"v".to_vec()));
        assert!(set_android_store(Box::new(super::super::MemoryStore::default())).is_err());
    }
}
