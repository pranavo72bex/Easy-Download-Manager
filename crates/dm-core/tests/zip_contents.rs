// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What is inside a ZIP, learned from its end with range requests: the names
//! and sizes, without the archive being fetched.

use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use dm_core::{client, zip_contents, zip_take_one};

mod common;

fn archive(files: usize, each: usize) -> Vec<u8> {
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.add_directory("pack/", stored).unwrap();
        for n in 0..files {
            zip.start_file(format!("pack/file-{n:03}.bin"), stored)
                .unwrap();
            zip.write_all(&vec![n as u8; each]).unwrap();
        }
        zip.set_comment("made for a test").unwrap();
        zip.finish().unwrap();
    }
    buffer.into_inner()
}

#[derive(Clone)]
struct Site {
    body: Arc<Vec<u8>>,
    ranges: bool,
    /// How many bytes of body the server has sent: a listing must not cost the
    /// archive.
    sent: Arc<AtomicU64>,
}

async fn answer(State(site): State<Site>, headers: HeaderMap) -> Response {
    let total = site.body.len();
    let range = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.split_once('-'))
        .map(|(a, b)| {
            (
                a.parse::<usize>().unwrap_or(0),
                b.parse::<usize>().unwrap_or(total - 1).min(total - 1),
            )
        });
    match range {
        Some((a, b)) if site.ranges => {
            site.sent.fetch_add((b - a + 1) as u64, Ordering::SeqCst);
            Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header("accept-ranges", "bytes")
                .header("content-range", format!("bytes {a}-{b}/{total}"))
                .header("content-length", b - a + 1)
                .header("etag", "\"z\"")
                .body(Body::from(site.body[a..=b].to_vec()))
                .unwrap()
        }
        _ => {
            site.sent.fetch_add(total as u64, Ordering::SeqCst);
            Response::builder()
                .status(StatusCode::OK)
                .header("content-length", total)
                .body(Body::from(site.body.to_vec()))
                .unwrap()
        }
    }
}

async fn serve(body: Vec<u8>, ranges: bool) -> (String, Arc<AtomicU64>) {
    let sent = Arc::new(AtomicU64::new(0));
    let site = Site {
        body: Arc::new(body),
        ranges,
        sent: sent.clone(),
    };
    let app = axum::Router::new().fallback(answer).with_state(site);
    let addr = common::serve(app).await;
    (format!("http://{addr}/pack.zip"), sent)
}

#[tokio::test]
async fn the_files_of_a_zip_are_listed_from_its_end_without_fetching_it() {
    // About 3 MB of archive: the listing must cost a small part of it.
    let body = archive(30, 100_000);
    let total = body.len() as u64;
    let (url, sent) = serve(body, true).await;

    let entries = zip_contents(&client(), &url, None, None)
        .await
        .expect("a listing");
    assert_eq!(entries.len(), 31);
    assert!(entries[0].is_folder() && entries[0].name == "pack/");
    assert_eq!(entries[1].name, "pack/file-000.bin");
    assert_eq!(entries[1].size, 100_000);
    assert_eq!(entries[30].name, "pack/file-029.bin");
    assert!(
        sent.load(Ordering::SeqCst) < total / 10,
        "{} of {total} bytes were fetched for a list",
        sent.load(Ordering::SeqCst)
    );
}

#[tokio::test]
async fn a_server_without_ranges_gives_no_listing() {
    let (url, _) = serve(archive(3, 1000), false).await;
    assert!(zip_contents(&client(), &url, None, None).await.is_none());
}

#[tokio::test]
async fn something_that_is_not_a_zip_gives_no_listing() {
    let (url, _) = serve(vec![b'x'; 200_000], true).await;
    assert!(zip_contents(&client(), &url, None, None).await.is_none());
}

