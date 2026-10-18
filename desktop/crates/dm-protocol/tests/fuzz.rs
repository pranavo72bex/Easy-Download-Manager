//! The catch rules and the address helpers, fed random and hostile input:
//! the browser sends anything, and so does a hand-edited settings file.

use dm_protocol::{extension_for_mime, extension_of, host_of, Candidate, CatchRules};
use proptest::prelude::*;

proptest! {
    #[test]
    fn any_download_is_caught_or_left_with_a_reason(
        url in any::<String>(),
        filename in any::<String>(),
        mime in any::<String>(),
        size in proptest::option::of(any::<u64>()),
        extensions in prop::collection::vec(any::<String>(), 0..6),
        min in any::<u64>(),
        skip in prop::collection::vec(any::<String>(), 0..4),
        enabled in any::<bool>(),
    ) {
        let rules = CatchRules { enabled, extensions, min_size_kb: min, skip_hosts: skip, ..CatchRules::default() };
        let candidate = Candidate { url: &url, asked_url: &filename, referrer: &mime, filename: &filename, mime: &mime, size };
        let _ = rules.takes(&candidate, enabled);
        let _ = rules.catches(&candidate);
        let _ = host_of(&url);
        let _ = extension_of(&filename);
        let _ = extension_for_mime(&mime);
        let _ = dm_protocol::page_instead_of_file(&mime, &url, &filename);
    }

    #[test]
    fn catch_rules_of_any_json_read_or_refuse(text in any::<String>()) {
        let _ = serde_json::from_str::<CatchRules>(&text);
    }
}
