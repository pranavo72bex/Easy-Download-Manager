// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// While a script acts, a folder outside the download folder is refused, `..` included; a
/// folder below it, or any folder with no script acting, is not (gaps row 199).
#[test]
fn a_scripts_folder_is_held_to_the_download_folder() {
    let downloads = std::path::Path::new("/home/me/Downloads");
    let away = std::path::Path::new("/home/me/.config/autostart");
    assert!(folder_refused_while(true, away, downloads).is_some());
    assert!(
        folder_refused_while(true, &downloads.join("../.config/autostart"), downloads).is_some()
    );
    assert!(folder_refused_while(true, &downloads.join("films"), downloads).is_none());
    assert!(folder_refused_while(false, away, downloads).is_none());
    // One folder written through a link and without it is one folder.
    let dir = tempfile::tempdir().unwrap();
    let real_downloads = dir.path().join("downloads");
    std::fs::create_dir_all(&real_downloads).unwrap();
    #[cfg(unix)]
    {
        let linked = dir.path().join("linked");
        std::os::unix::fs::symlink(dir.path(), &linked).unwrap();
        let through = linked.join("downloads").join("Videos");
        assert!(folder_refused_while(true, &through, &real_downloads).is_none());
    }
}

/// `url` with a login in it, set through its parts rather than written
/// out: a login spelled in the source reads to a secret scanner as a leak.
fn with_login(url: &str, user: &str, word: &str) -> String {
    let mut parsed = url::Url::parse(url).unwrap();
    parsed.set_username(user).unwrap();
    parsed.set_password(Some(word)).unwrap();
    parsed.to_string()
}

fn row(id: Id, url: &str, state: State) -> Arc<Entry> {
    Arc::new(
        serde_json::from_value(json!({
            "id": id,
            "url": url,
            "state": serde_json::to_value(&state).unwrap(),
            "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
            "filename": "f",
            "cookies": "sid=1",
        }))
        .unwrap(),
    )
}

/// A row's sign-in and request headers never leave over the control port,
/// and a password in its address is masked.
#[test]
fn the_list_says_whether_a_row_is_signed_in_and_never_what_with() {
    let word = format!("{}{}", "secret", "word");
    let listed = queue_list(&[row(
        1,
        &with_login("https://h.example/f", "u", &word),
        State::Paused,
    )]);
    let first = &listed[0];
    assert!(first.get("cookies").is_none() && first.get("headers").is_none());
    assert_eq!(first["signed_in"], json!(true));
    assert!(!first["url"].as_str().unwrap().contains(&word));
}

/// An add is checked before anything is added: a list, a folder inside the
/// download folder, both ends of a part; a listed address is answered back
/// once and not added again unless asked.
#[test]
fn an_add_is_checked_and_a_listed_address_is_not_added_twice() {
    let applied = Settings {
        download_dir: std::path::PathBuf::from("/tmp/dm-down"),
        ..Settings::default()
    };
    let rows = [row(1, "https://h.example/a", State::Paused)];
    assert!(add_asked(&rows, &applied, &json!({})).is_err());
    assert!(add_asked(&rows, &applied, &json!({"urls": ["  "]})).is_err());
    assert!(add_asked(
        &rows,
        &applied,
        &json!({"url": "https://h.example/b", "folder": "/etc"})
    )
    .is_err());
    assert!(add_asked(
        &rows,
        &applied,
        &json!({"url": "https://h.example/b", "from": "1:00"})
    )
    .is_err());
    let asked = add_asked(
        &rows,
        &applied,
        &json!({"urls": ["https://h.example/a", "https://h.example/a", "https://h.example/b"], "later": true}),
    )
    .unwrap();
    assert_eq!(asked.fresh, vec!["https://h.example/b".to_string()]);
    assert_eq!(asked.listed, vec!["https://h.example/a".to_string()]);
    assert!(asked.with.later && asked.with.queue == dm_core::MAIN_QUEUE);
    let again = add_asked(
        &rows,
        &applied,
        &json!({"url": "https://h.example/a", "again": true}),
    )
    .unwrap();
    assert_eq!(again.fresh.len(), 1);
}

