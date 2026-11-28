// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Secrets sealed with a passphrase a person gives, rather than with a key a store keeps: how a
//! backup of EDM carries its sign-ins (gaps row W16).
//!
//! The key is made from the passphrase and a random salt by PBKDF2 over HMAC-SHA256 (RFC 8018),
//! written here over the SHA-256 EDM already builds, as the archives' PBKDF2 over HMAC-SHA1 is
//! (`extract/crypt.rs`); no crate is added for it. The text is sealed with ChaCha20-Poly1305, as
//! the queue's file of secrets is (`secretstore.rs`), which also proves the passphrase: a wrong
//! one, or a sealed text changed since, opens nothing. What a sealed text needs besides the
//! passphrase (the rounds, the salt, the nonce) is written beside it, so a later EDM can raise the
//! rounds and still open what an earlier one sealed.

use base64::Engine as _;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use sha2::{Digest, Sha256};

/// The rounds of PBKDF2 a passphrase is stretched with: the count OWASP gives for
/// PBKDF2-HMAC-SHA256, so guessing at a passphrase from a copy of the file is slow.
pub const ROUNDS: u32 = 600_000;

/// The most rounds a sealed text may ask for: a file that asked for billions would hold the
/// work that opens it for hours.
pub const MOST_ROUNDS: u32 = 10_000_000;

/// The cipher, by name, as the sealed text says it.
const ALG: &str = "chacha20poly1305";
/// How the key is made from the passphrase, by name.
const KDF: &str = "pbkdf2-hmac-sha256";
/// Authenticated with the text, so a sealed text of another kind is not opened as this one.
const AAD: &[u8] = b"dm passphrase seal 1";
/// SHA-256's block, which HMAC pads its key to.
const BLOCK: usize = 64;

/// A text sealed with a passphrase, with what opening it needs besides: written as it is into
/// a file (a backup's `sign_ins`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Sealed {
    /// The cipher: `chacha20poly1305`.
    pub alg: String,
    /// How the key was made from the passphrase: `pbkdf2-hmac-sha256`.
    pub kdf: String,
    /// The rounds of PBKDF2.
    pub rounds: u32,
    /// The salt, in base64.
    pub salt: String,
    /// The cipher's nonce, in base64.
    pub nonce: String,
    /// The sealed text with its tag, in base64.
    pub ct: String,
}

/// Why a sealed text was not opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotOpened {
    /// The passphrase is not the one it was sealed with, or the sealed text was changed since.
    Wrong,
    /// It was sealed in a way this EDM does not know (a newer EDM's), or it cannot be read: why,
    /// in English.
    Unreadable(String),
}

impl std::fmt::Display for NotOpened {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotOpened::Wrong => f.write_str(
                "the passphrase is not the one they were sealed with, or they were changed since",
            ),
            NotOpened::Unreadable(why) => f.write_str(why),
        }
    }
}

/// `plain` sealed with `passphrase`, its key made with `rounds` rounds of PBKDF2 (held to 1 to
/// [`MOST_ROUNDS`]; [`ROUNDS`] is the one to use). The error says why, where the system gave no
/// random numbers.
pub fn seal(passphrase: &str, plain: &[u8], rounds: u32) -> Result<Sealed, String> {
    let rounds = rounds.clamp(1, MOST_ROUNDS);
    let salt = super::random::<16>()?;
    let nonce = super::random::<12>()?;
    let key = key_of(passphrase, &salt, rounds);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    let sealed = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plain,
                aad: AAD,
            },
        )
        .map_err(|_| "the secrets could not be sealed".to_string())?;
    let b64 = base64::engine::general_purpose::STANDARD;
    Ok(Sealed {
        alg: ALG.to_string(),
        kdf: KDF.to_string(),
        rounds,
        salt: b64.encode(salt),
        nonce: b64.encode(nonce),
        ct: b64.encode(sealed),
    })
}

/// What `sealed` holds, opened with `passphrase`.
pub fn open(passphrase: &str, sealed: &Sealed) -> Result<Vec<u8>, NotOpened> {
    if sealed.alg != ALG || sealed.kdf != KDF {
        return Err(NotOpened::Unreadable(
            "they were sealed by a newer EDM, in a way this one cannot open".to_string(),
        ));
    }
    if sealed.rounds == 0 || sealed.rounds > MOST_ROUNDS {
        return Err(NotOpened::Unreadable(format!(
            "they ask for {} rounds to open, which EDM does not take",
            sealed.rounds
        )));
    }
    let unreadable = || NotOpened::Unreadable("the sealed text is not readable".to_string());
    let b64 = base64::engine::general_purpose::STANDARD;
    let salt = b64.decode(&sealed.salt).map_err(|_| unreadable())?;
    let nonce = b64.decode(&sealed.nonce).map_err(|_| unreadable())?;
    let text = b64.decode(&sealed.ct).map_err(|_| unreadable())?;
    if nonce.len() != 12 {
        return Err(unreadable());
    }
    let key = key_of(passphrase, &salt, sealed.rounds);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &text,
                aad: AAD,
            },
        )
        .map_err(|_| NotOpened::Wrong)
}

