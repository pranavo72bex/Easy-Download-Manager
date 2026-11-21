// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use crate::register::{default_apps_keys, magnet_keys, reg_add, reg_delete};
use dm_protocol::NATIVE_HOST;
use std::path::Path;

fn words(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

#[test]
fn every_list_the_registration_writes_is_read_as_one_call() {
    let exe = Path::new(r"C:\Apps\dm\dm.exe");
    let lists = magnet_keys(exe)
        .into_iter()
        .chain(default_apps_keys(exe))
        .chain([reg_add(
            r"Software\Mozilla\NativeMessagingHosts",
            Path::new(r"C:\dm\firefox.json"),
        )]);
    for args in lists {
        let call = parse(&args).unwrap_or_else(|why| panic!("{args:?}: {why}"));
        assert!(matches!(call, Call::Set { .. }), "{args:?}: {call:?}");
    }
    assert_eq!(
        parse(&reg_add(
            r"Software\Google\Chrome\NativeMessagingHosts",
            Path::new(r"C:\dm\chromium.json")
        )),
        Ok(Call::Set {
            root: Root::CurrentUser,
            key: format!(r"Software\Google\Chrome\NativeMessagingHosts\{NATIVE_HOST}"),
            value: Value::Default,
            data: r"C:\dm\chromium.json".to_string(),
        })
    );
    assert_eq!(
        parse(&reg_delete(r"Software\Vivaldi\NativeMessagingHosts")),
        Ok(Call::DeleteKey {
            root: Root::CurrentUser,
            key: format!(r"Software\Vivaldi\NativeMessagingHosts\{NATIVE_HOST}"),
        })
    );
}

#[test]
fn a_value_set_to_nothing_and_a_named_value_keep_their_names() {
    let call = parse(&words(&[
        "add",
        r"HKCU\Software\Classes\magnet",
        "/v",
        "URL Protocol",
        "/t",
        "REG_SZ",
        "/d",
        "",
        "/f",
    ]));
    assert_eq!(
        call,
        Ok(Call::Set {
            root: Root::CurrentUser,
            key: r"Software\Classes\magnet".to_string(),
            value: Value::Named("URL Protocol".to_string()),
            data: String::new(),
        })
    );
    let made = parse(&words(&["add", r"HKCU\Software\dm"]));
    assert_eq!(
        made,
        Ok(Call::Make {
            root: Root::CurrentUser,
            key: r"Software\dm".to_string()
        })
    );
}

#[test]
fn deletes_and_queries_say_whether_a_key_or_a_value_is_meant() {
    let key = r"HKCU\Software\RegisteredApplications";
    let place = (
        Root::CurrentUser,
        r"Software\RegisteredApplications".to_string(),
    );
    assert_eq!(
        parse(&words(&["delete", key, "/v", "dm", "/f"])),
        Ok(Call::DeleteValue {
            root: place.0,
            key: place.1.clone(),
            value: Value::Named("dm".to_string()),
        })
    );
    assert_eq!(
        parse(&words(&["delete", key, "/f"])),
        Ok(Call::DeleteKey {
            root: place.0,
            key: place.1.clone(),
        })
    );
    assert_eq!(
        parse(&words(&["query", key, "/v", "dm"])),
        Ok(Call::Query {
            root: place.0,
            key: place.1.clone(),
            value: Some(Value::Named("dm".to_string())),
        })
    );
    assert_eq!(
        parse(&words(&["query", key])),
        Ok(Call::Query {
            root: place.0,
            key: place.1,
            value: None,
        })
    );
}

#[test]
fn the_kept_copies_stay_reg_exes_and_anything_unknown_is_refused() {
    let export = words(&["export", r"HKCU\Software\Classes\magnet", r"C:\x.reg", "/y"]);
    assert_eq!(parse(&export), Ok(Call::Program));
    assert_eq!(parse(&words(&["import", r"C:\x.reg"])), Ok(Call::Program));
    for refused in [
        words(&["copy", r"HKCU\a", r"HKCU\b"]),
        words(&["add", r"HKCR\.torrent", "/ve", "/d", "x"]),
        words(&["add", r"HKCU\a", "/v", "n", "/t", "REG_DWORD", "/d", "1"]),
        words(&["add", r"HKCU\a", "/v"]),
        words(&["add", r"HKCU\a", "/reg:32"]),
        words(&[]),
        words(&["delete", "HKCU", "/f"]),
        words(&["delete", r"HKCU\", "/f"]),
    ] {
        assert!(parse(&refused).is_err(), "{refused:?}");
    }
    assert_eq!(
        split_root(r"HKEY_LOCAL_MACHINE\SYSTEM\Rules\"),
        Ok((Root::LocalMachine, r"SYSTEM\Rules".to_string()))
    );
}

/// Run on Windows (CI's runners): a key of dm's tests' own, written, read and removed through
/// the API, as the registration does.
#[cfg(windows)]
#[test]
fn on_windows_a_key_is_written_read_and_removed_without_reg_exe() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let key = format!(r"HKCU\Software\dm-tests\{}-{stamp}", std::process::id());
    let add = |name: &str, data: &str| {
        run(&words(&[
            "add", &key, "/v", name, "/t", "REG_SZ", "/d", data, "/f",
        ]))
    };
    assert!(run(&words(&["query", &key])).is_err(), "not there yet");
    add("first", "v2.10|App=C:\\Apps\\dm.exe|").unwrap();
    add("second", &"x".repeat(10_000)).unwrap();
    run(&words(&[
        "add", &key, "/ve", "/t", "REG_SZ", "/d", "unnamed", "/f",
    ]))
    .unwrap();
    run(&words(&["query", &key, "/v", "first"])).unwrap();
    run(&words(&["query", &key, "/ve"])).unwrap();
    assert!(run(&words(&["query", &key, "/v", "third"])).is_err());
    let mut read = strings(&key).unwrap();
    read.sort();
    assert_eq!(
        read,
        [
            "unnamed".to_string(),
            "v2.10|App=C:\\Apps\\dm.exe|".to_string(),
            "x".repeat(10_000)
        ]
    );
    run(&words(&["delete", &key, "/v", "first", "/f"])).unwrap();
    assert!(run(&words(&["query", &key, "/v", "first"])).is_err());
    run(&words(&[
        "add",
        &format!(r"{key}\shell\open\command"),
        "/ve",
        "/d",
        "x",
    ]))
    .unwrap();
    run(&words(&["delete", &key, "/f"])).unwrap();
    assert!(
        run(&words(&["query", &key])).is_err(),
        "removed with what was under it"
    );
    let gone = run(&words(&["delete", &key, "/f"])).unwrap_err();
    assert!(gone.starts_with("reg delete: "), "{gone}");
    let _ = run(&words(&["delete", r"HKCU\Software\dm-tests", "/f"]));
}
