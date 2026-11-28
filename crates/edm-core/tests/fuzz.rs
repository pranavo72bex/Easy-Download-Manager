// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The engine's readers of outside input, fed random and hostile input.
//!
//! Panics are hunted with fuzzing, on stable Rust with proptest, inside `cargo test`, so they need no extra toolchain and
//! run in CI. A few hundred cases a run by default; for a deep hunt:
//!
//!     PROPTEST_CASES=200000 cargo test -p edm-core --test fuzz
//!
//! Beyond "does not panic", each holds what must stay true whatever arrives:
//! a name never becomes a path, a plan covers every byte exactly once.

use edm_core::Checksum;
use edm_core::Schedule;
use edm_core::{
    decode_percent, filename_for, kind_of, media_playlist, mime_essence, parse_metalink,
    pick_quality, plan, sanitize_name, stream_variants, Quality, MAX_CONNECTIONS,
};
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
        let cookies = edm_core::cookies::parse(&text);
        for cookie in &cookies {
            // Whatever a line held, nothing in it can split the header.
            prop_assert!(!cookie.name.contains(';') && !cookie.value.contains(';'));
            prop_assert!(!cookie.value.contains('\n'));
        }
        let _ = edm_core::cookies::header_for(&cookies, &url, 0);
        let _ = edm_core::cookies::header_for(&cookies, &format!("https://{url}/x"), 0);
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
        let said = edm_core::readable(&page);
        prop_assert!(said.chars().count() <= 201);
        prop_assert_eq!(said.trim(), said.as_str());
    }

    #[test]
    fn a_name_from_a_server_never_becomes_a_path(
        disposition in proptest::option::of(texty()),
        url in texty(),
        mime in proptest::option::of(texty()),
    ) {
        let name = filename_for(disposition.as_deref(), &url, mime.as_deref());
        prop_assert!(!name.contains('/') && !name.contains('\\'), "{name:?}");
        prop_assert!(name != "." && name != "..");
        prop_assert!(!name.contains('\0'));
        prop_assert!(name.len() <= 240, "{} bytes", name.len());
        let _ = mime_essence(mime.as_deref());
        let _ = decode_percent(&url);
    }

    #[test]
    fn sanitize_is_safe_and_settles(name in texty()) {
        let once = sanitize_name(&name);
        prop_assert!(!once.contains('/') && !once.contains('\\'));
        prop_assert!(once.len() <= 240);
        // Cleaning a clean name changes nothing.
        prop_assert_eq!(sanitize_name(&once), once.clone());
    }

    /// A long name cut to length could end on a space or a dot,
    /// which Windows drops and a second clean trimmed: not settled.
    #[test]
    fn a_long_name_cut_to_length_settles(
        name in "[a-z\u{4e2d}]{100,230}[ .\u{3000}]{0,6}[a-z .]{0,40}(\\.[a-z]{1,30})?",
    ) {
        let once = sanitize_name(&name);
        prop_assert!(once.len() <= 240);
        prop_assert!(!once.ends_with([' ', '.']), "{once:?}");
        prop_assert_eq!(sanitize_name(&once), once.clone());
    }

    #[test]
    fn a_metalink_of_anything_is_read_or_refused(xml in prop_oneof![
        any::<String>(),
        "<metalink[^>]{0,20}><file name=\"[^\"]{0,20}\"><size>[0-9]{0,22}</size>(<url>[^<]{0,30}</url>){0,3}(<hash type=\"[a-z0-9-]{0,8}\">[0-9a-f]{0,70}</hash>)?</file></metalink>",
    ]) {
        if let Some(found) = parse_metalink(&xml) {
            for url in &found.mirrors {
                prop_assert!(url.starts_with("http://") || url.starts_with("https://"), "{url}");
            }
        }
    }

    #[test]
    fn a_playlist_of_anything_is_read_or_refused(text in playlist(), base in texty()) {
        let offered = stream_variants(&text, &base);
        let _ = pick_quality(offered, Quality::UpTo(720));
        let _ = media_playlist(&text, &base, "name", Quality::Best);
        let _ = media_playlist(&text, "https://e.com/v/index.m3u8", "x", Quality::Audio);
        let _ = kind_of(&base, Some(&text));
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
        let schedule = Schedule { enabled: true, from, to, days, wake: false, when_done: String::new(), retries: 0, run_when_done: String::new(), hang_up: false, once_at: None, force: false };
        let _ = schedule.open_at(weekday, minute);
        let _ = schedule.open();
    }

    #[test]
    fn a_plan_covers_every_byte_exactly_once(
        size in proptest::option::of(0u64..u64::MAX / 2),
        connections in any::<usize>(),
        resumable in any::<bool>(),
    ) {
        let planned = plan(size, connections, resumable);
        prop_assert!(!planned.is_empty());
        if let Some(size) = size.filter(|s| *s > 0) {
            prop_assert_eq!(planned[0].start, 0);
            prop_assert_eq!(planned.last().map(|s| s.end), Some(size - 1));
            for pair in planned.windows(2) {
                prop_assert_eq!(pair[0].end + 1, pair[1].start, "a gap or an overlap");
            }
            let covered: u64 = planned.iter().map(|s| s.span()).sum();
            prop_assert_eq!(covered, size);
            prop_assert!(planned.len() <= MAX_CONNECTIONS);
        }
    }
}

