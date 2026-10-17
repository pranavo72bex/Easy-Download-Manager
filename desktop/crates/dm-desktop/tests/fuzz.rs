//! The window's readers of outside input — the control protocol, from the
//! socket to the handler, and the helpers that show addresses — fed random
//! and hostile input. `PROPTEST_CASES=20000` for a deep hunt.

use std::io::{BufRead, BufReader, Write};
use std::sync::Arc;

use dm_core::Queue;
use dm_desktop::app::DmApp;
use dm_desktop::backend::{Backend, Platform};
use dm_desktop::format;
use dm_shell::Settings;
use proptest::prelude::*;
use serde_json::{json, Value};

struct Nothing;

impl Platform for Nothing {
    fn set_login_item(&self, _: bool) -> Result<(), String> {
        Ok(())
    }
    fn notify(&self, _: &str, _: &str) {}
    fn clipboard(&self) -> Option<String> {
        None
    }
    fn reveal(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn open(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn choose_file(&self, _: bool, _: &str) -> Option<std::path::PathBuf> {
        None
    }
    fn choose_app(&self) -> Option<std::path::PathBuf> {
        None
    }
    fn chime(&self) {}
    fn power(
        &self,
        _: dm_desktop::power::WhenDone,
        _: dm_desktop::power::Report,
    ) -> Result<(), String> {
        Ok(())
    }
    fn open_with(&self, _: &str, _: &std::path::Path) -> Result<(), String> {
        Ok(())
    }
}

fn any_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|n| json!(n)),
        any::<String>().prop_map(Value::String),
        "(tasks|file|downloads|view|help)\\.[a-z_.]{0,20}".prop_map(Value::String),
        "(all|finished|unfinished|queue:.{0,6}|kind:[a-z]{0,10})".prop_map(Value::String),
    ];
    leaf.prop_recursive(2, 12, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::hash_map(
                prop_oneof![
                    Just("id".to_string()),
                    Just("ids".to_string()),
                    Just("place".to_string()),
                    Just("name".to_string()),
                    "[a-z]{0,6}",
                ],
                inner,
                0..4
            )
            .prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(
        std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(64)
    ))]

    #[test]
    fn any_control_request_is_answered_or_refused(
        method in prop_oneof![
            Just("commands.list".to_string()),
            Just("command.run".to_string()),
            Just("ui.inspect".to_string()),
            Just("ui.select".to_string()),
            Just("ui.place".to_string()),
            Just("queue.list".to_string()),
            any::<String>(),
        ],
        params in any_json(),
    ) {
        let dir = tempfile::tempdir().unwrap();
        // The window's settings file stays out of the real home.
        unsafe { std::env::set_var("DM_HOME", dir.path()) };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let settings = Settings {
            download_dir: dir.path().join("out"),
            ..Settings::default()
        };
        let queue = Queue::new(
            dm_core::client(),
            settings.options(dir.path().join("parts"), dm_core::Limiter::unlimited()),
            1,
        );
        let backend = Backend::new(runtime.handle().clone(), queue, settings, Vec::new(), Arc::new(Nothing));
        let mut app = DmApp::new(backend);
        let _ = app.control_call(&method, &params);
    }

    #[test]
    fn any_line_on_the_control_socket_gets_a_line_back_or_the_door(line in prop_oneof![
        any::<String>(),
        "\\{\"id\":[0-9]{0,3},\"method\":\"[a-z.]{0,12}\"(,\"params\":\\{[^}]{0,20}\\})?\\}".prop_map(|s| s),
    ]) {
        // One listener for every case, as dm has one: a listener per case
        // left an accept thread per case, and a deep run ran out of threads.
        let port = control_port();
        // A deep run opens connections faster than the system frees its
        // ports; a case the system will not connect is not one dm answered.
        let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) else {
            return Ok(());
        };
        stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        let line = line.replace('\n', " ");
        stream.write_all(format!("{line}\n").as_bytes()).unwrap();
        let mut reply = String::new();
        let _ = BufReader::new(stream).read_line(&mut reply);
        if !reply.is_empty() {
            let parsed: Value = serde_json::from_str(&reply).unwrap();
            // Nothing is answered before the token.
            prop_assert_eq!(&parsed["ok"], &json!(false));
        }
    }

    #[test]
    fn any_address_is_shown_without_its_password(url in prop_oneof![
        any::<String>(),
        "https?://[a-z]{0,6}(:[a-z]{0,6})?@[a-z.]{0,10}/[^ ]{0,10}".prop_map(|s| s),
    ]) {
        let shown = format::shown_url(&url);
        if let Ok(parsed) = url::Url::parse(&url) {
            if let Some(password) = parsed.password().filter(|p| p.len() > 2) {
                prop_assert!(!shown.contains(&format!(":{password}@")), "{shown}");
            }
        }
        let _ = format::provisional(&url);
        let _ = format::host(&url);
    }

    /// Any name, in any mix of scripts and directions, is put into drawing
    /// order without a crash, and keeps every character it had (brackets
    /// may face the other way).
    #[test]
    fn any_name_is_laid_out_with_all_its_letters(name in prop_oneof![
        any::<String>(),
        "[a-z .()\\[\\]0-9\u{05d0}-\u{05ea}\u{0627}-\u{064a}\u{0660}-\u{0669}\u{064b}\u{200f}\u{202e}]{0,24}".prop_map(|s| s),
    ]) {
        let shown = dm_desktop::bidi::visual(&name);
        let plain = |s: &str| {
            let mut c: Vec<char> = s
                .chars()
                .map(|c| match c {
                    ')' => '(',
                    ']' => '[',
                    '}' => '{',
                    '>' => '<',
                    '»' => '«',
                    '›' => '‹',
                    c => c,
                })
                .collect();
            c.sort_unstable();
            c
        };
        prop_assert_eq!(plain(&shown), plain(&name));
    }
}

/// The control listener the socket fuzzing talks to, made once.
fn control_port() -> u16 {
    static PORT: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
    *PORT.get_or_init(|| {
        let (to_window, requests) = std::sync::mpsc::channel();
        // Kept, or the listener's sends would fail and it would close.
        std::mem::forget(requests);
        dm_desktop::control::listen(0, "a".repeat(64), to_window, || {}).unwrap()
    })
}
