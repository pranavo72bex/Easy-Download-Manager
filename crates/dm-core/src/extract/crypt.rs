// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The ciphers of an archive with a password (gaps row F10), from the formats' public
//! descriptions, over the AES, SHA-1 and SHA-256 crates dm already builds:
//!
//! * a ZIP's AES (the AE-1 and AE-2 extra field `0x9901`): a key of 128, 192 or 256 bits made
//!   from the password and the entry's salt by PBKDF2 over HMAC-SHA1 (1000 rounds), a two-byte
//!   check of the password before any byte is read, AES in counter mode (the counter
//!   little-endian, from 1), and the first ten bytes of an HMAC-SHA1 of the encrypted bytes
//!   after them, which proves both the password and the bytes;
//! * a 7z's AES (coder `06F10701`): a 256-bit key that is SHA-256 over 2^n rounds of salt,
//!   password (UTF-16LE) and round number, and AES in CBC mode.
//!
//! A ZIP's older PKWARE cipher is the zip crate's own (`by_index_decrypt`).

use std::io::{self, Read};

use aes::cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
use sha1::{Digest, Sha1};

/// SHA-1's block, which HMAC pads its key to.
const SHA1_BLOCK: usize = 64;

/// An HMAC-SHA1 being computed.
#[derive(Clone)]
pub(super) struct HmacSha1 {
    inner: Sha1,
    outer: Sha1,
}

impl HmacSha1 {
    pub(super) fn new(key: &[u8]) -> HmacSha1 {
        let mut block = [0u8; SHA1_BLOCK];
        if key.len() > SHA1_BLOCK {
            let digest = Sha1::digest(key);
            block
                .get_mut(..digest.len())
                .unwrap_or_default()
                .copy_from_slice(&digest);
        } else {
            block
                .get_mut(..key.len())
                .unwrap_or_default()
                .copy_from_slice(key);
        }
        let (mut inner, mut outer) = (Sha1::new(), Sha1::new());
        inner.update(block.map(|b| b ^ 0x36));
        outer.update(block.map(|b| b ^ 0x5C));
        HmacSha1 { inner, outer }
    }

    pub(super) fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    pub(super) fn finish(self) -> [u8; 20] {
        let HmacSha1 { inner, mut outer } = self;
        outer.update(inner.finalize());
        outer.finalize().into()
    }
}

/// PBKDF2 with HMAC-SHA1 (RFC 8018), `out.len()` bytes of key.
pub(super) fn pbkdf2_sha1(password: &[u8], salt: &[u8], rounds: u32, out: &mut [u8]) {
    let keyed = HmacSha1::new(password);
    for (i, chunk) in out.chunks_mut(20).enumerate() {
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
        let n = chunk.len();
        chunk.copy_from_slice(t.get(..n).unwrap_or_default());
    }
}

/// Whether two byte strings are equal, taking as long whatever they hold.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// AES of any of its three key sizes.
enum Aes {
    A128(aes::Aes128),
    A192(aes::Aes192),
    A256(aes::Aes256),
}

impl Aes {
    fn new(key: &[u8]) -> io::Result<Aes> {
        let bad = |_| io::Error::new(io::ErrorKind::InvalidData, "an AES key of no AES size");
        Ok(match key.len() {
            16 => Aes::A128(aes::Aes128::new_from_slice(key).map_err(bad)?),
            24 => Aes::A192(aes::Aes192::new_from_slice(key).map_err(bad)?),
            _ => Aes::A256(aes::Aes256::new_from_slice(key).map_err(bad)?),
        })
    }

    fn encrypt(&self, block: &mut [u8; 16]) {
        let mut b = aes::Block::from(*block);
        match self {
            Aes::A128(c) => c.encrypt_block(&mut b),
            Aes::A192(c) => c.encrypt_block(&mut b),
            Aes::A256(c) => c.encrypt_block(&mut b),
        }
        block.copy_from_slice(&b);
    }

    fn decrypt(&self, block: &mut [u8; 16]) {
        let mut b = aes::Block::from(*block);
        match self {
            Aes::A128(c) => c.decrypt_block(&mut b),
            Aes::A192(c) => c.decrypt_block(&mut b),
            Aes::A256(c) => c.decrypt_block(&mut b),
        }
        block.copy_from_slice(&b);
    }
}

