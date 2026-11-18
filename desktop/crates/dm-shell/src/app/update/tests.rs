// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

#[test]
fn a_releases_notes_are_followed_only_to_the_projects_own_page() {
    let page = format!("{NOTES}desktop-v0.2.0");
    let manifest = |notes: &str| format!("{{\"version\":\"0.2.0\",\"notes\":\"{notes}\"}}");
    assert_eq!(notes_in(&manifest(&page)), Some(page));
    assert_eq!(notes_in(&manifest("https://elsewhere.example/notes")), None);
    assert_eq!(notes_in("{\"version\":\"0.2.0\"}"), None);
    assert_eq!(notes_in("not json"), None);
}

fn rules_doc(revision: u64, host: &str) -> String {
    format!(
        r#"{{ "format": 1, "revision": {revision}, "exclude": {{ "advert_hosts": ["{host}"] }} }}"#
    )
}

/// A server that answers `body` at /rules.json; its address.
async fn serve(body: Vec<u8>) -> String {
    let app = axum::Router::new().route(
        "/rules.json",
        axum::routing::get(move || {
            let body = body.clone();
            async move { body }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{at}/")
}

fn entry_for(base: &str, body: &[u8]) -> RulesEntry {
    RulesEntry {
        url: format!("{base}rules.json"),
        sha256: sha256_hex(body),
        size: body.len() as u64,
    }
}

fn home() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn the_manifests_rules_entry_is_read_only_whole_and_from_the_projects_releases() {
    let url = format!("{RELEASES}desktop-v1/rules.json");
    let good = "ab".repeat(32);
    let manifest = |url: &str, sha: &str, size: u64| {
        serde_json::json!({ "version": "1", "rules": { "url": url, "sha256": sha, "size": size } })
            .to_string()
    };
    let got = rules_in(&manifest(&url, &good.to_uppercase(), 100), RELEASES).unwrap();
    assert_eq!(got.sha256, good);
    assert_eq!((got.url.as_str(), got.size), (url.as_str(), 100));
    assert_eq!(
        rules_in(
            &manifest("https://elsewhere.example/r.json", &good, 100),
            RELEASES
        ),
        None
    );
    assert_eq!(rules_in(&manifest(&url, "abc", 100), RELEASES), None);
    assert_eq!(rules_in(&manifest(&url, &good, 0), RELEASES), None);
    assert_eq!(
        rules_in(&manifest(&url, &good, RULES_CAP + 1), RELEASES),
        None
    );
    assert_eq!(
        rules_in(r#"{"version":"1"}"#, RELEASES),
        None,
        "an older manifest"
    );
    assert_eq!(rules_in("not json", RELEASES), None);
}

#[tokio::test]
async fn newer_rules_are_kept_and_older_or_equal_ones_are_not() {
    let dir = home();
    let client = dm_core::reqwest_defaults();
    let newer = rules_doc(900, "ads.example").into_bytes();
    let base = serve(newer.clone()).await;
    assert_eq!(
        fetch_rules(&client, &entry_for(&base, &newer), dir.path()).await,
        Ok(true)
    );
    let kept = std::fs::read_to_string(dir.path().join(crate::host::served::FILE)).unwrap();
    assert_eq!(kept.as_bytes(), newer.as_slice());

    let older = rules_doc(10, "old.example").into_bytes();
    let base = serve(older.clone()).await;
    assert_eq!(
        fetch_rules(&client, &entry_for(&base, &older), dir.path()).await,
        Ok(false)
    );
    let kept = std::fs::read_to_string(dir.path().join(crate::host::served::FILE)).unwrap();
    assert_eq!(kept.as_bytes(), newer.as_slice());
}

#[tokio::test]
async fn rules_with_a_wrong_digest_size_or_content_are_refused_and_change_nothing() {
    let dir = home();
    let client = dm_core::reqwest_defaults();
    let file = dir.path().join(crate::host::served::FILE);
    let doc = rules_doc(900, "ads.example").into_bytes();
    let base = serve(doc.clone()).await;

    let mut wrong_digest = entry_for(&base, &doc);
    wrong_digest.sha256 = "0".repeat(64);
    assert!(fetch_rules(&client, &wrong_digest, dir.path())
        .await
        .is_err());

    let mut too_small = entry_for(&base, &doc);
    too_small.size -= 1;
    assert!(fetch_rules(&client, &too_small, dir.path()).await.is_err());

    let mut too_big = entry_for(&base, &doc);
    too_big.size += 1;
    assert!(fetch_rules(&client, &too_big, dir.path()).await.is_err());

    // Right digest of something that is not a rules document.
    let garbage = b"<html>not rules</html>".to_vec();
    let base = serve(garbage.clone()).await;
    assert!(
        fetch_rules(&client, &entry_for(&base, &garbage), dir.path())
            .await
            .is_err()
    );

    // Nothing there at all.
    let mut missing = entry_for(&base, &doc);
    missing.url = format!("{base}absent.json");
    assert!(fetch_rules(&client, &missing, dir.path()).await.is_err());
    assert!(!file.exists());
}

#[test]
fn the_weekly_check_is_due_after_a_week_and_not_before() {
    assert!(due(0, WEEK), "never checked");
    assert!(!due(1_000_000, 1_000_000 + WEEK - 1));
    assert!(due(1_000_000, 1_000_000 + WEEK));
    assert!(due(1_000_000, 10), "a clock set back");
}

#[test]
fn a_later_version_is_newer_and_the_same_one_is_not() {
    assert!(newer("0.2.0", "0.1.0"));
    assert!(newer("v0.1.10", "0.1.9"));
    assert!(!newer("0.1.0", "0.1.0"));
    assert!(!newer("0.0.9", "0.1.0"));
    assert_eq!(
        version_in(r#"{"version":"1.2.3","platforms":{}}"#).as_deref(),
        Some("1.2.3")
    );
    assert_eq!(version_in("not json"), None);
}

#[test]
fn an_installers_digest_is_read_by_its_name_and_only_whole() {
    let url = format!("{RELEASES}desktop-v9.9.9/dm_9.9.9_aarch64.dmg");
    let good = "AB".repeat(32);
    let manifest = serde_json::json!({
        "version": "9.9.9",
        "installers": [url],
        "sha256": {
            "dm_9.9.9_aarch64.dmg": good,
            "dm_9.9.9_x64-setup.exe": "abc",
        },
    })
    .to_string();
    assert_eq!(digest_in(&manifest, &url), Some(good.to_lowercase()));
    // Too short to be one, or not listed: none.
    let exe = format!("{RELEASES}desktop-v9.9.9/dm_9.9.9_x64-setup.exe");
    assert_eq!(digest_in(&manifest, &exe), None);
    assert_eq!(digest_in(r#"{"version":"9.9.9"}"#, &url), None);
}

#[test]
fn the_installer_for_this_machine_is_picked_by_its_name() {
    let base = format!("{RELEASES}desktop-v1.2.3");
    let manifest = serde_json::json!({
        "version": "1.2.3",
        "installers": [
            format!("{base}/dm_1.2.3_aarch64.dmg"),
            format!("{base}/dm_1.2.3_x64.dmg"),
            format!("{base}/dm_1.2.3_x64-setup.exe"),
            format!("{base}/dm_1.2.3_amd64.deb"),
            format!("{base}/dm_1.2.3_x86_64.AppImage"),
            "http://insecure.example/dm_1.2.3_aarch64.AppImage",
            "https://elsewhere.example/dm_1.2.3_arm64-setup.exe",
        ],
    })
    .to_string();
    let pick = |os, arch| installer_in(&manifest, os, arch, true);
    assert_eq!(
        pick("macos", "aarch64"),
        Some(format!("{base}/dm_1.2.3_aarch64.dmg"))
    );
    assert_eq!(
        pick("macos", "x86_64"),
        Some(format!("{base}/dm_1.2.3_x64.dmg"))
    );
    assert_eq!(
        pick("windows", "x86_64"),
        Some(format!("{base}/dm_1.2.3_x64-setup.exe"))
    );
    assert_eq!(
        pick("linux", "x86_64"),
        Some(format!("{base}/dm_1.2.3_x86_64.AppImage"))
    );
    assert_eq!(pick("linux", "aarch64"), None, "never over plain http");
    assert_eq!(
        pick("windows", "aarch64"),
        None,
        "only from dm's own releases"
    );
    assert_eq!(pick("freebsd", "x86_64"), None);
    assert_eq!(
        installer_in(&manifest, "linux", "x86_64", false),
        None,
        "a packaged dm is updated by its package manager"
    );
    assert_eq!(
        installer_in(r#"{"version":"1"}"#, "macos", "aarch64", false),
        None,
        "an older manifest"
    );
}

/// Windows on Arm has an installer of its own (gaps row R3): an arm64 Windows is offered the
/// `_arm64-setup.exe`, an x64 one the `_x64-setup.exe`, and neither a portable zip.
#[test]
fn an_arm64_windows_is_offered_the_arm64_installer() {
    let base = format!("{RELEASES}desktop-v1.2.3");
    let manifest = serde_json::json!({
        "version": "1.2.3",
        "installers": [
            format!("{base}/dm_1.2.3_aarch64.dmg"),
            format!("{base}/dm_1.2.3_arm64-portable.zip"),
            format!("{base}/dm_1.2.3_arm64.deb"),
            format!("{base}/dm_1.2.3_x64-portable.zip"),
            format!("{base}/dm_1.2.3_x64-setup.exe"),
            format!("{base}/dm_1.2.3_arm64-setup.exe"),
        ],
    })
    .to_string();
    assert_eq!(
        installer_in(&manifest, "windows", "aarch64", false),
        Some(format!("{base}/dm_1.2.3_arm64-setup.exe"))
    );
    assert_eq!(
        installer_in(&manifest, "windows", "x86_64", false),
        Some(format!("{base}/dm_1.2.3_x64-setup.exe"))
    );
    // A release from before Windows on Arm was built: nothing for an arm64 Windows, rather
    // than the x64 installer, which it would run only under emulation.
    let older = serde_json::json!({
        "version": "1.2.3",
        "installers": [format!("{base}/dm_1.2.3_x64-setup.exe")],
    })
    .to_string();
    assert_eq!(installer_in(&older, "windows", "aarch64", false), None);
}

/// A proxy on loopback that refuses every tunnel, and how many it was asked
/// for: what the update check reaches in place of the update server.
fn refusing_proxy(runtime: &tokio::runtime::Runtime) -> (u16, std::sync::Arc<Mutex<Vec<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let asked = std::sync::Arc::new(Mutex::new(Vec::new()));
    let seen = asked.clone();
    let port = runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut tcp, _)) = listener.accept().await {
                let seen = seen.clone();
                tokio::spawn(async move {
                    let mut head = vec![0u8; 4096];
                    let n = tcp.read(&mut head).await.unwrap_or(0);
                    let line = String::from_utf8_lossy(&head[..n])
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .to_string();
                    seen.lock().unwrap().push(line);
                    let _ = tcp
                        .write_all(b"HTTP/1.1 403 Forbidden\r\ncontent-length: 0\r\n\r\n")
                        .await;
                });
            }
        });
        port
    });
    (port, asked)
}

#[test]
fn an_update_check_the_way_out_refuses_fails_with_why() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (port, asked) = refusing_proxy(&runtime);
    // The proxy a person sets in Options: the check is asked through it, with
    // the application's own client (`Backend::client`).
    let dir = tempfile::tempdir().unwrap();
    let settings = crate::Settings {
        proxy: dm_core::Proxy {
            enabled: true,
            host: "127.0.0.1".into(),
            port,
            ..dm_core::Proxy::default()
        },
        ..crate::Settings::default()
    };
    let backend = crate::app::backend::testing::backend_with(&runtime, dir.path(), settings);
    let into = std::sync::Arc::new(Mutex::new(Update::NotAsked));
    let (woken, wake) = std::sync::mpsc::channel();
    check(&backend, "0.0.1", into.clone(), move || {
        let _ = woken.send(());
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let now = into.lock().unwrap().clone();
        match now {
            Update::Asking => {}
            Update::Failed(why) => {
                assert!(!why.is_empty());
                break;
            }
            other => panic!("the check came to {other:?} through a proxy that refuses it"),
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the check never ended"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        wake.recv_timeout(std::time::Duration::from_secs(5)).is_ok(),
        "never woken"
    );
    let asked = asked.lock().unwrap();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(
        asked[0].starts_with("CONNECT github.com:443"),
        "the check went elsewhere: {asked:?}"
    );
}

#[test]
fn the_weekly_check_is_stamped_when_due_and_wanted() {
    let settings = crate::Settings {
        check_updates: true,
        update_checked: 1_000,
        ..crate::Settings::default()
    };
    assert!(
        weekly(&settings, 1_000 + WEEK - 1).is_none(),
        "not a week yet"
    );
    let wanted = weekly(&settings, 1_000 + WEEK).unwrap();
    assert_eq!(wanted.update_checked, 1_000 + WEEK);
    assert!(weekly(&wanted, 1_000 + WEEK + 60).is_none(), "stamped");
    let off = crate::Settings {
        check_updates: false,
        ..settings
    };
    assert!(weekly(&off, 1_000 + 2 * WEEK).is_none(), "not wanted");
}

/// A key pair for the tests alone, made afresh each run: no private key is written anywhere.
fn test_pair() -> (ring::signature::Ed25519KeyPair, String) {
    use base64::Engine as _;
    use ring::signature::KeyPair as _;
    let pkcs8 =
        ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
    let pair = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
    let public = base64::engine::general_purpose::STANDARD.encode(pair.public_key().as_ref());
    (pair, public)
}

fn signature_of(pair: &ring::signature::Ed25519KeyPair, bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(pair.sign(bytes).as_ref())
}

/// A stand-in for the releases page: `manifest` at /latest.json and, when given, `signature`
/// at /latest.json.sig (a 404 without); the manifest's address.
async fn serve_manifest(manifest: Vec<u8>, signature: Option<String>) -> String {
    let mut app = axum::Router::new().route(
        "/latest.json",
        axum::routing::get(move || {
            let body = manifest.clone();
            async move { body }
        }),
    );
    if let Some(signature) = signature {
        app = app.route(
            "/latest.json.sig",
            axum::routing::get(move || {
                let body = signature.clone();
                async move { body }
            }),
        );
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{at}/latest.json")
}

const MANIFEST: &str = r#"{"version":"9.9.9","installers":[],"sha256":{}}"#;

#[tokio::test]
async fn a_manifest_signed_by_the_projects_key_is_believed() {
    let (pair, public) = test_pair();
    let client = dm_core::reqwest_defaults();
    let signed = signature_of(&pair, MANIFEST.as_bytes());
    let at = serve_manifest(MANIFEST.into(), Some(format!("{signed}\n"))).await;
    assert_eq!(
        fetch_manifest(&client, &at, &public).await,
        Ok(Some(MANIFEST.to_owned()))
    );
}

#[tokio::test]
async fn a_manifest_unsigned_signed_by_another_key_or_changed_is_refused() {
    let (pair, public) = test_pair();
    let (theirs, _) = test_pair();
    let client = dm_core::reqwest_defaults();
    let refused = Err(UNSIGNED.to_owned());

    let unsigned = serve_manifest(MANIFEST.into(), None).await;
    assert_eq!(fetch_manifest(&client, &unsigned, &public).await, refused);

    let wrong_key = serve_manifest(
        MANIFEST.into(),
        Some(signature_of(&theirs, MANIFEST.as_bytes())),
    )
    .await;
    assert_eq!(fetch_manifest(&client, &wrong_key, &public).await, refused);

    // One byte changed after signing: a version, for one.
    let changed = MANIFEST.replace("9.9.9", "9.9.8");
    let tampered = serve_manifest(
        changed.into_bytes(),
        Some(signature_of(&pair, MANIFEST.as_bytes())),
    )
    .await;
    assert_eq!(fetch_manifest(&client, &tampered, &public).await, refused);

    let junk = serve_manifest(MANIFEST.into(), Some("not a signature".into())).await;
    assert_eq!(fetch_manifest(&client, &junk, &public).await, refused);
    let empty = serve_manifest(MANIFEST.into(), Some(String::new())).await;
    assert_eq!(fetch_manifest(&client, &empty, &public).await, refused);
}

#[tokio::test]
async fn no_release_at_all_is_not_a_refusal() {
    let (_, public) = test_pair();
    let client = dm_core::reqwest_defaults();
    let at = serve_manifest(MANIFEST.into(), None).await;
    let missing = at.replace("latest.json", "nothing.json");
    assert_eq!(fetch_manifest(&client, &missing, &public).await, Ok(None));
}

#[test]
fn a_signature_is_checked_against_exactly_the_bytes_and_a_whole_key() {
    let (pair, public) = test_pair();
    let good = signature_of(&pair, MANIFEST.as_bytes());
    assert!(signed_by(&public, MANIFEST.as_bytes(), &good));
    assert!(signed_by(
        &format!(" {public}\n"),
        MANIFEST.as_bytes(),
        &format!("{good}\r\n")
    ));
    assert!(!signed_by(&public, b"", &good));
    assert!(!signed_by("", MANIFEST.as_bytes(), &good));
    assert!(
        !signed_by("AAAA", MANIFEST.as_bytes(), &good),
        "not a whole key"
    );
    // The key dm carries is a whole Ed25519 key, and nobody's signature here is its.
    use base64::Engine as _;
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(SIGNING_KEY)
            .unwrap()
            .len(),
        32
    );
    assert!(!signed_by(SIGNING_KEY, MANIFEST.as_bytes(), &good));
    assert_eq!(signature_at(ENDPOINT), format!("{ENDPOINT}.sig"));
}
