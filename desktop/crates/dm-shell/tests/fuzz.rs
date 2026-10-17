//! The shell's readers of outside input — what a browser sends the native
//! host, lists and pages people import, settings files people edit — fed
//! random and hostile input. `PROPTEST_CASES=200000` for a deep hunt.

use dm_shell::host::{self, World};
use dm_shell::lists;
use dm_shell::Settings;
use proptest::prelude::*;
use serde_json::{json, Value};

/// A world where nothing is real: no app, no network.
struct Nowhere;

impl World for Nowhere {
    fn app_running(&mut self) -> bool {
        true
    }
    fn launch(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn reachable(
        &mut self,
        _url: &str,
        _referrer: &str,
        _cookies: &str,
        _within_ms: u64,
    ) -> Result<String, String> {
        Ok(String::new())
    }
    fn formats(&mut self, _url: &str, _referrer: &str) -> Result<Vec<dm_protocol::Choice>, String> {
        Ok(Vec::new())
    }
}

/// Any JSON at all, a few levels deep.
fn any_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|n| json!(n)),
        any::<f64>().prop_map(|n| json!(n)),
        any::<String>().prop_map(Value::String),
        "[a-z.]{0,20}".prop_map(Value::String),
    ];
    leaf.prop_recursive(3, 24, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
            prop::collection::hash_map("[a-zA-Z]{0,12}", inner, 0..6)
                .prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

/// Messages shaped like the extension's, with anything in the fields.
fn message() -> impl Strategy<Value = Value> {
    prop_oneof![
        any_json(),
        (
            prop_oneof![Just(json!(1)), any_json()],
            prop_oneof![
                Just("engine.hello"),
                Just("download.start"),
                Just("settings.get"),
                Just("nonsense"),
            ],
            any_json(),
            prop_oneof![any::<String>(), "https?://[a-z.]{1,10}/[a-z./]{0,10}"],
            any::<String>(),
            any_json(),
        )
            .prop_map(|(version, kind, id, url, referrer, caught)| {
                json!({ "protocolVersion": version, "type": kind, "id": id,
                        "url": url, "referrer": referrer, "caught": caught })
            }),
    ]
}

proptest! {
    #[test]
    fn the_host_answers_any_message_or_none(frame in message()) {
        let home = tempfile::tempdir().unwrap();
        let _ = host::answer(home.path(), &frame, &mut Nowhere);
        // And nothing it wrote escaped dm's home.
        for entry in walk(home.path()) {
            prop_assert!(entry.starts_with(home.path()));
        }
    }

    #[test]
    fn a_frame_of_any_bytes_is_read_or_refused(bytes in prop::collection::vec(any::<u8>(), 0..200)) {
        let _ = host::read_frame(&mut &bytes[..]);
    }

    #[test]
    fn a_settings_file_of_anything_reads_as_settings(value in any_json()) {
        if let Ok(settings) = serde_json::from_value::<Settings>(value) {
            let settings = settings.clamped();
            prop_assert!((1..=32).contains(&settings.connections));
            prop_assert!((1..=10).contains(&settings.concurrency));
            let _ = settings.with_login("https://e.com/f");
        }
    }

    #[test]
    fn a_list_of_anything_imports_only_links(text in prop_oneof![
        any::<String>(),
        prop::collection::vec(prop_oneof![
            Just("<".to_string()), Just(">".to_string()),
            "https?://[a-z]{1,8}\\.com/[a-z]{0,8}".prop_map(|s| s),
            "referer: [^\\n]{0,20}".prop_map(|s| s),
            any::<String>(),
        ], 0..20).prop_map(|l| l.join("\r\n")),
    ]) {
        for row in lists::from_list(&text) {
            prop_assert!(dm_core::queue::accepted(&row.url), "{}", row.url);
        }
        // What is written reads back as itself.
        let rows = lists::from_list(&text);
        prop_assert_eq!(lists::from_list(&lists::to_list(&rows)), rows);
    }

    #[test]
    fn a_page_of_anything_gives_only_links(
        html in prop_oneof![
            any::<String>(),
            prop::collection::vec(prop_oneof![
                "<a href=\"[^\"]{0,20}\">".prop_map(|s| s),
                "<img src='[^']{0,20}'>".prop_map(|s| s),
                "href=[^ >]{0,15}".prop_map(|s| s),
                Just("é中".to_string()),
                any::<String>(),
            ], 0..20).prop_map(|p| p.concat()),
        ],
        base in prop_oneof![Just("https://e.com/dir/page.html".to_string()), any::<String>()],
    ) {
        for link in lists::links_on_page(&base, &html) {
            prop_assert!(dm_core::queue::accepted(&link), "{link}");
            let _ = lists::has_extension(&link, &["zip".into()]);
        }
    }

    #[test]
    fn a_batch_of_anything_is_made_or_refused(
        template in any::<String>(),
        from in "[0-9a-zA-Z]{0,6}|.{0,4}",
        to in "[0-9a-zA-Z]{0,6}|.{0,4}",
        width in 0usize..12,
    ) {
        if let Ok(urls) = lists::series(&template, &from, &to, width) {
            prop_assert!(urls.len() <= 10_000);
        }
    }
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(walk(&path));
            }
            found.push(path);
        }
    }
    found
}