/// A ZIP64 archive written by hand: one entry larger than 4 GB whose sizes are
/// in its ZIP64 extra field (after a field of another kind), and an end record
/// that sends the reader to the ZIP64 one. `comment` bytes of comment end it:
/// at its longest the directory lies outside the end the first request reads.
fn zip64(comment: usize) -> Vec<u8> {
    let mut out = b"local data".to_vec();
    let directory_at = out.len() as u64;
    let name = b"disk.img";
    let mut extra = Vec::new();
    extra.extend_from_slice(&0x5455u16.to_le_bytes());
    extra.extend_from_slice(&5u16.to_le_bytes());
    extra.extend_from_slice(&[1, 0, 0, 0, 0]);
    extra.extend_from_slice(&1u16.to_le_bytes());
    extra.extend_from_slice(&16u16.to_le_bytes());
    extra.extend_from_slice(&5_000_000_000u64.to_le_bytes());
    extra.extend_from_slice(&4_900_000_000u64.to_le_bytes());
    out.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
    out.extend_from_slice(&[45, 0, 45, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out.extend_from_slice(&u32::MAX.to_le_bytes()); // packed
    out.extend_from_slice(&u32::MAX.to_le_bytes()); // size
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&(extra.len() as u16).to_le_bytes());
    out.extend_from_slice(&[0u8; 14]); // comment length to local offset
    out.extend_from_slice(name);
    out.extend_from_slice(&extra);
    let directory_size = out.len() as u64 - directory_at;
    let record_at = out.len() as u64;
    out.extend_from_slice(&0x0606_4b50u32.to_le_bytes());
    out.extend_from_slice(&44u64.to_le_bytes());
    out.extend_from_slice(&[45, 0, 45, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out.extend_from_slice(&1u64.to_le_bytes());
    out.extend_from_slice(&1u64.to_le_bytes());
    out.extend_from_slice(&directory_size.to_le_bytes());
    out.extend_from_slice(&directory_at.to_le_bytes());
    out.extend_from_slice(&0x0706_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&record_at.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF]);
    out.extend_from_slice(&u32::MAX.to_le_bytes());
    out.extend_from_slice(&u32::MAX.to_le_bytes());
    out.extend_from_slice(&(comment as u16).to_le_bytes());
    out.extend(std::iter::repeat_n(b'c', comment));
    out
}

/// What a listing says of each file that a person sees: its name and sizes.
fn seen(entries: Option<Vec<dm_core::ZipEntry>>) -> Option<Vec<(String, u64, u64)>> {
    entries.map(|all| {
        all.into_iter()
            .map(|e| (e.name, e.size, e.packed))
            .collect()
    })
}

fn the_large_entry() -> Option<Vec<(String, u64, u64)>> {
    Some(vec![("disk.img".into(), 5_000_000_000, 4_900_000_000)])
}

#[tokio::test]
async fn a_zip64_archive_is_listed_with_its_large_sizes_over_the_web_and_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    // Short: everything in the end the first request reads. Long: the record and the
    // directory each fetched on their own.
    for comment in [0, 65_535] {
        let body = zip64(comment);
        let path = dir.path().join(format!("big-{comment}.zip"));
        std::fs::write(&path, &body).unwrap();
        assert_eq!(
            seen(dm_core::zip_contents_of_file(&path)),
            the_large_entry(),
            "on disk, comment {comment}"
        );
        let (url, _) = serve(body, true).await;
        assert_eq!(
            seen(zip_contents(&client(), &url, None, None).await),
            the_large_entry(),
            "over the web, comment {comment}"
        );
    }
}

#[tokio::test]
async fn an_empty_zip_lists_nothing_and_a_tiny_or_lying_one_gives_no_listing() {
    let dir = tempfile::tempdir().unwrap();
    // An end record and nothing else: an archive of no entries.
    let mut none = Vec::new();
    none.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    none.extend_from_slice(&[0u8; 18]);
    let path = dir.path().join("none.zip");
    std::fs::write(&path, &none).unwrap();
    assert_eq!(dm_core::zip_contents_of_file(&path), Some(Vec::new()));
    let (url, _) = serve(none, true).await;
    assert_eq!(
        zip_contents(&client(), &url, None, None).await,
        Some(Vec::new())
    );

    // Shorter than an end record.
    let tiny = dir.path().join("tiny.zip");
    std::fs::write(&tiny, b"PK").unwrap();
    assert_eq!(dm_core::zip_contents_of_file(&tiny), None);
    let (url, _) = serve(b"PK\x05\x06".to_vec(), true).await;
    assert_eq!(zip_contents(&client(), &url, None, None).await, None);

    // An end record whose directory would run past the end of the file.
    let mut lying = vec![0u8; 100];
    lying.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    lying.extend_from_slice(&[0, 0, 0, 0, 1, 0, 1, 0]);
    lying.extend_from_slice(&1000u32.to_le_bytes());
    lying.extend_from_slice(&50u32.to_le_bytes());
    lying.extend_from_slice(&0u16.to_le_bytes());
    let path = dir.path().join("lying.zip");
    std::fs::write(&path, &lying).unwrap();
    assert_eq!(dm_core::zip_contents_of_file(&path), None);
    let (url, _) = serve(lying, true).await;
    assert_eq!(zip_contents(&client(), &url, None, None).await, None);
}

/// An archive with a stored file, a deflated one and a folder, each of
/// `each` bytes of its own pattern, after `padding` files of filler so the
/// archive is much larger than any one of them.
fn mixed(padding: usize, each: usize) -> Vec<u8> {
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let deflated = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for n in 0..padding {
            zip.start_file(format!("filler/{n}.bin"), stored).unwrap();
            zip.write_all(&vec![0xAA; 100_000]).unwrap();
        }
        zip.add_directory("docs/", stored).unwrap();
        zip.start_file("docs/plain.txt", stored).unwrap();
        zip.write_all(&pattern(each, 7)).unwrap();
        zip.start_file("docs/packed.txt", deflated).unwrap();
        zip.write_all(&pattern(each, 13)).unwrap();
        zip.finish().unwrap();
    }
    buffer.into_inner()
}

