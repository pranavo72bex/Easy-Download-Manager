// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
        let opened = run(&s(&["unlock-keychain", "-p", "dm-test", &keychain]), None).unwrap();
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