/// A ZIP entry's AES settings, from its `0x9901` extra field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ZipAes {
    /// 1 (AE-1: the entry's CRC is kept and checked) or 2 (AE-2: it is not).
    pub(super) vendor: u16,
    /// 1, 2 or 3: AES-128, -192 or -256.
    pub(super) strength: u8,
    /// The compression method under the encryption: 0 stored, 8 Deflate.
    pub(super) method: u16,
}

/// The `0x9901` extra field's header id.
pub(super) const ZIP_AES_FIELD: u16 = 0x9901;
/// The bytes of the password check, and of the authentication code at the end.
pub(super) const ZIP_AES_CHECK: usize = 2;
pub(super) const ZIP_AES_MAC: usize = 10;

impl ZipAes {
    /// The extra field's seven bytes, or why they are no AES settings dm reads.
    pub(super) fn parse(data: &[u8]) -> Result<ZipAes, super::Error> {
        let &[v0, v1, b'A', b'E', strength, m0, m1] = data else {
            return Err(super::Error::Method("an AES field of no known form".into()));
        };
        let vendor = u16::from_le_bytes([v0, v1]);
        if !(1..=2).contains(&vendor) || !(1..=3).contains(&strength) {
            return Err(super::Error::Method(format!(
                "AES of a kind dm does not know (AE-{vendor}, strength {strength})"
            )));
        }
        Ok(ZipAes {
            vendor,
            strength,
            method: u16::from_le_bytes([m0, m1]),
        })
    }

    pub(super) fn key_len(self) -> usize {
        8 * (usize::from(self.strength) + 1)
    }

    pub(super) fn salt_len(self) -> usize {
        self.key_len() / 2
    }

    /// The bytes an entry's stored data has beyond its encrypted bytes.
    pub(super) fn overhead(self) -> u64 {
        (self.salt_len() + ZIP_AES_CHECK + ZIP_AES_MAC) as u64
    }
}

/// The rounds of PBKDF2 a ZIP's AES uses.
const ZIP_AES_ROUNDS: u32 = 1000;

/// A ZIP entry's encrypted bytes read as plain ones: the password checked first, the
/// authentication code at the end (an `InvalidData` error where it differs).
pub(super) struct ZipAesReader<R: Read> {
    inner: R,
    cipher: Aes,
    mac: Option<HmacSha1>,
    counter: u64,
    stream: [u8; 16],
    used: usize,
    left: u64,
}

/// Why a ZIP entry could not be opened with the password.
#[derive(Debug)]
pub(super) enum Opened {
    /// The password's check differs: it is the wrong one.
    WrongPassword,
    /// The bytes could not be read.
    Io(io::Error),
}

impl<R: Read> ZipAesReader<R> {
    /// Reads the salt and the password check from `inner` (the entry's `stored` bytes) and
    /// derives the keys.
    pub(super) fn open(
        mut inner: R,
        aes: ZipAes,
        stored: u64,
        password: &[u8],
    ) -> Result<ZipAesReader<R>, Opened> {
        let left = stored.checked_sub(aes.overhead()).ok_or_else(|| {
            Opened::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "an encrypted entry too short for its salt",
            ))
        })?;
        let mut salt = [0u8; 16];
        let salt = salt.get_mut(..aes.salt_len()).unwrap_or_default();
        inner.read_exact(salt).map_err(Opened::Io)?;
        let mut check = [0u8; ZIP_AES_CHECK];
        inner.read_exact(&mut check).map_err(Opened::Io)?;
        let n = aes.key_len();
        let mut keys = [0u8; 2 * 32 + ZIP_AES_CHECK];
        let keys = keys.get_mut(..2 * n + ZIP_AES_CHECK).unwrap_or_default();
        pbkdf2_sha1(password, salt, ZIP_AES_ROUNDS, keys);
        if !same(keys.get(2 * n..).unwrap_or_default(), &check) {
            return Err(Opened::WrongPassword);
        }
        Ok(ZipAesReader {
            inner,
            cipher: Aes::new(keys.get(..n).unwrap_or_default()).map_err(Opened::Io)?,
            mac: Some(HmacSha1::new(keys.get(n..2 * n).unwrap_or_default())),
            counter: 0,
            stream: [0; 16],
            used: 16,
            left,
        })
    }

    /// Reads to the end, so the authentication code is checked whatever a decompressor
    /// above left unread.
    pub(super) fn finish(&mut self) -> io::Result<()> {
        io::copy(self, &mut io::sink()).map(|_| ())
    }
}

