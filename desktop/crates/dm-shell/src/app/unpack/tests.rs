// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use std::time::{Duration, Instant};

use super::*;
use crate::app::backend::testing;
use crate::app::session::Session;

/// A ZIP of `files` (name, bytes), stored, written here byte by byte: dm-shell has no ZIP
/// writer of its own, and an entry that would leave its folder is one no writer makes.
fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let (mut out, mut central) = (Vec::new(), Vec::new());
    for (name, data) in files {
        let at = out.len() as u32;
        let (crc, size, length) = (crc32(data), data.len() as u32, name.len() as u16);
        // The local header, then the central directory's entry: a stored file dated
        // 1980-01-01, made on a system with no Unix mode.
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        for half in [20u16, 0, 0, 0, 0x21] {
            out.extend_from_slice(&half.to_le_bytes());
        }
        for word in [crc, size, size] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        for half in [20u16, 20, 0, 0, 0, 0x21] {
            central.extend_from_slice(&half.to_le_bytes());
        }
        for word in [crc, size, size] {
            central.extend_from_slice(&word.to_le_bytes());
        }
        for half in [length, 0, 0, 0, 0] {
            central.extend_from_slice(&half.to_le_bytes());
        }
        for word in [0u32, at] {
            central.extend_from_slice(&word.to_le_bytes());
        }
        central.extend_from_slice(name.as_bytes());
    }
    let (at, count) = (out.len() as u32, files.len() as u16);
    let central_size = central.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    for half in [0u16, 0, count, count] {
        out.extend_from_slice(&half.to_le_bytes());
    }
    for word in [central_size, at] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// The ZIP's CRC-32 of `data`.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A server on this computer that sends each of `files` whole, at its name.
