// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A ZIP entry with a password (gaps row F10): which cipher locks it, and its bytes read
//! with the password. AES (AE-1, AE-2) is dm's own (`crypt.rs`), the zip crate built without
//! its AES; the older PKWARE cipher is the zip crate's. The password is its UTF-8 bytes, as
//! the AES description asks and as archivers of today write the older one.

use std::fs::File;
use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, Ordering};

use flate2::read::DeflateDecoder;
use zip::read::ZipFile;
use zip::ZipArchive;

use super::crypt::{Opened, ZipAes, ZipAesReader, ZIP_AES_FIELD};
use super::Error;

/// What locks an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Lock {
    /// Nothing.
    Open,
    /// PKWARE's own cipher, the oldest.
    Pkware,
    /// AES, with its settings.
    Aes(ZipAes),
}

impl Lock {
    pub(super) fn locked(self) -> bool {
        self != Lock::Open
    }
}

/// The lock of `entry`, read without a password; an AES entry dm cannot unpack once
/// decrypted is refused here, before anything is written.
pub(super) fn lock_of(entry: &ZipFile<'_, File>) -> Result<Lock, Error> {
    if !entry.encrypted() {
        return Ok(Lock::Open);
    }
    let field = entry.extra_data_fields().find_map(|f| match f {
        zip::ExtraField::Custom(c) if c.header_id == ZIP_AES_FIELD => Some(c.data.clone()),
        _ => None,
    });
    let Some(data) = field else {
        return Ok(Lock::Pkware);
    };
    let aes = ZipAes::parse(&data)?;
    match aes.method {
        0 | 8 => Ok(Lock::Aes(aes)),
        m => Err(Error::Method(format!("compression method {m} under AES"))),
    }
}

/// An AES entry's plain bytes: decrypted, inflated where it was deflated, and at the end its
/// authentication code and, for AE-1, its CRC checked.
struct AesEntry<'a> {
    body: Body<'a>,
    crc: Option<(u32, crc32fast::Hasher)>,
    done: bool,
}

enum Body<'a> {
    Stored(Box<ZipAesReader<ZipFile<'a, File>>>),
    Deflated(Box<DeflateDecoder<ZipAesReader<ZipFile<'a, File>>>>),
}

impl Read for AesEntry<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.done {
            return Ok(0);
        }
        let n = match &mut self.body {
            Body::Stored(r) => r.read(buf)?,
            Body::Deflated(r) => r.read(buf)?,
        };
        if n > 0 || buf.is_empty() {
            if let Some((_, h)) = self.crc.as_mut() {
                h.update(buf.get(..n).unwrap_or_default());
            }
            return Ok(n);
        }
        self.done = true;
        match &mut self.body {
            Body::Stored(r) => r.finish()?,
            Body::Deflated(r) => r.get_mut().finish()?,
        }
        if let Some((want, h)) = self.crc.take() {
            if h.finalize() != want {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "an encrypted entry does not add up (its checksum differs)",
                ));
            }
        }
        Ok(0)
    }
}

/// Entry `i`'s plain bytes, opened with `password` where `lock` asks for one. `shown` names
/// it in an error.
pub(super) fn open<'a>(
    zip: &'a mut ZipArchive<File>,
    i: usize,
    lock: Lock,
    password: Option<&str>,
    shown: &str,
) -> Result<Box<dyn Read + 'a>, Error> {
    let unreadable = |e: zip::result::ZipError| Error::Unreadable(e.to_string());
    let password = match (lock, password) {
        (Lock::Open, _) => return Ok(Box::new(zip.by_index(i).map_err(unreadable)?)),
        (_, None) => return Err(Error::Encrypted(shown.into())),
        (_, Some(p)) => p.as_bytes(),
    };
    let aes = match lock {
        Lock::Aes(aes) => aes,
        _ => {
            return match zip.by_index_decrypt(i, password) {
                Ok(entry) => Ok(Box::new(entry)),
                Err(zip::result::ZipError::InvalidPassword) => {
                    Err(Error::WrongPassword(shown.into()))
                }
                Err(e) => Err(unreadable(e)),
            }
        }
    };
    let raw = zip.by_index_raw(i).map_err(unreadable)?;
    let (stored, crc) = (raw.compressed_size(), raw.crc32());
    let reader = match ZipAesReader::open(raw, aes, stored, password) {
        Ok(r) => r,
        Err(Opened::WrongPassword) => return Err(Error::WrongPassword(shown.into())),
        Err(Opened::Io(e)) => return Err(Error::Unreadable(e.to_string())),
    };
    let body = if aes.method == 8 {
        Body::Deflated(Box::new(DeflateDecoder::new(reader)))
    } else {
        Body::Stored(Box::new(reader))
    };
    Ok(Box::new(AesEntry {
        body,
        // AE-2 leaves the CRC out (zero): the authentication code stands for it.
        crc: (aes.vendor == 1).then(|| (crc, crc32fast::Hasher::new())),
        done: false,
    }))
}

/// Proves `password` on one locked entry, the one with the fewest stored bytes, by reading
/// it whole (its authentication code or CRC checked) before anything is written: for the
/// older cipher, whose one-byte check passes one wrong password in 256, this is what tells.
/// `entries` are each entry's index, lock, stored size and size once unpacked.
pub(super) fn prove(
    zip: &mut ZipArchive<File>,
    entries: impl Iterator<Item = (usize, Lock, u64, u64, String)>,
    password: Option<&str>,
    cancel: &AtomicBool,
) -> Result<(), Error> {
    let Some((i, lock, _, size, shown)) = entries.filter(|e| e.1.locked()).min_by_key(|e| e.2)
    else {
        return Ok(());
    };
    let mut entry = open(zip, i, lock, password, &shown)?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut read = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let n = match entry.read(&mut buf) {
            Ok(n) => n,
            // Its bytes do not add up under this password: the wrong one, or damage.
            Err(_) => return Err(Error::WrongPassword(shown)),
        };
        if n == 0 {
            return Ok(());
        }
        read += n as u64;
        // The header's size is a claim; the bytes are held to it here as when written.
        if read > size {
            return Err(Error::TooBig("an entry is larger than it said"));
        }
    }
}
