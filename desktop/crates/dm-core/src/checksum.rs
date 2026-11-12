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

/// The endings a checksum file of one file is published under beside it (`dm.zip.sha256`), in
/// the order they are preferred: the strongest hash first (gaps row F5).
pub(crate) const BESIDE: [(&str, Algorithm); 4] = [
    ("sha256", Algorithm::Sha256),
    ("sha512", Algorithm::Sha512),
    ("sha1", Algorithm::Sha1),
    ("md5", Algorithm::Md5),
];

/// The lists of a whole folder's digests a release publishes beside its files, in the order
/// they are preferred.
pub(crate) const LISTS: [(&str, Algorithm); 2] = [
    ("SHA256SUMS", Algorithm::Sha256),
    ("SHA512SUMS", Algorithm::Sha512),
];

/// The digest a checksum file of `algorithm` publishes for a file called one of `names`.
///
/// Reads the forms the common tools write: `<digest>  name` and `<digest> *name` (the GNU
/// tools), `SHA256 (name) = <digest>` (the BSD ones) and `SHA256(name)= <digest>` (OpenSSL's). A
/// list (`SHA256SUMS`) must name the file; a file of one digest (`alone`, `dm.zip.sha256`) may
/// hold the bare digest, but one that names some other file is not taken for this one. A digest
/// of the wrong length for the algorithm is never taken, so a page or a note that happens to hold
/// hex is not a checksum.
///
/// A line naming the file itself (`dm.zip`, `./dm.zip`) is taken first. One naming it in another
/// folder (`dist/dm.zip`, how a list hashed in a build folder reads) is taken only where every
/// such line gives the same digest: a list of `linux/dm.zip` and `windows/dm.zip` does not say
/// which this one is, and the wrong one would fail a good file and remove it.
pub(crate) fn listed_for(
    text: &str,
    algorithm: Algorithm,
    names: &[&str],
    alone: bool,
) -> Option<Checksum> {
    let fits = |digest: &str| {
        digest.chars().all(|c| c.is_ascii_hexdigit())
            && Algorithm::from_digest_length(digest) == Some(algorithm)
    };
    // How a listed name names the file: as itself, in another folder, or not at all.
    let names_this = |listed: &str| -> Option<bool> {
        let listed = listed.trim();
        let listed = listed.strip_prefix("./").unwrap_or(listed);
        if names.iter().any(|name| !name.is_empty() && *name == listed) {
            return Some(true);
        }
        let base = listed.rsplit(['/', '\\']).next().unwrap_or(listed);
        names
            .iter()
            .any(|name| !name.is_empty() && *name == base)
            .then_some(false)
    };
    let found = |digest: &str| Checksum {
        algorithm,
        expected: digest.to_ascii_lowercase(),
    };
    // The digests lines give for the file in another folder, while no line names it itself.
    let mut elsewhere: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // BSD and OpenSSL: `SHA256 (name) = digest`, `SHA256(name)= digest`, the tag naming the
        // algorithm (`SHA2-256` in newer OpenSSL).
        let tagged = line.rsplit_once('=').and_then(|(head, digest)| {
            let (tag, name) = head.trim_end().strip_suffix(')')?.split_once('(')?;
            let tag = tag.trim().to_ascii_lowercase().replace('-', "");
            let tag = match tag.strip_prefix("sha2") {
                Some(bits @ ("256" | "512")) => format!("sha{bits}"),
                _ => tag,
            };
            (!tag.is_empty() && tag.chars().all(|c| c.is_ascii_alphanumeric()))
                .then(|| (tag, name, digest.trim()))
        });
        let (digest, named) = match tagged {
            Some((tag, name, digest)) => {
                if tag != algorithm.name() {
                    continue;
                }
                (digest, names_this(name))
            }
            // GNU: the digest, then the name after a space and, in binary mode, a `*`.
            None => match line.split_once(char::is_whitespace) {
                Some((digest, name)) => {
                    let name = name.trim_start().trim_start_matches('*');
                    if name.trim().is_empty() {
                        (digest, alone.then_some(true))
                    } else {
                        (digest, names_this(name))
                    }
                }
                None => (line, alone.then_some(true)),
            },
        };
        if !fits(digest) {
            continue;
        }
        match named {
            Some(true) => return Some(found(digest)),
            Some(false) => elsewhere.push(digest.to_ascii_lowercase()),
            None => {}
        }
    }
    let first = elsewhere.first()?;
    elsewhere
        .iter()
        .all(|digest| digest == first)
        .then(|| found(first))
}

#[cfg(test)]
mod tests {
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
}
