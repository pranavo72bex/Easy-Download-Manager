// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use crate::settings::SiteRule;
use crate::test_secrets::secret;

/// Settings with something on every part a backup carries: a sign-in, the proxy's password, a
/// queue's schedule and limit, a site rule, a saved search, and a program to run.
fn chosen() -> Settings {
    let mut settings = Settings::default();
    settings.site_logins.push(SiteLogin {
        host: "files.example.com".into(),
        username: "me".into(),
        password: secret("backup_login"),
    });
    settings.advanced.proxy.host = "proxy.example.com".into();
    settings.advanced.proxy.port = 3128;
    settings.advanced.proxy.username = "me".into();
    settings.advanced.proxy.password = secret("backup_proxy");
    settings.advanced.site_rules.push(SiteRule {
        host: "slow.example.com".into(),
        speed: 64,
        ..SiteRule::default()
    });
    settings.queue_schedules.insert(
        "Night".into(),
        edm_core::Schedule {
            enabled: true,
            from: "01:30".into(),
            to: "05:00".into(),
            ..edm_core::Schedule::default()
        },
    );
    settings
        .downloads
        .queue_limits
        .push(crate::settings::QueueLimit {
            queue: "Night".into(),
            files: 2,
        });
    settings.site_searches.push(crate::lists::Search {
        name: "Night files".into(),
        page: "https://example.com/files/".into(),
        ..crate::lists::Search::default()
    });
    settings.downloads.connections = 4;
    settings.downloads.on_complete = "their-program".into();
    settings
}

/// A row as the queue keeps it.
fn row(id: Id, url: &str, state: State) -> Entry {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "url": url,
        "state": serde_json::to_value(state).unwrap(),
        "progress": {"downloaded": 1, "total": 1, "speed": 0.0, "eta": null},
        "filename": format!("{id}.bin"),
        "queue": "Night",
    }))
    .unwrap()
}

/// Two rows: a finished one, and one with a login in its address and the browser's cookies
/// and headers.
fn rows() -> Vec<Entry> {
    let mut signed = row(
        2,
        &format!(
            "https://me:{}@files.example.com/b.zip",
            secret("backup_row")
        ),
        State::Paused,
    );
    signed.cookies = format!("session={}", secret("backup_cookie"));
    signed.headers = vec![("Authorization".into(), secret("backup_token"))];
    vec![
        row(1, "https://example.com/a.zip", State::Completed),
        signed,
    ]
}

/// Every secret the settings and rows above hold.
fn secrets() -> Vec<String> {
    [
        "backup_login",
        "backup_proxy",
        "backup_row",
        "backup_cookie",
        "backup_token",
    ]
    .into_iter()
    .map(secret)
    .collect()
}

/// Backed up with a passphrase and restored: the settings, queues, schedules, site rules and
/// searches come back, and the sign-ins with the passphrase alone; none of them is in the file
/// in clear; the program to run is this computer's.
#[test]
fn a_backup_comes_back_whole_and_its_sign_ins_with_the_passphrase_alone() {
    let passphrase = secret("backup_passphrase");
    let text = write(&chosen(), &rows(), Some(&passphrase)).unwrap();
    for secret in secrets() {
        assert!(!text.contains(&secret), "{secret} in the file in clear");
    }
    assert_eq!(edm_core::saved::format_of(text.as_bytes()), BACKUP_FORMAT);

    let mut backup = read(&text).unwrap();
    assert!(backup.sealed() && !backup.opened());
    assert_eq!((backup.rows(), backup.unread_rows), (2, 0));
    // A wrong passphrase: said, and nothing of the sign-ins taken.
    assert_eq!(backup.open("not it"), Err(Refused::WrongPassphrase));
    assert!(!backup.opened());
    let (without, _, _) = backup.settings_over(&Settings::default()).unwrap();
    assert!(without.site_logins[0].password.is_empty());

    backup.open(&passphrase).unwrap();
    assert!(backup.opened());
    let mine = Settings::default();
    let (wanted, kept, said) = backup.settings_over(&mine).unwrap();
    let was = chosen();
    assert!(said.is_empty(), "{said:?}");
    assert_eq!(wanted.site_logins, was.site_logins);
    assert_eq!(wanted.advanced.proxy.password, was.advanced.proxy.password);
    assert_eq!(wanted.advanced.site_rules, was.advanced.site_rules);
    assert_eq!(wanted.site_searches, was.site_searches);
    assert_eq!(wanted.downloads.queue_limits, was.downloads.queue_limits);
    assert_eq!(
        wanted.queue_schedules.get("Night").map(|s| s.from.as_str()),
        Some("01:30")
    );
    assert_eq!(wanted.downloads.connections, 4);
    assert!(kept.command, "a program to run is never set from a file");
    assert_eq!(wanted.downloads.on_complete, mine.downloads.on_complete);

    let (back, already) = backup.rows_for(&[]);
    assert_eq!(already, 0);
    let was = rows();
    assert_eq!(back.len(), 2);
    assert_eq!(back[0].url, was[0].url);
    assert_eq!(
        (&back[1].url, &back[1].cookies, &back[1].headers),
        (&was[1].url, &was[1].cookies, &was[1].headers)
    );
    assert_eq!(back[1].queue, "Night");
}

