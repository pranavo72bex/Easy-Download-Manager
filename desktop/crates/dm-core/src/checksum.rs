// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Checking that what arrived is what was published.
//!
//! Worth having precisely because everything else in the engine is about
//! trusting the transfer: ranges, retries and resumes all assume the bytes
//! that came back belong where they were put. A published digest is the only
//! evidence from outside that assumption.

// Reads outside input: an index out of range here is a crash a server or
// a file could cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
use std::path::Path;

use md5::Md5;
use sha2::{Digest, Sha256, Sha512};

use crate::control::Control;
use crate::Error;

/// A hash a download can be checked against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Algorithm {
    /// CRC-32, as a file host or an old release page gives one (8 hex digits): it tells a
    /// damaged file, not a forged one.
    Crc32,
    /// MD5 (128 bits).
    Md5,
    /// SHA-1 (160 bits), as many release pages still give one.
    Sha1,
    /// SHA-256.
    Sha256,
    /// SHA-512.
    Sha512,
}

impl Algorithm {
    /// The algorithm a digest of this length must be, where that is
    /// unambiguous. People paste a hash far more often than they name one.
    pub fn from_digest_length(hex: &str) -> Option<Algorithm> {
        match hex.trim().len() {
            8 => Some(Algorithm::Crc32),
            32 => Some(Algorithm::Md5),
            40 => Some(Algorithm::Sha1),
            64 => Some(Algorithm::Sha256),
            128 => Some(Algorithm::Sha512),
            _ => None,
        }
    }

    /// The algorithm's name as a person writes it (`SHA-256`).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Algorithm::Crc32 => "crc32",
            Algorithm::Md5 => "md5",
            Algorithm::Sha1 => "sha1",
            Algorithm::Sha256 => "sha256",
            Algorithm::Sha512 => "sha512",
        }
    }
}

/// A digest a download is expected to have.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Checksum {
    /// The hash that was used.
    pub algorithm: Algorithm,
    /// Lower-case hex, without any `sha256:` prefix.
    pub expected: String,
}

impl Checksum {
    /// Reads `sha256:abc…`, `SHA256=abc…`, or a bare digest whose length says
    /// what it is.
    ///
    /// Bare digests are accepted because that is overwhelmingly what a
    /// release page offers, and refusing them would mean asking people to
    /// name an algorithm they can already see the length of.
    pub fn parse(text: &str) -> Option<Checksum> {
        let text = text.trim();
        let (name, digest) = match text.split_once([':', '=']) {
            Some((name, digest)) => (Some(name.trim().to_ascii_lowercase()), digest.trim()),
            None => (None, text),
        };

        let digest = digest.trim().to_ascii_lowercase();
        if digest.is_empty() || !digest.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }

        // The length says what it must be; a name only has to agree with it,
        // or the check would pass on a digest that could never match.
        let by_length = Algorithm::from_digest_length(&digest)?;
        let algorithm = match name.as_deref() {
            Some("crc32") | Some("crc") => Algorithm::Crc32,
            Some("md5") => Algorithm::Md5,
            Some("sha1") | Some("sha-1") => Algorithm::Sha1,
            Some("sha256") | Some("sha-256") => Algorithm::Sha256,
            Some("sha512") | Some("sha-512") => Algorithm::Sha512,
            Some(_) => return None,
            None => by_length,
        };
        if algorithm != by_length {
            return None;
        }

        Some(Checksum {
            algorithm,
            expected: digest,
        })
    }
}

/// Hashes a file. Streamed, because these are the large files by definition.
pub async fn digest(path: &Path, algorithm: Algorithm) -> Result<String, Error> {
    digest_until(path, algorithm, &Control::new()).await
}

/// [`digest`], stopped at the next piece once the download `control` belongs to is cancelled
/// (gaps row 185): on the blocking pool with the computer's other heavy work, a few at a time
/// (`heavy`), not on the runtime's threads between reads, where a 4 GB file kept a worker busy
/// for every second it took and a Cancel waited for the whole file.
pub(crate) async fn digest_until(
    path: &Path,
    algorithm: Algorithm,
    control: &Control,
) -> Result<String, Error> {
    let (path, control) = (path.to_path_buf(), control.clone());
    crate::heavy::run(move || digest_blocking(&path, algorithm, &control)).await?
}

fn digest_blocking(path: &Path, algorithm: Algorithm, control: &Control) -> Result<String, Error> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut buffer = vec![0u8; 256 * 1024];

    // One expansion per algorithm: the three hashers are different types,
    // and boxing them to share one loop buys nothing here.
    macro_rules! run {
        ($hasher:expr) => {{
            let mut hasher = $hasher;
            loop {
                crate::heavy::stopped(control)?;
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hasher.update(buffer.get(..read).unwrap_or_default());
            }
            crate::hex::encode(hasher.finalize())
        }};
    }

    Ok(match algorithm {
        Algorithm::Crc32 => {
            let mut crc = crc32fast::Hasher::new();
            loop {
                crate::heavy::stopped(control)?;
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                crc.update(buffer.get(..read).unwrap_or_default());
            }
            format!("{:08x}", crc.finalize())
        }
        Algorithm::Md5 => run!(Md5::new()),
        Algorithm::Sha1 => run!(sha1::Sha1::new()),
        Algorithm::Sha256 => run!(Sha256::new()),
        Algorithm::Sha512 => run!(Sha512::new()),
    })
}

/// Checks a finished file against what was published, stopped once the download `control`
/// belongs to is cancelled ([`digest_until`]).
pub(crate) async fn verify(
    path: &Path,
    checksum: &Checksum,
    control: &Control,
) -> Result<(), Error> {
    let got = digest_until(path, checksum.algorithm, control).await?;
    if got.eq_ignore_ascii_case(&checksum.expected) {
        return Ok(());
    }
    Err(Error::Checksum {
        algorithm: checksum.algorithm.name(),
        expected: checksum.expected.clone(),
        got,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// A Cancel stops the digest of a 2 GiB file within a second, where the digest ran to the
    /// file's end (gaps row 185); a pause does not stop it. The file is sparse: nothing of it is
    /// on the disk.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_cancel_stops_the_digest_of_a_large_file_within_a_second() {
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
        let outcome = tokio::time::timeout(Duration::from_secs(1), digesting)
            .await
            .expect("the digest went on for a second after the cancel")
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
}