fn pattern(len: usize, step: usize) -> Vec<u8> {
    (0..len).map(|i| b"abcdefghij\n"[(i * step) % 11]).collect()
}

async fn listed(url: &str, name: &str) -> dm_core::ZipEntry {
    zip_contents(&client(), url, None, None)
        .await
        .expect("a listing")
        .into_iter()
        .find(|e| e.name == name)
        .expect("the file is listed")
}

#[tokio::test]
async fn one_file_is_taken_out_of_a_remote_zip_without_fetching_the_archive() {
    let body = mixed(30, 50_000);
    let total = body.len() as u64;
    let (url, sent) = serve(body, true).await;
    let dir = tempfile::tempdir().unwrap();

    for (name, step) in [("docs/plain.txt", 7), ("docs/packed.txt", 13)] {
        let entry = listed(&url, name).await;
        let saved = zip_take_one(&client(), &url, None, None, &entry, dir.path())
            .await
            .expect("taken out");
        assert_eq!(saved.parent(), Some(dir.path()));
        assert_eq!(
            std::fs::read(&saved).unwrap(),
            pattern(50_000, step),
            "{name}"
        );
    }
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        2,
        "nothing staged is left"
    );
    // Two listings and two files of 50 KB: a small part of a 3 MB archive.
    assert!(
        sent.load(Ordering::SeqCst) < total / 10,
        "{} of {total} bytes were fetched",
        sent.load(Ordering::SeqCst)
    );

    // Taken again, it is saved beside the first under a free name.
    let entry = listed(&url, "docs/plain.txt").await;
    let again = zip_take_one(&client(), &url, None, None, &entry, dir.path())
        .await
        .unwrap();
    assert_eq!(again.file_name().unwrap(), "plain (1).txt");
}

#[tokio::test]
async fn a_folder_a_damaged_file_or_a_server_without_ranges_takes_nothing_out() {
    let body = mixed(0, 20_000);
    let (url, _) = serve(body.clone(), true).await;
    let dir = tempfile::tempdir().unwrap();

    let folder = listed(&url, "docs/").await;
    assert!(
        zip_take_one(&client(), &url, None, None, &folder, dir.path())
            .await
            .unwrap_err()
            .contains("folder")
    );

    // The archive's bytes of the stored file changed under its listed CRC.
    let entry = listed(&url, "docs/plain.txt").await;
    let mut damaged = body.clone();
    let at = usize::try_from(entry.at).unwrap() + 30 + "docs/plain.txt".len() + 1000;
    damaged[at] ^= 0xFF;
    let (bad_url, _) = serve(damaged, true).await;
    let why = zip_take_one(&client(), &bad_url, None, None, &entry, dir.path())
        .await
        .unwrap_err();
    assert!(why.contains("CRC"), "{why}");

    // A server that answers every request with the whole archive.
    let (whole_url, _) = serve(body, false).await;
    assert!(
        zip_take_one(&client(), &whole_url, None, None, &entry, dir.path())
            .await
            .is_err()
    );

    // A file listed as larger than may be held is the archive's to download.
    let huge = dm_core::ZipEntry {
        packed: dm_core::ZIP_MOST_TAKEN + 1,
        ..entry
    };
    assert!(zip_take_one(&client(), &url, None, None, &huge, dir.path())
        .await
        .unwrap_err()
        .contains("too large"));
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        0,
        "nothing was saved"
    );
}
