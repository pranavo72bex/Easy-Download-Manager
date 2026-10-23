//! The engine's readers of outside input, fed random and hostile input.
//!
//! Panics are hunted with fuzzing, on stable Rust with proptest, inside `cargo test`, so they need no extra toolchain and
//! run in CI. A few hundred cases a run by default; for a deep hunt:
//!
//!     PROPTEST_CASES=200000 cargo test -p dm-core --test fuzz
//!
//! Beyond "does not panic", each holds what must stay true whatever arrives:
//! a name never becomes a path, a plan covers every byte exactly once.

use dm_core::checksum::Checksum;
use dm_core::stream::{self, Quality};
use dm_core::{metalink, plan, probe, Schedule};
use proptest::prelude::*;

/// Strings that look like what servers send, mixed with anything at all.
fn texty() -> impl Strategy<Value = String> {
    prop_oneof![
        any::<String>(),
        "[ -~]{0,80}",
        prop::collection::vec(
            prop_oneof![
                Just("attachment; ".to_string()),
                Just("filename=".to_string()),
                Just("filename*=UTF-8''".to_string()),
                Just("\"".to_string()),
                Just("%".to_string()),
                Just("%2F".to_string()),
                Just("../".to_string()),
                Just("\\".to_string()),
                Just(";".to_string()),
                Just("CON".to_string()),
                Just("COM\u{b9}".to_string()),
                Just("CONOUT$".to_string()),
                Just(" ".to_string()),
                Just(".".to_string()),
                Just("\u{0}".to_string()),
                Just("é".to_string()),
                Just("中".to_string()),
                "[a-z0-9]{0,6}",
            ],
            0..16
        )
        .prop_map(|parts| parts.concat()),
    ]
}

/// Lines a playlist might have, in any order.
fn playlist() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            Just("#EXTM3U".to_string()),
            Just("#EXT-X-VERSION:3".to_string()),
            "#EXT-X-STREAM-INF:BANDWIDTH=[0-9]{0,9},RESOLUTION=[0-9]{0,5}x[0-9]{0,5}"
                .prop_map(|s| s),
            "#EXTINF:[0-9.]{0,6},".prop_map(|s| s),
            Just("#EXT-X-KEY:METHOD=AES-128,URI=\"k.key\",IV=0x00".to_string()),
            "#EXT-X-KEY:METHOD=[A-Z0-9-]{0,8},URI=\"[^\"]{0,10}\",IV=0x[0-9a-fA-F]{0,40}"
                .prop_map(|s| s),
            "#EXT-X-MAP:URI=\"[a-z.]{0,8}\"(,BYTERANGE=\"[0-9@]{0,10}\")?".prop_map(|s| s),
            "#EXT-X-MEDIA-SEQUENCE:[0-9-]{0,20}".prop_map(|s| s),
            "#EXT-X-BYTERANGE:[0-9@]{0,12}".prop_map(|s| s),
            Just("#EXT-X-ENDLIST".to_string()),
            "[a-z0-9/._:?=-]{0,30}".prop_map(|s| s),
            any::<String>(),
        ],
        0..30,
    )
    .prop_map(|lines| lines.join("\n"))
}

