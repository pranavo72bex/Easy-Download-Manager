// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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

/// A store that cannot be read or written, as a locked keychain is.
struct Broken;

impl SecretStore for Broken {
    fn get(&self, _: &str) -> Result<Option<Vec<u8>>, String> {
        Err("locked".to_string())
    }
    fn put(&self, _: &str, _: &[u8]) -> Result<(), String> {
        Err("locked".to_string())
    }
    fn delete(&self, _: &str) -> Result<(), String> {
        Err("locked".to_string())
    }
}

#[test]
fn a_store_that_cannot_be_used_seals_nothing_and_opens_nothing_and_says_why() {
    assert_eq!(
        seal_with(&Broken, QUEUE_KEY, PLAIN),
        Err("locked".to_string())
    );
    let sealed = seal_with(&MemoryStore::default(), QUEUE_KEY, PLAIN).unwrap();
    match open_with(&Broken, &sealed) {
        Opened::Failed(why) => assert_eq!(why, "the store could not be read (locked)"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_key_of_the_wrong_length_is_not_sealed_with() {
    let store = MemoryStore::default();
    store.put(QUEUE_KEY, &[1u8; 16]).unwrap();
    let why = seal_with(&store, QUEUE_KEY, PLAIN).unwrap_err();
    assert_eq!(why, "the key kept for the secrets is 16 bytes, not 32");
    // The short key is left as it was, not replaced by a new one.
    assert_eq!(store.get(QUEUE_KEY).unwrap(), Some(vec![1u8; 16]));
}

#[test]
fn an_envelope_with_text_that_is_not_base64_or_a_short_nonce_is_not_readable() {
    let store = MemoryStore::default();
    let sealed = seal_with(&store, QUEUE_KEY, PLAIN).unwrap();
    let not_base64 = tampered(&sealed, |e| e.ct = "not base64 at all!".to_string());
    let short_nonce = tampered(&sealed, |e| {
        e.nonce = base64::engine::general_purpose::STANDARD.encode([0u8; 8])
    });
    for bad in [not_base64, short_nonce] {
        assert!(is_sealed(&bad));
        assert_eq!(
            open_with(&store, &bad),
            Opened::Failed("the sealed secrets are not readable".to_string())
        );
    }
}
