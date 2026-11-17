// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// A file of one digest is read bare or naming the file; a list only where it names it
/// (gaps row F5).
#[test]
fn a_checksum_file_gives_the_digest_it_lists_for_this_file() {
    let sha = "ab".repeat(32);
    let names = ["dm.zip"];
    let want = Some(Checksum {
        algorithm: Algorithm::Sha256,
        expected: sha.clone(),
    });
    for text in [
        sha.clone(),
        format!("{sha}\r\n"),
        format!("{}  dm.zip\n", sha.to_ascii_uppercase()),
        format!("{sha} *dm.zip"),
        format!("{sha}  ./dm.zip"),
        format!("SHA256 (dm.zip) = {sha}"),
    ] {
        assert_eq!(
            listed_for(&text, Algorithm::Sha256, &names, true),
            want,
            "{text:?}"
        );
    }
    let list = format!(
        "# release\n{}  other.zip\n{sha}  dist/dm.zip\n{}  dm.zip.asc\n",
        "cd".repeat(32),
        "ef".repeat(32)
    );
    assert_eq!(listed_for(&list, Algorithm::Sha256, &names, false), want);
    // OpenSSL's form, its newer tag too, and a name with brackets of its own.
    for text in [
        format!("SHA256(dm.zip)= {sha}"),
        format!("SHA2-256(dm.zip)= {sha}"),
    ] {
        assert_eq!(
            listed_for(&text, Algorithm::Sha256, &names, false),
            want,
            "{text:?}"
        );
    }
    assert_eq!(
        listed_for(
            &format!("SHA256 (dm (1).zip) = {sha}"),
            Algorithm::Sha256,
            &["dm (1).zip"],
            false
        ),
        want
    );
    // A GNU name holding `=` is still a GNU line.
    assert_eq!(
        listed_for(
            &format!("{sha}  a=b.zip"),
            Algorithm::Sha256,
            &["a=b.zip"],
            false
        ),
        want
    );
    // The file named itself wins over the same name in another folder, wherever it is.
    let both = format!("{}  linux/dm.zip\n{sha}  dm.zip\n", "cd".repeat(32));
    assert_eq!(listed_for(&both, Algorithm::Sha256, &names, false), want);
    // Two folders' files of that name that differ: which is this one is not known, so
    // neither is taken (the wrong one would fail a good file).
    let two = format!("{}  linux/dm.zip\n{sha}  windows/dm.zip\n", "cd".repeat(32));
    assert_eq!(listed_for(&two, Algorithm::Sha256, &names, false), None);
    // A bare digest says nothing of which file a list means.
    assert_eq!(listed_for(&sha, Algorithm::Sha256, &names, false), None);
    // Another file's digest, a digest of the wrong length, and a page are not this one's.
    for text in [
        format!("{sha}  other.zip"),
        "ab".repeat(20),
        format!("MD5 (dm.zip) = {sha}"),
        "<!doctype html><title>Not found</title>".to_string(),
    ] {
        assert_eq!(
            listed_for(&text, Algorithm::Sha256, &names, true),
            None,
            "{text:?}"
        );
    }
    assert_eq!(
        listed_for(
            &format!("{}  dm.zip", "1".repeat(40)),
            Algorithm::Sha1,
            &names,
            true
        )
        .map(|c| c.algorithm),
        Some(Algorithm::Sha1)
    );
}

/// A 40-digit digest is SHA-1 and an 8-digit one CRC-32, each checked as such (gaps row F5).
#[tokio::test]
async fn sha1_and_crc32_are_known_by_their_length_and_checked() {
    assert_eq!(
        Checksum::parse(&"b".repeat(40)).unwrap().algorithm,
        Algorithm::Sha1
    );
    assert_eq!(
        Checksum::parse("CRC32:CBF43926").unwrap().algorithm,
        Algorithm::Crc32
    );
    assert_eq!(Checksum::parse(&format!("sha1:{}", "c".repeat(64))), None);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("f");
    std::fs::write(&file, b"123456789").unwrap();
    // The standard check values of "123456789".
    assert_eq!(digest(&file, Algorithm::Crc32).await.unwrap(), "cbf43926");
    assert_eq!(
        digest(&file, Algorithm::Sha1).await.unwrap(),
        "f7c3bc1d808e04732adf679965ccc34ca7ae3441"
    );
}