// The header readers a server's answer reaches (BUGLOG.md, Step 3).
proptest! {
    #[test]
    fn a_retry_after_of_anything_is_read_or_ignored_and_never_waits_long(
        value in prop_oneof![texty(), "[0-9]{1,25}", Just("Wed, 21 Oct 2099 07:28:00 GMT".to_string())],
        date in proptest::option::of(prop_oneof![texty(), Just("Wed, 21 Oct 2015 07:28:00 GMT".to_string())]),
        attempt in 0u32..1000,
    ) {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Ok(v) = reqwest::header::HeaderValue::from_str(&value) {
            headers.insert(reqwest::header::RETRY_AFTER, v);
        }
        if let Some(Ok(v)) = date.as_deref().map(reqwest::header::HeaderValue::from_str) {
            headers.insert(reqwest::header::DATE, v);
        }
        let asked = edm_core::retry::retry_after(&headers);
        // However long a server asks for, one wait is at most a minute, and
        // EDM's own backoff at most half a minute and its jitter.
        let wait = edm_core::retry::backoff(attempt, asked);
        prop_assert!(wait <= std::time::Duration::from_secs(60), "{wait:?}");
        prop_assert!(edm_core::retry::backoff(attempt, None) <= std::time::Duration::from_millis(30_400));
    }

    #[test]
    fn a_login_challenge_of_anything_is_read_or_refused(header in texty(), scheme in texty()) {
        let _ = edm_core::auth::challenge(&header);
        let _ = edm_core::ntlm::challenge_in(&header, &scheme);
        let _ = edm_core::ntlm::offered(&header);
    }

    #[test]
    fn a_cookie_header_of_anything_is_kept_to_its_site(url in texty(), header in texty()) {
        if let Some(sign_in) = edm_core::cookies::SignIn::new(&url, &header) {
            let _ = edm_core::cookies::header_with(Some(&sign_in), &url);
            // Never sent to a site the sign-in was not given for.
            prop_assert!(edm_core::cookies::header_with(Some(&sign_in), "https://other.invalid/").is_none());
        }
    }
}

// The dates servers and people write (`edm_core::clock`, gaps row 215).
proptest! {
    #[test]
    fn a_date_of_anything_is_read_or_refused(text in prop_oneof![
        texty(),
        "[0-9 :.,+TtZz()-]{0,40}",
        Just("Sun, 06 Nov 1994 08:49:37 GMT".to_string()),
    ]) {
        let _ = edm_core::clock::rfc2822(&text);
        let _ = edm_core::clock::rfc3339(&text);
        let _ = edm_core::clock::iso_utc(&text);
        let _ = edm_core::clock::ymd(&text);
        let _ = edm_core::clock::ymd_hm(&text);
    }

    #[test]
    fn an_http_date_edm_writes_reads_back(unix in -62_167_219_200i64..=253_402_300_799) {
        let text = edm_core::clock::http_date(unix).unwrap();
        prop_assert_eq!(edm_core::clock::rfc2822(&text).map(|at| at.unix), Some(unix), "{}", text);
    }

    #[test]
    fn a_written_date_reads_back(days in -95_000_000i64..95_000_000) {
        if let Some(date) = edm_core::clock::Date::from_days_since_epoch(days) {
            prop_assert_eq!(date.days_since_epoch(), days);
            prop_assert_eq!(edm_core::clock::ymd(&date.to_string()), Some(date));
        }
    }
}

// YouTube's framed answers and the messages inside them (`edm_core::youtube::framed`, gaps row Y2).
proptest! {
    #[test]
    fn a_framed_answer_of_anything_is_read_or_refused(
        bytes in prop::collection::vec(any::<u8>(), 0..600),
        cut in 1usize..64,
    ) {
        use edm_core::youtube::framed::{context, messages, ump, wire};
        let mut reader = ump::Reader::new();
        let mut refused = false;
        for chunk in bytes.chunks(cut) {
            reader.push(chunk);
            while !refused {
                match reader.next_part() {
                    Ok(Some((_, data))) => {
                        let _ = messages::MediaHeader::read(&data);
                        let _ = messages::FormatStart::read(&data);
                        let _ = messages::NextAsk::read(&data);
                        let _ = messages::Head::read(&data);
                        let _ = messages::media_of(&data);
                        let _ = messages::error_of(&data);
                        let _ = messages::redirect_of(&data);
                        let _ = context::Update::read(&data);
                        let _ = context::SendingPolicy::read(&data);
                        let _ = wire::Message(&data).whole();
                    }
                    Ok(None) => break,
                    Err(_) => refused = true,
                }
            }
        }
        // Whatever was not refused is held whole or in part, never more than was pushed.
        prop_assert!(reader.left() <= bytes.len());
    }

    #[test]
    fn framed_numbers_edm_writes_read_back(value in any::<u64>(), text in texty()) {
        use edm_core::youtube::framed::{messages, ump, wire};
        let mut bytes = Vec::new();
        wire::put_varint(&mut bytes, value);
        prop_assert_eq!(wire::varint(&bytes), Some((value, bytes.len())));
        let small = value & 0xffff_ffff;
        let mut framed = Vec::new();
        ump::put_varint(&mut framed, small);
        prop_assert_eq!(ump::varint(&framed), Some((small, framed.len())));
        let _ = messages::xtag_pairs(&text);
    }
}
