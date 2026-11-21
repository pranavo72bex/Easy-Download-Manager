// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

fn file(name: &str, wanted: bool, priority: Priority) -> TorrentFile {
    TorrentFile {
        name: name.into(),
        size: 1,
        wanted,
        priority,
    }
}

#[test]
fn files_are_fetched_high_then_normal_then_low_and_those_left_out_never() {
    let files = [
        file("a", true, Priority::Low),
        file("b", true, Priority::Normal),
        file("c", true, Priority::High),
        file("d", false, Priority::High),
        file("e", true, Priority::High),
    ];
    assert_eq!(tiers(&files), vec![vec![2, 4], vec![1], vec![0]]);
    // All Normal, or one tier however it is called: fetched together.
    let plain = [
        file("a", true, Priority::Normal),
        file("b", true, Priority::Normal),
    ];
    assert_eq!(tiers(&plain).len(), 1);
    let low = [
        file("a", true, Priority::Low),
        file("b", false, Priority::High),
    ];
    assert_eq!(tiers(&low), vec![vec![0]]);
    // Every tier's files are the wanted ones, not the first tier's alone.
    let options = TorrentOptions {
        only: Some(vec![2, 4]),
        tiers: tiers(&files),
        ..Default::default()
    };
    assert_eq!(options.wanted(), Some(vec![0, 1, 2, 4]));
    let untiered = TorrentOptions {
        only: Some(vec![1]),
        ..Default::default()
    };
    assert_eq!(untiered.wanted(), Some(vec![1]));
}