/// A new limit keeps the one it replaces for the menu to turn back on.
#[test]
fn a_speed_limit_is_checked_and_keeps_the_one_it_replaces() {
    let applied = Settings {
        speed_limit: 300,
        ..Settings::default()
    };
    assert!(speed_limit(&applied, &json!({})).is_err());
    assert!(speed_limit(&applied, &json!({"kb": 100_000_001u64})).is_err());
    let (wanted, replaced) = speed_limit(&applied, &json!({"kb": 50})).unwrap();
    assert_eq!(
        (wanted.speed_limit, wanted.last_speed_limit, replaced),
        (50, 300, Some(300))
    );
}

#[test]
fn who_may_ask_is_decided_afresh_each_request() {
    assert_eq!(refused(false, false, false), None, "a script, port open");
    assert_eq!(refused(true, false, true), None, "an assistant, allowed");
    assert!(refused(false, true, true)
        .unwrap()
        .contains("for AI assistants only"));
    assert!(refused(true, false, false)
        .unwrap()
        .contains("AI assistants are turned off"));
    assert!(refused(true, true, false)
        .unwrap()
        .contains("AI assistants are turned off"));
}

#[test]
fn opening_a_file_is_never_done_over_the_port() {
    for id in PERSON_ONLY {
        assert!(person_only(id)
            .unwrap()
            .contains("done in the window by a person"));
    }
    assert_eq!(person_only("downloads.pause_all"), None);
}

#[test]
fn a_batch_answers_each_step_in_order_and_refuses_what_cannot_be_batched() {
    let mut asked = Vec::new();
    let params = json!({"steps": [
        {"method": "queue.list"},
        {"method": "batch", "params": {}},
        {"method": "ui.screenshot"},
        {"method": "nope", "params": {"x": 1}},
    ]});
    let answered = batch(&params, |method, params| {
        asked.push((method.to_string(), params.clone()));
        if method == "nope" {
            Err("no method is called nope".into())
        } else {
            Ok(json!(method))
        }
    })
    .unwrap();
    assert_eq!(
        asked,
        vec![
            ("queue.list".to_string(), Value::Null),
            ("nope".to_string(), json!({"x": 1}))
        ]
    );
    assert_eq!(answered[0], json!({"ok": true, "result": "queue.list"}));
    assert_eq!(answered[1]["error"], "batch cannot be batched");
    assert_eq!(answered[2]["error"], "ui.screenshot cannot be batched");
    assert_eq!(
        answered[3],
        json!({"ok": false, "error": "no method is called nope"})
    );

    assert!(batch(&json!({}), |_, _| Ok(Value::Null)).is_err());
    let many = json!({"steps": vec![json!({"method": "queue.list"}); BATCH_MOST + 1]});
    assert_eq!(
        batch(&many, |_, _| Ok(Value::Null)).unwrap_err(),
        "a batch is at most 256 steps"
    );
}

#[test]
fn a_screenshot_is_saved_under_a_safe_name() {
    let home = std::path::Path::new("/h");
    assert_eq!(
        screenshot_path(home, &json!({"name": "list"})),
        home.join("screenshots").join("list.png")
    );
    assert_eq!(
        screenshot_path(home, &Value::Null),
        home.join("screenshots").join("dm.png")
    );
    let odd = screenshot_path(home, &json!({"name": "../../etc/x"}));
    assert_eq!(odd.parent(), Some(home.join("screenshots").as_path()));
}

#[test]
fn only_the_queues_own_methods_are_answered_here() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let state = tempfile::tempdir().unwrap();
    let backend = crate::app::backend::testing::backend(&runtime, state.path());
    assert_eq!(
        answer(&backend, &[], "queue.list", &Value::Null),
        Some(Ok(json!([])))
    );
    assert!(answer(&backend, &[], "downloads.pause", &json!({"ids": []})).is_some());
    assert_eq!(answer(&backend, &[], "ui.inspect", &Value::Null), None);
    assert_eq!(answer(&backend, &[], "command.run", &Value::Null), None);
}

#[test]
fn a_file_is_not_run_for_a_moment_after_a_script_acted() {
    let mut guard = ScriptGuard::default();
    assert!(
        !guard.by_script() && guard.run_refused().is_none(),
        "no script yet"
    );
    guard.acted();
    assert!(guard.by_script());
    assert_eq!(
        guard.run_refused(),
        Some("Opening a file is done by a person, not by a script")
    );
    let long_ago = ScriptGuard(std::time::Instant::now().checked_sub(SCRIPT_ACTS_FOR));
    assert!(!long_ago.by_script(), "a moment is {SCRIPT_ACTS_FOR:?}");
}
