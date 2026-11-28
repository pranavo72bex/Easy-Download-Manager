// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
        file_name("EDM:queue-secrets-key").unwrap(),
        "EDM%3Aqueue-secrets-key.sealed"
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
    assert_eq!(store.get("EDM:key").unwrap(), None, "nothing there yet");
    store.delete("EDM:key").unwrap();
    store.put("EDM:key", &[9; 32]).unwrap();
    assert_eq!(store.get("EDM:key").unwrap(), Some(vec![9; 32]));
    store.put("EDM:key", b"second, longer").unwrap();
    assert_eq!(
        store.get("EDM:key").unwrap(),
        Some(b"second, longer".to_vec())
    );
    let file = home.path().join("secrets").join("EDM%3Akey.sealed");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(!text.contains("second"), "kept sealed, not in clear");
    assert!(!home.path().join("secrets/EDM%3Akey.sealed.new").exists());
    store.delete("EDM:key").unwrap();
    assert_eq!(store.get("EDM:key").unwrap(), None, "forgotten");
    store.delete("EDM:key").unwrap();
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
    // What edm-core keeps here: the 32-byte key that seals its file of secrets.
    let home = tempfile::tempdir().unwrap();
    let store = FileStore::new(home.path(), Toy);
    let plain = b"logins and cookies".to_vec();
    let sealed =
        edm_core::secretstore::seal_with(&store, edm_core::secretstore::QUEUE_KEY, &plain).unwrap();
    assert_eq!(
        edm_core::secretstore::open_with(&store, &sealed),
        edm_core::secretstore::Opened::Opened(plain)
    );
    assert!(home
        .path()
        .join("secrets")
        .join(file_name(edm_core::secretstore::QUEUE_KEY).unwrap())
        .exists());
}