#[test]
fn a_normal_priority_is_not_written_and_an_old_file_reads_as_normal() {
    let plain = serde_json::to_string(&file("a", true, Priority::Normal)).unwrap();
    assert!(!plain.contains("priority"), "{plain}");
    let high = serde_json::to_string(&file("a", true, Priority::High)).unwrap();
    assert!(high.contains(r#""priority":"high""#), "{high}");
    let old: TorrentFile = serde_json::from_str(r#"{"name":"a","size":1}"#).unwrap();
    assert_eq!((old.wanted, old.priority), (true, Priority::Normal));
}

#[test]
fn behind_a_proxy_only_http_trackers_are_kept() {
    let magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567\
        &tr=udp%3A%2F%2Ft.example%3A6969&tr=http%3A%2F%2Ft.example%2Fannounce&dn=x";
    let kept = super::tcp_trackers_of_magnet(magnet).unwrap();
    assert!(!kept.contains("udp"), "{kept}");
    assert!(kept.contains("tr=http%3A%2F%2Ft.example%2Fannounce") && kept.contains("dn=x"));
    assert!(super::tcp_trackers_of_magnet(
        "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&tr=udp%3A%2F%2Fa%3A1"
    )
    .is_err());

    // A .torrent: the udp tracker dropped, the info kept byte for byte.
    let info = b"d6:lengthi3e4:name1:x12:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
    let b = |s: &str| format!("{}:{s}", s.len());
    let (udp, http) = ("udp://t.example:69", "http://t.example/announce");
    let mut file = format!(
        "d{}{}{}ll{}el{}ee{}",
        b("announce"),
        b(udp),
        b("announce-list"),
        b(udp),
        b(http),
        b("info")
    )
    .into_bytes();
    file.extend_from_slice(info);
    file.push(b'e');
    let before = librqbit::torrent_from_bytes(&file).unwrap().info_hash;
    let kept = super::tcp_trackers_of(&file).unwrap();
    let parsed = librqbit::torrent_from_bytes(&kept).unwrap();
    assert_eq!(parsed.info_hash, before, "another torrent");
    let trackers: Vec<String> = parsed
        .iter_announce()
        .map(|t| String::from_utf8_lossy(t.as_ref()).into_owned())
        .collect();
    assert_eq!(trackers, ["http://t.example/announce"]);
}

#[test]
fn a_torrent_with_only_udp_trackers_or_none_is_refused_behind_a_proxy() {
    let info = b"d6:lengthi3e4:name1:x12:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
    let mut udp_only = b"d8:announce18:udp://t.example:694:info".to_vec();
    udp_only.extend_from_slice(info);
    udp_only.push(b'e');
    let mut none = b"d4:info".to_vec();
    none.extend_from_slice(info);
    none.push(b'e');
    for file in [udp_only, none] {
        let why = super::tcp_trackers_of(&file).unwrap_err().to_string();
        assert!(why.contains("http or https trackers"), "{why}");
    }
    let why = super::tcp_trackers_of(b"not bencode")
        .unwrap_err()
        .to_string();
    assert!(why.contains("not a torrent"), "{why}");
}

/// A blocklist served on 127.0.0.1, counting how often it was fetched.
async fn blocklist_server(status: u16) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let hits = std::sync::Arc::new(AtomicUsize::new(0));
    let counted = hits.clone();
    let app = axum::Router::new().route(
        "/list.p2p",
        axum::routing::get(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            async move {
                (
                    axum::http::StatusCode::from_u16(status).unwrap(),
                    "bad:1.2.3.4-1.2.3.5\n",
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/list.p2p"), hits)
}

#[tokio::test]
async fn a_blocklist_on_the_web_is_fetched_once_a_day_and_kept_beside_the_parts() {
    use std::sync::atomic::Ordering;
    let (address, hits) = blocklist_server(200).await;
    let dir = tempfile::tempdir().unwrap();
    let parts = dir.path().join("parts");
    let client = crate::reqwest_defaults();
    let first = super::blocklist_file(&client, &format!(" {address} "), &parts)
        .await
        .unwrap();
    let kept = local_file(&first).unwrap();
    assert_eq!(kept.parent(), Some(dir.path()), "beside the parts folder");
    assert_eq!(std::fs::read(&kept).unwrap(), b"bad:1.2.3.4-1.2.3.5\n");
    assert!(!kept.with_extension("part").exists(), "no half copy left");
    let again = super::blocklist_file(&client, &address, &parts)
        .await
        .unwrap();
    assert_eq!(again, first);
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "a fresh copy is not fetched again"
    );
}

#[tokio::test]
async fn a_blocklist_that_cannot_be_read_is_an_error_not_no_list() {
    let dir = tempfile::tempdir().unwrap();
    let client = crate::reqwest_defaults();
    let (missing, _) = blocklist_server(404).await;
    let why = super::blocklist_file(&client, &missing, dir.path())
        .await
        .unwrap_err()
        .to_string();
    assert!(
        why.contains("could not be read") && why.contains("404"),
        "{why}"
    );
    let nowhere = dir.path().join("no-such-list.p2p");
    let why = super::blocklist_file(&client, &nowhere.display().to_string(), dir.path())
        .await
        .unwrap_err()
        .to_string();
    assert!(why.contains("there is no such file"), "{why}");
}

#[tokio::test]
async fn a_blocklist_on_this_computer_is_handed_over_as_a_file_address() {
    let dir = tempfile::tempdir().unwrap();
    let list = dir.path().join("list.p2p");
    std::fs::write(&list, b"x").unwrap();
    let client = crate::reqwest_defaults();
    let address = super::blocklist_file(&client, &list.display().to_string(), dir.path())
        .await
        .unwrap();
    assert_eq!(local_file(&address).as_deref(), Some(list.as_path()));
    // An address already in that spelling goes as it is.
    let given = super::blocklist_file(&client, "file:///x/list.p2p", dir.path())
        .await
        .unwrap();
    assert_eq!(given, "file:///x/list.p2p");
}

use super::*;

/// Librqbit's count dropped (a piece failing its hash, a
/// check of the files) and the row's progress went back.
#[test]
fn reported_progress_never_goes_back() {
    let mut said = None;
    let mut seen = Vec::new();
    for progress in [0, 400, 900, 512, 700, 1200, 1100, 2048, 4096] {
        let done = high_water(said, progress, Some(2048));
        seen.push(done);
        said = Some(done);
    }
    assert_eq!(seen, [0, 400, 900, 900, 900, 1200, 1200, 2048, 2048]);
}

/// A move across disks copied straight into the download
/// folder, so a copy that failed midway left half a tree there looking
/// finished; and one whose source could not be removed failed the row
/// beside a whole copy.
#[cfg(unix)]
#[test]
fn a_move_across_disks_is_whole_or_absent_and_a_leftover_is_only_noted() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let holder = tmp.path().join("parts");
    let from = holder.join("Show");
    std::fs::create_dir_all(from.join("a/b")).unwrap();
    std::fs::write(from.join("one.bin"), b"one").unwrap();
    std::fs::write(from.join("a/b/two.bin"), b"two").unwrap();
    let unreadable = from.join("a/b/locked.bin");
    std::fs::write(&unreadable, b"x").unwrap();
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&unreadable).is_ok() {
        // Root reads past permissions (a container): nothing to test.
        return;
    }
    let downloads = tmp.path().join("downloads");
    let to = downloads.join("Show");
    std::fs::create_dir_all(&to).unwrap();

    assert!(
        copy_across(&from, &to).is_err(),
        "an unreadable file copied"
    );
    let listed = |dir: &Path| -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    assert_eq!(listed(&downloads), ["Show"], "a staging copy was left");
    assert!(listed(&to).is_empty(), "half a tree in the download folder");

    std::fs::remove_file(&unreadable).unwrap();
    // The source's folder cannot be changed: its removal will fail.
    std::fs::set_permissions(&holder, std::fs::Permissions::from_mode(0o555)).unwrap();
    let moved = copy_across(&from, &to);
    std::fs::set_permissions(&holder, std::fs::Permissions::from_mode(0o755)).unwrap();
    let note = moved.expect("a whole copy is not a failure");
    assert!(note.is_some_and(|n| n.contains("could not be removed")));
    assert_eq!(std::fs::read(to.join("a/b/two.bin")).unwrap(), b"two");
    assert_eq!(listed(&downloads), ["Show"]);

    // And the ordinary case: moved, nothing left behind. (The failed
    // removal above emptied the source but could not remove it.)
    std::fs::write(from.join("one.bin"), b"one").unwrap();
    let again = downloads.join("Again");
    std::fs::create_dir_all(&again).unwrap();
    assert_eq!(copy_across(&from, &again).unwrap(), None);
    assert!(!from.exists());
    assert_eq!(std::fs::read(again.join("one.bin")).unwrap(), b"one");
}

#[test]
fn magnets_and_torrent_files_are_for_the_torrent_engine() {
    assert!(is_torrent("magnet:?xt=urn:btih:0123456789abcdef"));
    assert!(is_torrent("file:///home/a/Some%20Thing.torrent"));
    assert!(is_torrent("  magnet:?xt=urn:btih:x  ")); // pasted with spaces
    assert!(is_torrent("https://example.com/ubuntu.torrent"));
    // A query string is where a signed link keeps its token; the path is
    // still what says .torrent.
    assert!(is_torrent("https://example.com/get.torrent?token=abc"));
    // A scheme and an extension in capitals are the same thing.
    assert!(is_torrent("Magnet:?xt=urn:btih:0123456789abcdef"));
    assert!(is_torrent("https://example.com/X.TORRENT"));
}

#[test]
fn only_a_socks5_proxy_can_carry_a_swarm() {
    assert!(matches!(swarm_proxy(None), Ok(None)));
    let Ok(Some(socks)) = swarm_proxy(Some(concat!("socks5://u:", "p", "@h:1080"))) else {
        panic!("a SOCKS5 proxy was not handed to the swarm");
    };
    assert_eq!(socks.as_str(), concat!("socks5://u:", "p", "@h:1080"));
    // What dm's settings hand out, in the spelling the engine reads.
    let Ok(Some(remote)) = swarm_proxy(Some(concat!("socks5h://u:", "p", "@h:1080"))) else {
        panic!("a SOCKS5 proxy that resolves names was not handed to the swarm");
    };
    assert_eq!(remote.as_str(), concat!("socks5://u:", "p", "@h:1080"));
    // An HTTP proxy cannot carry a peer connection, and going around it
    // is the one thing a configured proxy must never see.
    let Err(refused) = swarm_proxy(Some("http://h:8080")) else {
        panic!("an HTTP proxy was accepted for a swarm");
    };
    assert!(refused.to_string().contains("SOCKS5"), "{refused}");
}

#[tokio::test]
async fn a_torrents_own_name_cannot_lead_out_of_the_download_folder() {
    assert_eq!(folder_name("../.config/autostart"), "autostart");
    assert_eq!(folder_name(".."), "torrent");
    assert_eq!(folder_name(""), "torrent");
    assert_eq!(folder_name("Album (2024)"), "Album (2024)");
    let dir = tempfile::tempdir().unwrap();
    let downloads = dir.path().join("Downloads");
    tokio::fs::create_dir_all(&downloads).await.unwrap();
    let claimed = crate::download::claim_dir(&downloads, &folder_name("../.config/autostart"))
        .await
        .unwrap();
    assert_eq!(claimed.parent(), Some(downloads.as_path()));
}

#[test]
fn ordinary_files_are_not() {
    assert!(!is_torrent("https://example.com/ubuntu.iso"));
    assert!(!is_torrent("https://example.com/video.mp4"));
    // The word appears, but not as the extension: this is a page, not a
    // metainfo file, and fetching it as a torrent would be wrong.
    assert!(!is_torrent("https://example.com/torrent/list"));
    assert!(!is_torrent("https://example.com/a.torrent.html"));
}