fn served(
    runtime: &tokio::runtime::Runtime,
    files: Vec<(&'static str, Vec<u8>)>,
) -> std::net::SocketAddr {
    runtime.block_on(async move {
        let mut app = axum::Router::new();
        for (name, bytes) in files {
            let send = move || {
                let bytes = bytes.clone();
                async move { bytes }
            };
            app = app.route(&format!("/{name}"), axum::routing::get(send));
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    })
}

/// `url` added with `with` and read as a window reads the queue until a read tells of its
/// finish; then its row once it says what became of its archive (at once, for one not
/// unpacked).
fn finished_and_settled(backend: &Backend, url: &str, with: dm_core::AddOptions) -> Entry {
    let settings = backend.settings();
    let mut session = Session::default();
    session.read(backend, &settings);
    let id = backend.add_with(url, with).unwrap();
    let began = Instant::now();
    while !session
        .read(backend, &settings)
        .detected
        .finished
        .contains(&id)
    {
        assert!(
            began.elapsed() < Duration::from_secs(60),
            "not finished: {:?}",
            backend.row(id)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    loop {
        let row = backend.row(id).unwrap();
        if !row.said.is_empty() || !wanted(&row, &settings) {
            return row;
        }
        assert!(began.elapsed() < Duration::from_secs(120), "{row:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A row as the queue keeps it: finished, its file at `path`.
fn finished_at(path: &str) -> Entry {
    serde_json::from_value(serde_json::json!({
        "id": 1,
        "url": "http://example.com/f",
        "state": serde_json::to_value(State::Completed).unwrap(),
        "progress": {"downloaded": 10, "total": 10, "speed": 0.0, "eta": null},
        "filename": "f",
        "path": path,
    }))
    .unwrap()
}

#[test]
fn only_a_finished_archive_ticked_for_itself_or_its_kind_is_wanted() {
    let mut settings = Settings::default();
    let mut zip = finished_at("/files/pack.zip");
    assert!(!wanted(&zip, &settings), "nothing ticked");
    zip.unpack = true;
    assert!(wanted(&zip, &settings), "its own box");
    let mut tar = finished_at("/files/pack.tar.gz");
    settings.folders.unpack_kinds = vec![dm_core::Category::Archives];
    assert!(wanted(&tar, &settings), "its kind's");
    tar.state = State::Running;
    assert!(!wanted(&tar, &settings), "not finished");
    let mut film = finished_at("/files/film.mp4");
    film.unpack = true;
    settings.folders.unpack_kinds = dm_core::Category::ALL.to_vec();
    assert!(!wanted(&film, &settings), "no archive");
}

#[test]
fn what_an_extraction_comes_to_is_said_and_only_an_unpacked_archive_is_sent_away() {
    use dm_core::extract::{Error, Extracted};
    let archive = Path::new("pack.zip");
    let sent = std::cell::Cell::new(0);
    let send = |_: &Path| -> Result<(), String> {
        sent.set(sent.get() + 1);
        Ok(())
    };
    let refuse = |_: &Path| -> Result<(), String> { Err("the Trash is full".to_string()) };
    let unpacked = || -> Result<Extracted, Error> {
        Ok(Extracted {
            folder: PathBuf::from("pack"),
            files: 2,
            bytes: 14,
        })
    };
    assert_eq!(settle(unpacked(), archive, false, &send), Outcome::Kept);
    assert_eq!(sent.get(), 0, "kept");
    assert_eq!(settle(unpacked(), archive, true, &send), Outcome::Trashed);
    assert_eq!(sent.get(), 1);
    assert_eq!(
        settle(unpacked(), archive, true, &refuse),
        Outcome::NotTrashed
    );
    let locked = Err(Error::Encrypted("a.txt".into()));
    assert_eq!(settle(locked, archive, true, &send), Outcome::Password);
    let wrong = Err(Error::WrongPassword("a.txt".into()));
    assert_eq!(settle(wrong, archive, true, &send), Outcome::Password);
    let escapes = Err(Error::Escapes("../a.txt".into()));
    assert_eq!(settle(escapes, archive, true, &send), Outcome::Refused);
    assert_eq!(sent.get(), 1, "an archive not unpacked is kept");
    // Each is a note the row can say in every language.
    for outcome in [
        Outcome::Kept,
        Outcome::Trashed,
        Outcome::NotTrashed,
        Outcome::Password,
        Outcome::Refused,
    ] {
        assert!(dm_core::NOTES.contains(&outcome.note()), "{outcome:?}");
    }
}

#[test]
fn a_zip_ticked_for_itself_is_unpacked_into_a_folder_of_its_name_and_kept() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let backend = testing::backend(&runtime, dir.path());
    let pack = zip_of(&[
        ("docs/a.txt", b"hello, zip\n".as_slice()),
        ("b.txt", b"bee".as_slice()),
    ]);
    let addr = served(
        &runtime,
        vec![("pack.zip", pack.clone()), ("left.zip", pack)],
    );
    let ticked = dm_core::AddOptions {
        unpack: true,
        ..dm_core::AddOptions::default()
    };
    let row = finished_and_settled(&backend, &format!("http://{addr}/pack.zip"), ticked);
    assert_eq!(row.said, Outcome::Kept.note());
    let archive = Path::new(&row.path);
    assert!(archive.is_file(), "the archive was kept");
    let folder = archive.with_file_name("pack");
    assert_eq!(
        std::fs::read(folder.join("docs").join("a.txt")).unwrap(),
        b"hello, zip\n"
    );
    assert_eq!(std::fs::read(folder.join("b.txt")).unwrap(), b"bee");
    // Not ticked, and its kind not chosen: left as it is.
    let url = format!("http://{addr}/left.zip");
    let left = finished_and_settled(&backend, &url, dm_core::AddOptions::default());
    assert!(left.said.is_empty(), "{}", left.said);
    assert!(!Path::new(&left.path).with_file_name("left").exists());
}

#[test]
fn a_kind_chosen_in_options_is_unpacked_beside_what_is_there_and_its_archive_trashed() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut settings = Settings::default();
    settings.folders.unpack_kinds = vec![dm_core::Category::Archives];
    settings.folders.unpack_trash = true;
    let backend = testing::backend_with(&runtime, dir.path(), settings);
    // A folder of the archive's name is there already, with a file of the archive's in it.
    let sorted = backend.settings().folders.download_dir.join("Archives");
    let mine = sorted.join("pack").join("docs").join("a.txt");
    std::fs::create_dir_all(mine.parent().unwrap()).unwrap();
    std::fs::write(&mine, b"mine").unwrap();
    let pack = zip_of(&[("docs/a.txt", b"hello, zip\n".as_slice())]);
    let addr = served(&runtime, vec![("pack.zip", pack)]);
    let url = format!("http://{addr}/pack.zip");
    let row = finished_and_settled(&backend, &url, dm_core::AddOptions::default());
    assert_eq!(row.said, Outcome::Trashed.note());
    assert!(
        !Path::new(&row.path).exists(),
        "the archive is in the Trash"
    );
    let folder = Path::new(&row.path).with_file_name("pack (1)");
    assert_eq!(
        std::fs::read(folder.join("docs").join("a.txt")).unwrap(),
        b"hello, zip\n"
    );
    assert_eq!(
        std::fs::read(&mine).unwrap(),
        b"mine",
        "a file was replaced"
    );
}

#[test]
fn an_archive_with_an_entry_leaving_its_folder_is_not_unpacked_and_is_kept() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut settings = Settings::default();
    settings.folders.unpack_kinds = vec![dm_core::Category::Archives];
    settings.folders.unpack_trash = true;
    let backend = testing::backend_with(&runtime, dir.path(), settings);
    let bad = zip_of(&[
        ("fine.txt", b"fine".as_slice()),
        ("../evil.txt", b"evil".as_slice()),
    ]);
    let addr = served(&runtime, vec![("bad.zip", bad)]);
    let url = format!("http://{addr}/bad.zip");
    let row = finished_and_settled(&backend, &url, dm_core::AddOptions::default());
    assert_eq!(row.said, Outcome::Refused.note());
    let archive = Path::new(&row.path);
    assert!(archive.is_file(), "an archive not unpacked is kept");
    let beside = archive.parent().unwrap();
    assert!(!beside.join("bad").exists(), "its folder was left behind");
    assert!(!beside.join("evil.txt").exists());
    assert!(!beside.parent().unwrap().join("evil.txt").exists());
}