#[test]
fn a_bare_digest_is_recognised_by_its_length() {
    // What a release page actually gives you.
    let sha256 = "e".repeat(64);
    let parsed = Checksum::parse(&sha256).unwrap();
    assert_eq!(parsed.algorithm, Algorithm::Sha256);
    assert_eq!(parsed.expected, sha256);

    assert_eq!(
        Checksum::parse(&"a".repeat(32)).unwrap().algorithm,
        Algorithm::Md5
    );
    assert_eq!(
        Checksum::parse(&"a".repeat(128)).unwrap().algorithm,
        Algorithm::Sha512
    );
}

#[test]
fn a_named_digest_is_taken_at_its_word() {
    let parsed = Checksum::parse(&format!("sha256:{}", "A".repeat(64))).unwrap();
    assert_eq!(parsed.algorithm, Algorithm::Sha256);
    assert_eq!(parsed.expected, "a".repeat(64), "kept as lower case");

    assert_eq!(
        Checksum::parse(&format!("SHA-512={}", "b".repeat(128)))
            .unwrap()
            .algorithm,
        Algorithm::Sha512
    );
}

#[test]
fn a_name_that_disagrees_with_the_length_is_refused() {
    /* Otherwise "sha256:" in front of an md5 sum produces a check that
     * can never pass, reported as a corrupt download. */
    assert_eq!(Checksum::parse(&format!("sha256:{}", "a".repeat(32))), None);
}

#[test]
fn nonsense_is_not_a_checksum() {
    for text in ["", "   ", "not a hash", "sha999:abc", &"z".repeat(64)] {
        assert_eq!(Checksum::parse(text), None, "accepted {text:?}");
    }
}

/// A Cancel stops the digest of a 2 GiB file, where the digest ran to the file's end (gaps row
/// 185); a pause does not stop it. The file is sparse: nothing of it is on the disk. Stopped
/// is told by the answer, Cancelled where a whole digest would give its value; the bound in
/// time is only against a digest that never ends (a second failed under the load of the whole
/// workspace's run, as gaps row S19's did).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancel_stops_the_digest_of_a_large_file() {
    use std::time::{Duration, Instant};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.bin");
    std::fs::File::create(&path)
        .unwrap()
        .set_len(2 << 30)
        .unwrap();
    let control = Control::new();
    let digesting = {
        let (path, control) = (path.clone(), control.clone());
        tokio::spawn(async move { digest_until(&path, Algorithm::Sha256, &control).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    control.pause();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !digesting.is_finished(),
        "the digest ended under a pause, or before it could be cancelled"
    );
    control.cancel();
    let asked = Instant::now();
    let outcome = tokio::time::timeout(Duration::from_secs(30), digesting)
        .await
        .expect("the digest went on for 30 s after the cancel")
        .unwrap();
    assert!(
        matches!(outcome, Err(Error::Cancelled)),
        "{outcome:?} after {:?}",
        asked.elapsed()
    );
}

#[tokio::test]
async fn a_file_hashes_to_the_published_value() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.bin");
    tokio::fs::write(&path, b"abc").await.unwrap();

    // The published SHA-256 of "abc".
    let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    assert_eq!(digest(&path, Algorithm::Sha256).await.unwrap(), expected);
    assert_eq!(
        digest(&path, Algorithm::Md5).await.unwrap(),
        "900150983cd24fb0d6963f7d28e17f72"
    );

    verify(&path, &Checksum::parse(expected).unwrap(), &Control::new())
        .await
        .expect("a correct digest was rejected");
}

#[tokio::test]
async fn a_file_that_does_not_match_says_both_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.bin");
    tokio::fs::write(&path, b"abc").await.unwrap();

    let wrong = Checksum::parse(&"0".repeat(64)).unwrap();
    let err = verify(&path, &wrong, &Control::new()).await.unwrap_err();
    let text = err.to_string();
    assert!(
        text.contains("ba7816bf"),
        "the real digest is missing: {text}"
    );
    assert!(
        text.contains(&"0".repeat(64)),
        "the expected one is missing"
    );
}

#[tokio::test]
async fn hashing_reads_more_than_one_buffer() {
    // The streaming loop is the part worth exercising; a file that fits
    // in one read would never take the second pass.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.bin");
    tokio::fs::write(&path, vec![7u8; 1024 * 1024])
        .await
        .unwrap();

    let once = digest(&path, Algorithm::Sha256).await.unwrap();
    assert_eq!(once.len(), 64);
    assert_eq!(digest(&path, Algorithm::Sha256).await.unwrap(), once);
}