/// The 32-byte key `passphrase` and `salt` make in `rounds` rounds.
fn key_of(passphrase: &str, salt: &[u8], rounds: u32) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2_sha256(passphrase.as_bytes(), salt, rounds, &mut key);
    key
}

/// An HMAC-SHA256 being computed (RFC 2104), its key's two padded blocks taken in once.
#[derive(Clone)]
struct HmacSha256 {
    inner: Sha256,
    outer: Sha256,
}

impl HmacSha256 {
    fn new(key: &[u8]) -> HmacSha256 {
        let mut block = [0u8; BLOCK];
        if key.len() > BLOCK {
            for (b, k) in block.iter_mut().zip(Sha256::digest(key).iter()) {
                *b = *k;
            }
        } else {
            for (b, k) in block.iter_mut().zip(key) {
                *b = *k;
            }
        }
        let (mut inner, mut outer) = (Sha256::new(), Sha256::new());
        inner.update(block.map(|b| b ^ 0x36));
        outer.update(block.map(|b| b ^ 0x5C));
        HmacSha256 { inner, outer }
    }

    fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    fn finish(self) -> [u8; 32] {
        let HmacSha256 { inner, mut outer } = self;
        outer.update(inner.finalize());
        outer.finalize().into()
    }
}

/// PBKDF2 with HMAC-SHA256 (RFC 8018), `out.len()` bytes of key.
fn pbkdf2_sha256(password: &[u8], salt: &[u8], rounds: u32, out: &mut [u8]) {
    let keyed = HmacSha256::new(password);
    for (i, chunk) in out.chunks_mut(32).enumerate() {
        let mut mac = keyed.clone();
        mac.update(salt);
        mac.update(&(i as u32 + 1).to_be_bytes());
        let mut u = mac.finish();
        let mut t = u;
        for _ in 1..rounds {
            let mut mac = keyed.clone();
            mac.update(&u);
            u = mac.finish();
            for (a, b) in t.iter_mut().zip(u) {
                *a ^= b;
            }
        }
        for (o, b) in chunk.iter_mut().zip(t) {
            *o = b;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hmac(key: &[u8], data: &[u8]) -> String {
        let mut mac = HmacSha256::new(key);
        mac.update(data);
        crate::hex::encode(mac.finish())
    }

    fn pbkdf2(password: &str, salt: &str, rounds: u32, len: usize) -> String {
        let mut out = vec![0u8; len];
        pbkdf2_sha256(password.as_bytes(), salt.as_bytes(), rounds, &mut out);
        crate::hex::encode(out)
    }

    /// RFC 4231's cases 2 (a short key) and 6 (a key longer than the block).
    #[test]
    fn hmac_sha256_gives_the_rfcs_answers() {
        assert_eq!(
            hmac(b"Jefe", b"what do ya want for nothing?"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert_eq!(
            hmac(
                &[0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            ),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    /// The published PBKDF2-HMAC-SHA256 answers: one and two rounds, and RFC 7914's 64 bytes
    /// (two blocks of output).
    #[test]
    fn pbkdf2_gives_the_published_answers() {
        assert_eq!(
            pbkdf2("password", "salt", 1, 32),
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
        );
        assert_eq!(
            pbkdf2("password", "salt", 2, 32),
            "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43"
        );
        assert_eq!(
            pbkdf2("passwd", "salt", 1, 64),
            "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc\
             49ca9cccf179b645991664b39d77ef317c71b845b1e30bd509112041d3a19783"
        );
    }

    /// What is sealed opens with its passphrase, and with no other; a changed byte, a rounds
    /// count past the most, and a newer kind of seal are refused, not opened.
    #[test]
    fn a_sealed_text_opens_with_its_passphrase_alone() {
        let sealed = seal("correct horse", b"the secrets", 1_000).unwrap();
        assert_eq!((sealed.rounds, sealed.alg.as_str()), (1_000, ALG));
        assert!(!sealed.ct.contains("secrets"));
        assert_eq!(open("correct horse", &sealed).unwrap(), b"the secrets");
        assert_eq!(open("correct horsE", &sealed), Err(NotOpened::Wrong));
        assert_eq!(open("", &sealed), Err(NotOpened::Wrong));
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut text = b64.decode(&sealed.ct).unwrap();
        text[0] ^= 1;
        let changed = Sealed {
            ct: b64.encode(text),
            ..sealed.clone()
        };
        assert_eq!(open("correct horse", &changed), Err(NotOpened::Wrong));
        let greedy = Sealed {
            rounds: MOST_ROUNDS + 1,
            ..sealed.clone()
        };
        assert!(matches!(
            open("correct horse", &greedy),
            Err(NotOpened::Unreadable(_))
        ));
        let newer = Sealed {
            kdf: "argon2id".to_string(),
            ..sealed.clone()
        };
        assert!(matches!(
            open("correct horse", &newer),
            Err(NotOpened::Unreadable(_))
        ));
        // Each seal has a salt and a nonce of its own.
        let again = seal("correct horse", b"the secrets", 1_000).unwrap();
        assert_ne!((again.salt, again.ct), (sealed.salt, sealed.ct));
    }
}
