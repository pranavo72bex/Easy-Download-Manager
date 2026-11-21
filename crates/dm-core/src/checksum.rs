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

/// The digest a server's answer gives for the whole file it sends, where it gives one (gaps row
/// W19): `Repr-Digest` (`sha-256=:…:`), the older `Digest` (`SHA-256=…`) and `Content-MD5`, each
/// in base64. The strongest hash offered is taken, a `Repr-Digest` before the others at the same
/// strength; a hash dm does not know, or a value of the wrong length for its hash, is passed over.
///
/// `whole` says the answer's body is the whole file (a 200, or a 206 from its first byte to its
/// last): `Digest` and `Content-MD5` describe the body sent, so on a part of the file they are
/// not the file's, where `Repr-Digest` names the whole file however much of it was sent. None is
/// taken from an answer sent compressed: each names the compressed bytes, not the file dm saves.
pub(crate) fn from_headers(headers: &reqwest::header::HeaderMap, whole: bool) -> Option<Checksum> {
    let coded = headers
        .get_all(reqwest::header::CONTENT_ENCODING)
        .iter()
        .any(|v| {
            v.to_str()
                .map_or(true, |v| !v.trim().eq_ignore_ascii_case("identity"))
        });
    if coded {
        return None;
    }
    let all = |name: &str| -> String {
        headers
            .get_all(name)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .collect::<Vec<_>>()
            .join(",")
    };
    let mut offered: Vec<Checksum> = Vec::new();
    // RFC 9530: a dictionary of `name=:base64:`, any parameters after a `;`.
    for member in all("repr-digest").split(',') {
        let Some((name, value)) = member.split_once('=') else {
            continue;
        };
        let value = value.split(';').next().unwrap_or("").trim();
        if let Some(bytes) = value.strip_prefix(':').and_then(|v| v.strip_suffix(':')) {
            offered.extend(header_digest(name, bytes));
        }
    }
    if whole {
        // RFC 3230: `name=base64`, the name in any case; the base64 may end in `=`.
        for member in all("digest").split(',') {
            if let Some((name, value)) = member.split_once('=') {
                offered.extend(header_digest(name, value));
            }
        }
        offered.extend(header_digest("md5", &all("content-md5")));
    }
    let strength = |algorithm: Algorithm| match algorithm {
        Algorithm::Sha512 => 4,
        Algorithm::Sha256 => 3,
        Algorithm::Sha1 => 2,
        Algorithm::Md5 => 1,
        Algorithm::Crc32 => 0,
    };
    // The first of the strongest: `max_by_key` keeps the last of equals, so the list is turned.
    offered
        .into_iter()
        .rev()
        .max_by_key(|found| strength(found.algorithm))
}

/// One digest a header names, by the hash's registered name (`sha-256`, `sha-512`, `sha` for
/// SHA-1, `md5`) and its value in base64; `None` for a hash dm does not know or a value that is
/// not that hash's length.
fn header_digest(name: &str, value: &str) -> Option<Checksum> {
    use base64::Engine as _;
    let algorithm = match name.trim().to_ascii_lowercase().as_str() {
        "sha-512" => Algorithm::Sha512,
        "sha-256" => Algorithm::Sha256,
        "sha" => Algorithm::Sha1,
        "md5" => Algorithm::Md5,
        _ => return None,
    };
    let value = value.trim().trim_end_matches('=');
    let bytes = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(value)
        .ok()?;
    let expected = crate::hex::encode(bytes);
    (Algorithm::from_digest_length(&expected) == Some(algorithm)).then_some(Checksum {
        algorithm,
        expected,
    })
}

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
mod tests;