proptest! {
    #[test]
    fn a_cookies_file_of_anything_is_read_or_skipped(text in texty(), url in texty()) {
        let cookies = dm_core::cookies::parse(&text);
        for cookie in &cookies {
            // Whatever a line held, nothing in it can split the header.
            prop_assert!(!cookie.name.contains(';') && !cookie.value.contains(';'));
            prop_assert!(!cookie.value.contains('\n'));
        }
        let _ = dm_core::cookies::header_for(&cookies, &url, 0);
        let _ = dm_core::cookies::header_for(&cookies, &format!("https://{url}/x"), 0);
    }

    #[test]
    fn an_error_page_of_anything_reads_as_a_short_sentence(page in prop_oneof![
        texty(),
        prop::collection::vec(prop_oneof![
            Just("<style>".to_string()), Just("</style>".to_string()), Just("<p>".to_string()),
            Just("<script ".to_string()), Just("</scr".to_string()), Just(">".to_string()),
            Just("<".to_string()), Just("&amp;".to_string()), Just("&".to_string()),
            Just("é中".to_string()), "[a-z ]{0,12}",
        ], 0..40).prop_map(|parts| parts.concat()),
    ]) {
        let said = dm_core::readable(&page);
        prop_assert!(said.chars().count() <= 201);
        prop_assert_eq!(said.trim(), said.as_str());
    }

    #[test]
    fn a_name_from_a_server_never_becomes_a_path(
        disposition in proptest::option::of(texty()),
        url in texty(),
        mime in proptest::option::of(texty()),
    ) {
        let name = probe::filename_for(disposition.as_deref(), &url, mime.as_deref());
        prop_assert!(!name.contains('/') && !name.contains('\\'), "{name:?}");
        prop_assert!(name != "." && name != "..");
        prop_assert!(!name.contains('\0'));
        prop_assert!(name.len() <= 240, "{} bytes", name.len());
        let _ = probe::mime_essence(mime.as_deref());
        let _ = probe::decode(&url);
    }

    #[test]
    fn sanitize_is_safe_and_settles(name in texty()) {
        let once = probe::sanitize(&name);
        prop_assert!(!once.contains('/') && !once.contains('\\'));
        prop_assert!(once.len() <= 240);
        // Cleaning a clean name changes nothing.
        prop_assert_eq!(probe::sanitize(&once), once.clone());
    }

    /// A long name cut to length could end on a space or a dot,
    /// which Windows drops and a second clean trimmed: not settled.
    #[test]
    fn a_long_name_cut_to_length_settles(
        name in "[a-z\u{4e2d}]{100,230}[ .\u{3000}]{0,6}[a-z .]{0,40}(\\.[a-z]{1,30})?",
    ) {
        let once = probe::sanitize(&name);
        prop_assert!(once.len() <= 240);
        prop_assert!(!once.ends_with([' ', '.']), "{once:?}");
        prop_assert_eq!(probe::sanitize(&once), once.clone());
    }

    #[test]
    fn a_metalink_of_anything_is_read_or_refused(xml in prop_oneof![
        any::<String>(),
        "<metalink[^>]{0,20}><file name=\"[^\"]{0,20}\"><size>[0-9]{0,22}</size>(<url>[^<]{0,30}</url>){0,3}(<hash type=\"[a-z0-9-]{0,8}\">[0-9a-f]{0,70}</hash>)?</file></metalink>",
    ]) {
        if let Some(found) = metalink::parse(&xml) {
            for url in &found.mirrors {
                prop_assert!(url.starts_with("http://") || url.starts_with("https://"), "{url}");
            }
        }
    }

    #[test]
    fn a_playlist_of_anything_is_read_or_refused(text in playlist(), base in texty()) {
        let offered = stream::variants(&text, &base);
        let _ = stream::pick(offered, Quality::UpTo(720));
        let _ = stream::media_playlist(&text, &base, "name", Quality::Best);
        let _ = stream::media_playlist(&text, "https://e.com/v/index.m3u8", "x", Quality::Audio);
        let _ = stream::kind_of(&base, Some(&text));
    }

    #[test]
    fn a_digest_of_anything_is_read_or_refused(text in texty()) {
        if let Some(checksum) = Checksum::parse(&text) {
            prop_assert!(checksum.expected.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn a_schedule_of_anything_answers(
        from in texty(),
        to in texty(),
        days in prop::collection::vec(any::<u8>(), 0..10),
        weekday in any::<u8>(),
        minute in any::<u32>(),
    ) {
        let schedule = Schedule { enabled: true, from, to, days, wake: false };
        let _ = schedule.open_at(weekday, minute);
        let _ = schedule.open();
    }

    #[test]
    fn a_plan_covers_every_byte_exactly_once(
        size in proptest::option::of(0u64..u64::MAX / 2),
        connections in any::<usize>(),
        resumable in any::<bool>(),
    ) {
        let planned = plan::plan(size, connections, resumable);
        prop_assert!(!planned.is_empty());
        if let Some(size) = size.filter(|s| *s > 0) {
            prop_assert_eq!(planned[0].start, 0);
            prop_assert_eq!(planned.last().map(|s| s.end), Some(size - 1));
            for pair in planned.windows(2) {
                prop_assert_eq!(pair[0].end + 1, pair[1].start, "a gap or an overlap");
            }
            let covered: u64 = planned.iter().map(|s| s.span()).sum();
            prop_assert_eq!(covered, size);
            prop_assert!(planned.len() <= plan::MAX_CONNECTIONS);
        }
    }
}