impl<R: Read> Read for ZipAesReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.left == 0 {
            if let Some(mac) = self.mac.take() {
                let mut code = [0u8; ZIP_AES_MAC];
                self.inner.read_exact(&mut code)?;
                if !same(mac.finish().get(..ZIP_AES_MAC).unwrap_or_default(), &code) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "an encrypted entry does not add up (its authentication code differs)",
                    ));
                }
            }
            return Ok(0);
        }
        let want = (buf.len() as u64).min(self.left) as usize;
        let buf = buf.get_mut(..want).unwrap_or_default();
        let n = self.inner.read(buf)?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        let got = buf.get_mut(..n).unwrap_or_default();
        if let Some(mac) = self.mac.as_mut() {
            mac.update(got);
        }
        for b in got.iter_mut() {
            if self.used == 16 {
                self.counter = self.counter.wrapping_add(1);
                self.stream = [0; 16];
                self.stream
                    .get_mut(..8)
                    .unwrap_or_default()
                    .copy_from_slice(&self.counter.to_le_bytes());
                self.cipher.encrypt(&mut self.stream);
                self.used = 0;
            }
            *b ^= self.stream.get(self.used).copied().unwrap_or(0);
            self.used += 1;
        }
        self.left -= n as u64;
        Ok(n)
    }
}

/// A ZIP's AES applied to `plain`, as an archiver writes an entry's stored bytes: salt,
/// password check, the encrypted bytes and their authentication code. The tests write
/// archives with it; counter mode is its own inverse, so it is the reader's cipher run the
/// other way.
#[cfg(test)]
pub(super) fn zip_aes_seal(aes: ZipAes, password: &[u8], salt: &[u8], plain: &[u8]) -> Vec<u8> {
    let n = aes.key_len();
    let mut keys = vec![0u8; 2 * n + ZIP_AES_CHECK];
    pbkdf2_sha1(password, salt, ZIP_AES_ROUNDS, &mut keys);
    let cipher = Aes::new(&keys[..n]).unwrap_or_else(|_| unreachable!());
    let mut out = salt.to_vec();
    out.extend_from_slice(&keys[2 * n..]);
    let mut sealed = plain.to_vec();
    for (i, chunk) in sealed.chunks_mut(16).enumerate() {
        let mut stream = [0u8; 16];
        stream[..8].copy_from_slice(&(i as u64 + 1).to_le_bytes());
        cipher.encrypt(&mut stream);
        for (b, s) in chunk.iter_mut().zip(stream) {
            *b ^= s;
        }
    }
    let mut mac = HmacSha1::new(&keys[n..2 * n]);
    mac.update(&sealed);
    out.extend_from_slice(&sealed);
    out.extend_from_slice(&mac.finish()[..ZIP_AES_MAC]);
    out
}

/// The most rounds, as a power of two, a 7z key is made with: 7-Zip's own limit (2^24, a
/// second or two); the archives it writes use 2^19.
const SEVEN_MOST_CYCLES: u8 = 24;

/// A 7z AES coder's properties: how its key is made and its first block's IV.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SevenAes {
    pub(super) cycles: u8,
    pub(super) salt: Vec<u8>,
    pub(super) iv: [u8; 16],
}

impl SevenAes {
    /// The coder's property bytes: the rounds' power in the first's low six bits, and where
    /// either of its two high bits is set a second byte of the salt's and the IV's sizes.
    pub(super) fn parse(props: &[u8]) -> Result<SevenAes, super::Error> {
        let bad = || super::Error::Method("an AES coder with properties of no known form".into());
        let (&b0, rest) = props.split_first().ok_or_else(bad)?;
        let cycles = b0 & 0x3F;
        let (mut salt, mut iv) = (Vec::new(), [0u8; 16]);
        if b0 & 0xC0 == 0 {
            if !rest.is_empty() {
                return Err(bad());
            }
        } else {
            let (&b1, rest) = rest.split_first().ok_or_else(bad)?;
            let salt_len = usize::from((b0 >> 7) & 1) + usize::from(b1 >> 4);
            let iv_len = usize::from((b0 >> 6) & 1) + usize::from(b1 & 0x0F);
            if rest.len() != salt_len + iv_len {
                return Err(bad());
            }
            let (s, v) = rest.split_at(salt_len);
            salt = s.to_vec();
            iv.get_mut(..iv_len).unwrap_or_default().copy_from_slice(v);
        }
        if cycles > SEVEN_MOST_CYCLES && cycles != 0x3F {
            return Err(super::Error::Method(format!(
                "an AES key made with 2^{cycles} rounds, more than dm makes"
            )));
        }
        Ok(SevenAes { cycles, salt, iv })
    }