/// Without a passphrase the sign-ins are left out: the rows come back without them, and a
/// password this computer has for the same site and name is kept.
#[test]
fn without_a_passphrase_the_sign_ins_stay_behind() {
    let text = write(&chosen(), &rows(), None).unwrap();
    for secret in secrets() {
        assert!(!text.contains(&secret), "{secret} in the file");
    }
    let mut backup = read(&text).unwrap();
    assert!(!backup.sealed());
    assert_eq!(backup.open("anything"), Ok(()), "nothing to open");
    let (wanted, _, _) = backup.settings_over(&Settings::default()).unwrap();
    assert!(wanted.site_logins[0].password.is_empty());
    assert!(wanted.advanced.proxy.password.is_empty());
    let (wanted, _, _) = backup.settings_over(&chosen()).unwrap();
    assert_eq!(wanted.site_logins[0].password, secret("backup_login"));
    let (back, _) = backup.rows_for(&[]);
    assert_eq!(back[1].url, "https://files.example.com/b.zip");
    assert!(back[1].cookies.is_empty() && back[1].headers.is_empty());
}

/// A newer EDM's backup: a higher number, fields this EDM does not know, and a row it cannot
/// read, which is counted and left out; the rest is restored.
#[test]
fn a_newer_dms_backup_is_read_as_far_as_this_one_knows() {
    let text = write(&chosen(), &rows(), None).unwrap();
    let mut file: serde_json::Value = serde_json::from_str(&text).unwrap();
    file["format"] = 9.into();
    file["colour"] = "red".into();
    file["settings"]["a_newer_setting"] = serde_json::json!({"on": true});
    file["settings"]["connections"] = "many".into();
    file["rows"][0]["a_newer_field"] = 1.into();
    file["rows"]
        .as_array_mut()
        .unwrap()
        .insert(0, serde_json::json!({"id": "not a number"}));
    let backup = read(&file.to_string()).unwrap();
    assert_eq!(
        (backup.format, backup.rows(), backup.unread_rows),
        (9, 2, 1)
    );
    let (wanted, _, said) = backup.settings_over(&Settings::default()).unwrap();
    assert_eq!(wanted.advanced.site_rules, chosen().advanced.site_rules);
    assert_eq!(
        wanted.downloads.connections,
        Settings::default().clamped().downloads.connections,
        "a setting it cannot read keeps its default"
    );
    assert!(said.iter().any(|s| s.contains("connections")), "{said:?}");
    let (back, _) = backup.rows_for(&[]);
    assert_eq!(back[0].url, "https://example.com/a.zip");
}

/// What is not a backup is refused as one: no JSON, a settings file, a list.
#[test]
fn what_is_not_a_backup_is_refused() {
    let settings = serde_json::to_string(&Settings::default()).unwrap();
    for text in [
        "",
        "not json",
        "{}",
        "[]",
        settings.as_str(),
        "{\"kind\": \"EDM backup\"}",
    ] {
        assert!(matches!(read(text), Err(Refused::NotABackup)), "{text:.40}");
    }
}

/// Restored into a list that holds some of its rows already, those are not added again; a
/// second restore adds nothing; and what is added is held, touching no file.
#[test]
fn a_restore_adds_no_row_twice() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let backend = crate::app::backend::testing::backend(&runtime, dir.path());
    let text = write(&chosen(), &rows(), None).unwrap();
    let backup = read(&text).unwrap();
    let held = edm_core::AddOptions {
        later: true,
        ..edm_core::AddOptions::default()
    };
    backend
        .add_with("https://example.com/a.zip?utm_source=x", held)
        .unwrap();
    let first = backend.restore_rows(&backup);
    assert_eq!((first.added.len(), first.already), (1, 1));
    let again = backend.restore_rows(&backup);
    assert_eq!((again.added.len(), again.already), (0, 2));
    let listed = backend.list();
    assert_eq!(listed.len(), 2);
    let restored = listed.iter().find(|r| first.added.contains(&r.id)).unwrap();
    assert_eq!(restored.state, State::Paused);
    assert_eq!(restored.url, "https://files.example.com/b.zip");

    // What the list holds is what a backup of it holds.
    let made = read(&backend.back_up(None).unwrap()).unwrap();
    assert_eq!(made.rows(), 2);
}

/// A row marked "Keep no record" is named in no backup; this computer's history is left out.
#[test]
fn a_row_that_keeps_no_record_and_the_history_stay_out() {
    let mut settings = chosen();
    settings
        .recent_urls
        .push("https://example.com/typed-once.zip".into());
    let mut rows = rows();
    rows[0].keep_no_record = true;
    let text = write(&settings, &rows, None).unwrap();
    assert!(
        !text.contains("a.zip"),
        "the row that keeps no record is in the file"
    );
    assert!(!text.contains("typed-once"), "the history is in the file");
    let backup = read(&text).unwrap();
    assert_eq!(backup.rows(), 1);
    let (back, _) = backup.rows_for(&[]);
    assert_eq!(back[0].url, "https://files.example.com/b.zip");
}
