// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The update check's manifest and versions, read from what a release
//! publishes: what does not read offers nothing.

use edm_shell::app::update;

#[test]
fn a_manifest_that_does_not_read_offers_nothing() {
    let url = format!("{}desktop-v9/edm_9_aarch64.dmg", update::RELEASES);
    assert_eq!(update::digest_in("not json", &url), None);
    assert_eq!(
        update::digest_in(r#"{"sha256":{"edm_9_aarch64.dmg":7}}"#, &url),
        None
    );
    let not_hex = serde_json::json!({ "sha256": { "edm_9_aarch64.dmg": "g".repeat(64) } });
    assert_eq!(update::digest_in(&not_hex.to_string(), &url), None);
    assert_eq!(
        update::installer_in("not json", "macos", "aarch64", true),
        None
    );
    assert_eq!(
        update::installer_in(r#"{"installers":"one"}"#, "macos", "aarch64", true),
        None
    );
    let listed = serde_json::json!({ "installers": [url, 7] }).to_string();
    assert_eq!(
        update::installer_in(&listed, "macos", "riscv64", true),
        None,
        "no installer for an unknown machine"
    );
    assert_eq!(
        update::installer_in(&listed, "macos", "aarch64", false).as_deref(),
        Some(url.as_str())
    );
    assert_eq!(update::version_in(r#"{"version":7}"#), None);
    // A notes page with a space or a quote in it is not followed.
    let notes = |n: &str| serde_json::json!({ "notes": n }).to_string();
    assert_eq!(
        update::notes_in(&notes(&format!("{}v1 x", update::NOTES))),
        None
    );
    assert_eq!(
        update::notes_in(&notes(&format!("{}v1\"", update::NOTES))),
        None
    );
    assert_eq!(update::notes_in(r#"{"notes":7}"#), None);
    // A rules entry with a quote in its address, or no size, is not read.
    let rules = |url: &str, size: serde_json::Value| {
        serde_json::json!({ "rules": { "url": url, "sha256": "a".repeat(64), "size": size } })
            .to_string()
    };
    let at = format!("{}v1/rules.json", update::RELEASES);
    assert!(update::rules_in(&rules(&at, 10.into()), update::RELEASES).is_some());
    assert_eq!(
        update::rules_in(&rules(&format!("{at}\""), 10.into()), update::RELEASES),
        None
    );
    assert_eq!(
        update::rules_in(&rules(&at, "ten".into()), update::RELEASES),
        None
    );
    let bad_hex =
        serde_json::json!({ "rules": { "url": at, "sha256": "z".repeat(64), "size": 1 } });
    assert_eq!(
        update::rules_in(&bad_hex.to_string(), update::RELEASES),
        None
    );
}

#[test]
fn a_version_with_a_suffix_is_compared_by_its_numbers() {
    assert!(update::newer("1.2.10-beta", "1.2.9"));
    assert!(!update::newer("1.2.9+build7", "1.2.9"));
    assert!(update::newer("1.3", "1.2.99"));
    assert!(!update::newer("not a version", "0.0.1"));
    assert!(update::newer("0.0.1", "not a version"));
}

/// Held to a network that has no address, the update check reaches nothing: the client it is
/// made with (`Settings::client`) is bound as every other, so a VPN that dropped does not let
/// the check go out over the plain line (gaps row H24c).
#[tokio::test(flavor = "multi_thread")]
async fn held_to_a_network_with_no_address_the_update_check_reaches_nothing() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let came = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&came);
    let server = tokio::spawn(async move {
        while listener.accept().await.is_ok() {
            counted.fetch_add(1, Ordering::SeqCst);
        }
    });
    // The adapter list holds no `edm-test0`: held to it, clients are bound to an address no
    // computer has.
    edm_core::set_adapter_lister(Some(Arc::new(Vec::new)));
    edm_core::set_only_through(Some("edm-test0"));
    let client = edm_shell::Settings::default().client().unwrap();
    let endpoint = format!("http://{addr}/latest.json");
    let checked = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        update::fetch_manifest(&client, &endpoint, "key"),
    )
    .await;
    edm_core::set_only_through(None);
    edm_core::set_adapter_lister(None);
    server.abort();
    assert!(
        !matches!(checked, Ok(Ok(_))),
        "the update check answered with no network"
    );
    assert_eq!(
        came.load(Ordering::SeqCst),
        0,
        "the update check reached the server"
    );
}
