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

/// A header map of `(name, value)` pairs, a name given twice sent twice.
fn headers(pairs: &[(&str, &str)]) -> reqwest::header::HeaderMap {
    let mut map = reqwest::header::HeaderMap::new();
    for (name, value) in pairs {
        map.append(
            reqwest::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
            reqwest::header::HeaderValue::from_str(value).unwrap(),
        );
    }
    map
}

fn b64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// The SHA-256, SHA-512 and MD5 of `abc`, as bytes and as hex.
fn abc() -> ([Vec<u8>; 3], [String; 3]) {
    let sha256 = Sha256::digest(b"abc").to_vec();
    let sha512 = Sha512::digest(b"abc").to_vec();
    let md5 = Md5::digest(b"abc").to_vec();
    let hex = [
        crate::hex::encode(&sha256),
        crate::hex::encode(&sha512),
        crate::hex::encode(&md5),
    ];
    ([sha256, sha512, md5], hex)
}

/// Each of the three headers gives the file's digest in hex (gaps row W19).
#[test]
fn a_digest_header_of_each_kind_is_read() {
    let ([sha256, sha512, md5], [h256, h512, hmd5]) = abc();
    let want = |algorithm, expected: &str| {
        Some(Checksum {
            algorithm,
            expected: expected.to_owned(),
        })
    };
    let repr = format!("sha-256=:{}:", b64(&sha256));
    assert_eq!(
        from_headers(&headers(&[("Repr-Digest", &repr)]), true),
        want(Algorithm::Sha256, &h256)
    );
    let legacy = format!("SHA-512={}", b64(&sha512));
    assert_eq!(
        from_headers(&headers(&[("Digest", &legacy)]), true),
        want(Algorithm::Sha512, &h512)
    );
    assert_eq!(
        from_headers(&headers(&[("Content-MD5", &b64(&md5))]), true),
        want(Algorithm::Md5, &hmd5)
    );
    // Without its padding, with parameters, and in a second header line.
    let unpadded = format!("SHA-256={}", b64(&sha256).trim_end_matches('='));
    assert_eq!(
        from_headers(&headers(&[("Digest", &unpadded)]), true),
        want(Algorithm::Sha256, &h256)
    );
    let second = format!("sha-256=:{}:;x=1", b64(&sha256));
    assert_eq!(
        from_headers(
            &headers(&[("Repr-Digest", "unixsum=:AAAA:"), ("Repr-Digest", &second)]),
            true
        ),
        want(Algorithm::Sha256, &h256)
    );
}

/// The strongest hash offered is taken, a `Repr-Digest` before the older forms at one strength.
#[test]
fn the_strongest_digest_offered_is_taken() {
    let ([sha256, sha512, md5], [h256, h512, _]) = abc();
    let several = format!(
        "md5=:{}:, sha-512=:{}:, sha-256=:{}:",
        b64(&md5),
        b64(&sha512),
        b64(&sha256)
    );
    let found = from_headers(&headers(&[("Repr-Digest", &several)]), true).unwrap();
    assert_eq!((found.algorithm, found.expected), (Algorithm::Sha512, h512));

    let legacy = format!("MD5={}, SHA-256={}", b64(&md5), b64(&sha256));
    let found = from_headers(
        &headers(&[("Digest", &legacy), ("Content-MD5", &b64(&md5))]),
        true,
    )
    .unwrap();
    assert_eq!(
        (found.algorithm, &found.expected),
        (Algorithm::Sha256, &h256)
    );

    // At one strength the `Repr-Digest`'s is taken, whichever header came first.
    let other = b64(&Sha256::digest(b"abd"));
    let found = from_headers(
        &headers(&[
            ("Digest", &format!("SHA-256={other}")),
            ("Repr-Digest", &format!("sha-256=:{}:", b64(&sha256))),
        ]),
        true,
    )
    .unwrap();
    assert_eq!(found.expected, h256);
}

/// A hash dm does not know, a value of the wrong length, or one that is not base64 is passed
/// over; a part of the file has no digest but its `Repr-Digest`, and a compressed answer none.
#[test]
fn a_digest_that_is_not_the_files_is_not_taken() {
    let ([sha256, _, md5], [h256, _, _]) = abc();
    for (name, value) in [
        ("Repr-Digest", "unixsum=:AAAA:".to_owned()),
        ("Repr-Digest", format!("sha-256=:{}:", b64(&md5))),
        ("Repr-Digest", format!("sha-256={}", b64(&sha256))),
        ("Digest", format!("SHA-256={}", h256)),
        ("Digest", "SHA-256=not*base64".to_owned()),
        ("Digest", format!("CRC32C={}", b64(&[1, 2, 3, 4]))),
        ("Content-MD5", b64(&sha256)),
        ("Want-Repr-Digest", "sha-256=1".to_owned()),
    ] {
        assert_eq!(
            from_headers(&headers(&[(name, &value)]), true),
            None,
            "{name}: {value}"
        );
    }

    let legacy = [
        ("Digest", format!("SHA-256={}", b64(&sha256))),
        ("Content-MD5", b64(&md5)),
    ];
    for (name, value) in &legacy {
        assert_eq!(
            from_headers(&headers(&[(name, value)]), false),
            None,
            "{name}"
        );
    }
    let repr = format!("sha-256=:{}:", b64(&sha256));
    assert_eq!(
        from_headers(&headers(&[("Repr-Digest", &repr)]), false).map(|c| c.expected),
        Some(h256.clone())
    );
    assert_eq!(
        from_headers(
            &headers(&[("Repr-Digest", &repr), ("Content-Encoding", "identity")]),
            true
        )
        .map(|c| c.expected),
        Some(h256)
    );
    for coding in ["gzip", "br", "zstd"] {
        assert_eq!(
            from_headers(
                &headers(&[
                    ("Repr-Digest", &repr),
                    ("Digest", &legacy[0].1),
                    ("Content-Encoding", coding)
                ]),
                true
            ),
            None,
            "{coding}"
        );
    }
}