    /// The key: SHA-256 over 2^cycles rounds of salt, password and the round as eight
    /// little-endian bytes; or, for cycles 63, the salt and password themselves.
    pub(super) fn key(&self, password_utf16: &[u8]) -> [u8; 32] {
        let mut key = [0u8; 32];
        if self.cycles == 0x3F {
            for (k, b) in key.iter_mut().zip(self.salt.iter().chain(password_utf16)) {
                *k = *b;
            }
            return key;
        }
        let mut sha = sha2::Sha256::new();
        let mut round = Vec::with_capacity(self.salt.len() + password_utf16.len() + 8);
        for i in 0u64..(1u64 << self.cycles) {
            round.clear();
            round.extend_from_slice(&self.salt);
            round.extend_from_slice(password_utf16);
            round.extend_from_slice(&i.to_le_bytes());
            sha2::Digest::update(&mut sha, &round);
        }
        key.copy_from_slice(&sha2::Digest::finalize(sha));
        key
    }
}

/// A password as a 7z key is made from it: UTF-16, little-endian, with no end mark.
pub(super) fn utf16le(password: &str) -> Vec<u8> {
    password.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// A 7z folder's packed bytes decrypted (AES-256 in CBC mode): `size` bytes of them, the
/// zeros that filled the last block dropped.
pub(super) struct CbcReader {
    inner: Box<dyn Read>,
    cipher: Aes,
    last: [u8; 16],
    /// Decrypted bytes not yet handed on.
    ready: Vec<u8>,
    at: usize,
    left: u64,
}

impl CbcReader {
    pub(super) fn new(
        inner: Box<dyn Read>,
        key: &[u8; 32],
        iv: [u8; 16],
        size: u64,
    ) -> io::Result<CbcReader> {
        Ok(CbcReader {
            inner,
            cipher: Aes::new(key)?,
            last: iv,
            ready: Vec::new(),
            at: 0,
            left: size,
        })
    }

    /// Decrypts the next run of whole blocks into `ready`.
    fn fill(&mut self) -> io::Result<()> {
        let want = (self.left.div_ceil(16) * 16).min(64 * 1024) as usize;
        self.ready.resize(want, 0);
        let mut got = 0;
        while got < want {
            let n = self
                .inner
                .read(self.ready.get_mut(got..).unwrap_or_default())?;
            if n == 0 {
                break;
            }
            got += n;
        }
        if got == 0 || got % 16 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the encrypted bytes end early",
            ));
        }
        self.ready.truncate(got);
        for block in self.ready.as_chunks_mut::<16>().0 {
            let next = *block;
            self.cipher.decrypt(block);
            for (b, l) in block.iter_mut().zip(self.last) {
                *b ^= l;
            }
            self.last = next;
        }
        self.at = 0;
        Ok(())
    }
}

impl Read for CbcReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.left == 0 || buf.is_empty() {
            return Ok(0);
        }
        if self.at >= self.ready.len() {
            self.fill()?;
        }
        let have = self.ready.get(self.at..).unwrap_or_default();
        let n = have
            .len()
            .min(buf.len())
            .min(usize::try_from(self.left).unwrap_or(usize::MAX));
        buf.get_mut(..n)
            .unwrap_or_default()
            .copy_from_slice(have.get(..n).unwrap_or_default());
        self.at += n;
        self.left -= n as u64;
        Ok(n)
    }
}

/// `plain` encrypted as a 7z AES coder writes it, padded with zeros to whole blocks; for the
/// tests' hand-written archives.
#[cfg(test)]
pub(super) fn seven_seal(key: &[u8; 32], iv: [u8; 16], plain: &[u8]) -> Vec<u8> {
    let cipher = Aes::new(key).unwrap_or_else(|_| unreachable!());
    let mut out = plain.to_vec();
    out.resize(plain.len().div_ceil(16) * 16, 0);
    let mut last = iv;
    for block in out.as_chunks_mut::<16>().0 {
        for (b, l) in block.iter_mut().zip(last) {
            *b ^= l;
        }
        cipher.encrypt(block);
        last = *block;
    }
    out
}

#[cfg(test)]
mod tests;
