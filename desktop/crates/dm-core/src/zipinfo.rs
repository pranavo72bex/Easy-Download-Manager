// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What is inside a ZIP, from its end alone: a ZIP lists its files in a
//! central directory at the very end, so a few range requests learn the names
//! and sizes without fetching the archive, as reference A shows a ZIP's
//! contents before the download is done. Only for a server that serves
//! ranges; anything else, and anything that is not a ZIP, is `None`.

/// One file of an archive, as its central directory lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ZipEntry {
    /// The path inside the archive; a folder ends in `/`.
    pub name: String,
    /// Its size once unpacked.
    pub size: u64,
    /// Its size as stored in the archive.
    pub packed: u64,
    /// Where its own header starts in the archive, so it can be taken out
    /// alone ([`zip_take_one`]).
    pub at: u64,
    /// How it is packed: 0 stored, 8 deflated.
    pub method: u16,
    /// The CRC-32 of its unpacked bytes.
    pub crc: u32,
}

impl ZipEntry {
    /// Whether this is a folder.
    pub fn is_folder(&self) -> bool {
        self.name.ends_with('/')
    }
}

/// The end of a ZIP is read from this much: the end-of-directory record is 22
/// bytes and may be followed by a comment of up to 65,535.
const TAIL: u64 = 22 + 65_535 + 20;

/// A central directory larger than this is not read: the list of a ZIP with
/// hundreds of thousands of files is not worth a transfer of megabytes.
const MOST_DIRECTORY: u64 = 8 * 1024 * 1024;

const END_OF_DIRECTORY: u32 = 0x0605_4b50;
const END_LOCATOR64: u32 = 0x0706_4b50;
const END_OF_DIRECTORY64: u32 = 0x0606_4b50;
const ENTRY: u32 = 0x0201_4b50;

/// Where the central directory is, from the end-of-directory record.
#[derive(Debug, PartialEq, Eq)]
struct Directory {
    entries: u64,
    size: u64,
    offset: u64,
    /// The values were all ones: they are in the ZIP64 record.
    needs64: bool,
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

/// The end-of-directory record in the tail of a file, searched from the back
/// because a comment may follow it; and where in `tail` it starts.
fn end_of_directory(tail: &[u8]) -> Option<(usize, Directory)> {
    let last = tail.len().checked_sub(22)?;
    for at in (0..=last).rev() {
        if u32_at(tail, at)? != END_OF_DIRECTORY {
            continue;
        }
        let comment = usize::from(u16_at(tail, at + 20)?);
        // The comment must run to the end of the file, or this is bytes
        // inside a file that look like the record.
        if at + 22 + comment != tail.len() {
            continue;
        }
        let entries = u64::from(u16_at(tail, at + 10)?);
        let size = u64::from(u32_at(tail, at + 12)?);
        let offset = u64::from(u32_at(tail, at + 16)?);
        let needs64 = entries == 0xFFFF || size == 0xFFFF_FFFF || offset == 0xFFFF_FFFF;
        return Some((
            at,
            Directory {
                entries,
                size,
                offset,
                needs64,
            },
        ));
    }
    None
}

/// The ZIP64 directory location, from the locator that stands just before the
/// end-of-directory record: where the 64-bit record is.
fn locator64(tail: &[u8], end_at: usize) -> Option<u64> {
    let at = end_at.checked_sub(20)?;
    if u32_at(tail, at)? != END_LOCATOR64 {
        return None;
    }
    u64_at(tail, at + 8)
}

/// The directory from a ZIP64 end record (read from its own range).
fn directory64(record: &[u8]) -> Option<Directory> {
    if u32_at(record, 0)? != END_OF_DIRECTORY64 {
        return None;
    }
    Some(Directory {
        entries: u64_at(record, 32)?,
        size: u64_at(record, 40)?,
        offset: u64_at(record, 48)?,
        needs64: false,
    })
}

/// The files a central directory lists. `None` where it is cut off or is not
/// one.
fn entries_of(directory: &[u8], count: u64) -> Option<Vec<ZipEntry>> {
    let mut entries = Vec::new();
    let mut at = 0usize;
    while (entries.len() as u64) < count {
        if u32_at(directory, at)? != ENTRY {
            return None;
        }
        let flags = u16_at(directory, at + 8)?;
        let method = u16_at(directory, at + 10)?;
        let crc = u32_at(directory, at + 16)?;
        let mut header_at = u64::from(u32_at(directory, at + 42)?);
        let mut packed = u64::from(u32_at(directory, at + 20)?);
        let mut size = u64::from(u32_at(directory, at + 24)?);
        let name_length = usize::from(u16_at(directory, at + 28)?);
        let extra_length = usize::from(u16_at(directory, at + 30)?);
        let comment_length = usize::from(u16_at(directory, at + 32)?);
        let name_at = at + 46;
        let name_bytes = directory.get(name_at..name_at + name_length)?;
        let extra = directory.get(name_at + name_length..name_at + name_length + extra_length)?;
        // The real sizes of a big file are in the ZIP64 field of its extra
        // data, in the order of the values that were all ones.
        if size == 0xFFFF_FFFF || packed == 0xFFFF_FFFF || header_at == 0xFFFF_FFFF {
            let mut field = 0usize;
            while field + 4 <= extra.len() {
                let id = u16_at(extra, field)?;
                let length = usize::from(u16_at(extra, field + 2)?);
                if id == 1 {
                    let mut value = field + 4;
                    if size == 0xFFFF_FFFF {
                        size = u64_at(extra, value)?;
                        value += 8;
                    }
                    if packed == 0xFFFF_FFFF {
                        packed = u64_at(extra, value)?;
                        value += 8;
                    }
                    if header_at == 0xFFFF_FFFF {
                        header_at = u64_at(extra, value)?;
                    }
                    break;
                }
                field += 4 + length;
            }
        }
        // With the UTF-8 flag (bit 11) the name is UTF-8; without it, it is in
        // the old DOS code page, of which ASCII is nearly every name and the
        // rest is shown as well as it can be.
        let _ = flags;
        let name = String::from_utf8_lossy(name_bytes).into_owned();
        entries.push(ZipEntry {
            name,
            size,
            packed,
            at: header_at,
            method,
            crc,
        });
        at = name_at + name_length + extra_length + comment_length;
    }
    Some(entries)
}

/// The bytes `low..=high` of `url`, as an exact range answer or `None`.
async fn range(
    client: &reqwest::Client,
    url: &str,
    referrer: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
    low: u64,
    high: u64,
) -> Option<Vec<u8>> {
    let request = crate::download::get(client, url, referrer, sign_in)
        .header(reqwest::header::RANGE, format!("bytes={low}-{high}"));
    let res = crate::auth::send(client, request, url).await.ok()?;
    if res.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return None;
    }
    let wanted = high - low + 1;
    if res.content_length().is_some_and(|n| n != wanted) {
        return None;
    }
    let bytes = tokio::time::timeout(std::time::Duration::from_secs(20), res.bytes())
        .await
        .ok()?
        .ok()?;
    (bytes.len() as u64 == wanted).then(|| bytes.to_vec())
}

/// The length of the file at `url`, from one byte of it: only a server that
/// answers a range as a range (206 with the whole length after the slash)
/// can be asked for the end of a file, so anything else is `None`.
async fn total_size(
    client: &reqwest::Client,
    url: &str,
    referrer: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
) -> Option<u64> {
    let request = crate::download::get(client, url, referrer, sign_in)
        .header(reqwest::header::RANGE, "bytes=0-0");
    let res = crate::auth::send(client, request, url).await.ok()?;
    if res.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return None;
    }
    res.headers()
        .get(reqwest::header::CONTENT_RANGE)?
        .to_str()
        .ok()?
        .rsplit('/')
        .next()?
        .trim()
        .parse::<u64>()
        .ok()
}

/// The files of the ZIP at `url`, learned from its end with range requests
/// (two or three small ones), without fetching the archive. `None` where
/// the server does not serve ranges, the file is not a ZIP, or its directory
/// is too large to be worth reading.
pub async fn zip_contents(
    client: &reqwest::Client,
    url: &str,
    referrer: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
) -> Option<Vec<ZipEntry>> {
    let size = total_size(client, url, referrer, sign_in).await?;
    if size < 22 {
        return None;
    }
    let tail_from = size.saturating_sub(TAIL);
    let tail = range(client, url, referrer, sign_in, tail_from, size - 1).await?;
    let (end_at, mut directory) = end_of_directory(&tail)?;
    if directory.needs64 {
        let record_at = locator64(&tail, end_at)?;
        let record = range(client, url, referrer, sign_in, record_at, record_at + 55).await?;
        directory = directory64(&record)?;
    }
    if directory.size > MOST_DIRECTORY || directory.offset.checked_add(directory.size)? > size {
        return None;
    }
    if directory.entries == 0 {
        return Some(Vec::new());
    }
    // Usually the directory is within the tail already; only a very large
    // one (or a long comment after a small one) needs its own request.
    let bytes = if directory.offset >= tail_from {
        let from = usize::try_from(directory.offset - tail_from).ok()?;
        tail.get(from..from + usize::try_from(directory.size).ok()?)?
            .to_vec()
    } else {
        range(
            client,
            url,
            referrer,
            sign_in,
            directory.offset,
            directory.offset + directory.size - 1,
        )
        .await?
    };
    entries_of(&bytes, directory.entries)
}

/// The most of an archive one file taken out of it alone may be: it is held in
/// memory to be unpacked and checked, and a larger one is the archive's to
/// download.
pub const MOST_TAKEN: u64 = 256 * 1024 * 1024;

const LOCAL_HEADER: u32 = 0x0403_4b50;

/// Takes `entry` out of the ZIP at `url` alone, with range requests for its
/// own bytes, as reference A takes one file out of an archive before the
/// archive is downloaded: unpacked (stored or deflated), checked against the
/// CRC the archive lists, and saved in `folder` under its own name (a free
/// one where that is taken). Returns the path saved. Fails with words a
/// person can read: a folder, a locked or oddly packed file, one too large,
/// a server that stops serving ranges, bytes that do not check.
pub async fn zip_take_one(
    client: &reqwest::Client,
    url: &str,
    referrer: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
    entry: &ZipEntry,
    folder: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    if entry.is_folder() {
        return Err("a folder is not a file to take out".into());
    }
    // Unpacked as well as packed: deflated zeros said to unpack to more than the cap were
    // unpacked into memory up to what the archive claimed, a thousand times the packed bytes
    // (security audit, 2026-11-12).
    if entry.packed > MOST_TAKEN || entry.size > MOST_TAKEN {
        return Err("this file is too large to take out alone; download the archive".into());
    }
    let unreadable = || "the server would not give that part of the archive".to_string();
    let header = range(client, url, referrer, sign_in, entry.at, entry.at + 29)
        .await
        .ok_or_else(unreadable)?;
    if u32_at(&header, 0) != Some(LOCAL_HEADER) {
        return Err("the archive is not laid out as its list says".into());
    }
    let flags = u16_at(&header, 6).unwrap_or(0);
    if flags & 1 != 0 {
        return Err("this file is locked with a password".into());
    }
    let skip =
        u64::from(u16_at(&header, 26).unwrap_or(0)) + u64::from(u16_at(&header, 28).unwrap_or(0));
    let data_at = entry.at + 30 + skip;
    let packed = if entry.packed == 0 {
        Vec::new()
    } else {
        range(
            client,
            url,
            referrer,
            sign_in,
            data_at,
            data_at + entry.packed - 1,
        )
        .await
        .ok_or_else(unreadable)?
    };
    let bytes = unpack(&packed, entry)?;
    let name = crate::probe::sanitize(&entry.name);
    if name.is_empty() {
        return Err("the file has no name to save it under".into());
    }
    tokio::fs::create_dir_all(folder)
        .await
        .map_err(|e| format!("the folder could not be made: {e}"))?;
    let path = folder.join(crate::download::free_name(folder, &name));
    // Written beside it and moved into place, so a half-written file never
    // carries the name.
    let staged = folder.join(format!(".{name}.dm-taking"));
    tokio::fs::write(&staged, &bytes)
        .await
        .map_err(|e| format!("the file could not be written: {e}"))?;
    if let Err(e) = tokio::fs::rename(&staged, &path).await {
        let _ = tokio::fs::remove_file(&staged).await;
        return Err(format!("the file could not be written: {e}"));
    }
    Ok(path)
}

/// `packed`, the stored bytes of `entry`, unpacked and checked.
fn unpack(packed: &[u8], entry: &ZipEntry) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let bytes = match entry.method {
        0 => packed.to_vec(),
        8 => {
            let mut out = Vec::with_capacity(usize::try_from(entry.size.min(MOST_TAKEN)).unwrap_or(0));
            // Never more than the list says, plus one to tell a liar; never past the cap whatever
            // it says.
            flate2::read::DeflateDecoder::new(packed)
                .take(entry.size.min(MOST_TAKEN).saturating_add(1))
                .read_to_end(&mut out)
                .map_err(|_| "the file's packed bytes are damaged".to_string())?;
            out
        }
        other => {
            return Err(format!(
                "this file is packed in a way dm does not unpack (method {other}); download the archive"
            ))
        }
    };
    if bytes.len() as u64 != entry.size {
        return Err("the file came out a different size than the archive lists".into());
    }
    let mut crc = flate2::Crc::new();
    crc.update(&bytes);
    if crc.sum() != entry.crc {
        return Err("the file's bytes do not match the archive's check (CRC)".into());
    }
    Ok(bytes)
}

/// The files of a ZIP on disk, read from its end alone as [`zip_contents`]
/// reads one at an address: for a finished download's Properties, which has
/// the file and needs no server. `None` where the file is no ZIP, is cut
/// off, or its directory is too large to be worth listing.
pub fn zip_contents_of_file(path: &std::path::Path) -> Option<Vec<ZipEntry>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    if size < 22 {
        return None;
    }
    let read_at = |file: &mut std::fs::File, from: u64, to: u64| -> Option<Vec<u8>> {
        file.seek(SeekFrom::Start(from)).ok()?;
        let mut bytes = vec![0u8; usize::try_from(to.checked_sub(from)? + 1).ok()?];
        file.read_exact(&mut bytes).ok()?;
        Some(bytes)
    };
    let tail_from = size.saturating_sub(TAIL);
    let tail = read_at(&mut file, tail_from, size - 1)?;
    let (end_at, mut directory) = end_of_directory(&tail)?;
    if directory.needs64 {
        let record_at = locator64(&tail, end_at)?;
        let record = read_at(&mut file, record_at, record_at + 55)?;
        directory = directory64(&record)?;
    }
    if directory.size > MOST_DIRECTORY || directory.offset.checked_add(directory.size)? > size {
        return None;
    }
    if directory.entries == 0 {
        return Some(Vec::new());
    }
    let bytes = if directory.offset >= tail_from {
        let from = usize::try_from(directory.offset - tail_from).ok()?;
        tail.get(from..from + usize::try_from(directory.size).ok()?)?
            .to_vec()
    } else {
        read_at(
            &mut file,
            directory.offset,
            directory.offset + directory.size - 1,
        )?
    };
    entries_of(&bytes, directory.entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file said to unpack to more than the cap is refused before anything is fetched, and
    /// unpacking stops at the cap whatever the list says (security audit, 2026-11-12).
    #[tokio::test]
    async fn a_file_said_to_unpack_past_the_cap_is_refused_and_never_unpacked_past_it() {
        let huge = ZipEntry {
            name: "zeros.bin".into(),
            size: 10 * MOST_TAKEN,
            packed: 1024,
            at: 0,
            method: 8,
            crc: 0,
        };
        let dir = tempfile::tempdir().unwrap();
        let why = zip_take_one(
            &crate::client(),
            "http://127.0.0.1:9/a.zip",
            None,
            None,
            &huge,
            dir.path(),
        )
        .await
        .unwrap_err();
        assert!(why.contains("too large"), "{why}");
        // Deflated zeros: a few kilobytes that would unpack to far more.
        use std::io::Write;
        let mut packer =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
        packer.write_all(&vec![0u8; 4 * 1024 * 1024]).unwrap();
        let packed = packer.finish().unwrap();
        let lying = ZipEntry {
            size: u64::MAX,
            packed: packed.len() as u64,
            ..huge
        };
        // Unpacked no further than the cap plus one, then refused as not what the list says.
        assert!(unpack(&packed, &lying).is_err());
    }

    /// A ZIP with a folder and two files, stored, built by the writer.
    fn archive() -> Vec<u8> {
        use std::io::Write;
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buffer);
            let stored = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.add_directory("docs/", stored).unwrap();
            zip.start_file("docs/readme.txt", stored).unwrap();
            zip.write_all(&vec![b'a'; 1000]).unwrap();
            zip.start_file("setup.exe", stored).unwrap();
            zip.write_all(&vec![b'b'; 5000]).unwrap();
            zip.set_comment("a comment after the directory").unwrap();
            zip.finish().unwrap();
        }
        buffer.into_inner()
    }

    #[test]
    fn the_directory_is_found_through_a_comment_and_its_files_are_listed() {
        let bytes = archive();
        let (at, directory) = end_of_directory(&bytes).expect("an end record");
        assert!(at > 0);
        assert_eq!(directory.entries, 3);
        let start = usize::try_from(directory.offset).unwrap();
        let end = start + usize::try_from(directory.size).unwrap();
        let entries = entries_of(&bytes[start..end], directory.entries).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "docs/");
        assert!(entries[0].is_folder());
        assert_eq!(
            entries[1],
            ZipEntry {
                name: "docs/readme.txt".into(),
                size: 1000,
                packed: 1000,
                ..entries[1].clone()
            }
        );
        assert_eq!(entries[2].name, "setup.exe");
        assert_eq!(entries[2].size, 5000);
    }

    #[test]
    fn bytes_that_are_not_a_zip_or_are_cut_off_list_nothing() {
        assert!(end_of_directory(b"just some text, not an archive at all").is_none());
        // A record whose comment does not reach the end is not the record.
        let mut bytes = archive();
        bytes.push(0);
        assert!(end_of_directory(&bytes).is_none());
        // A directory cut short is not read.
        let good = archive();
        let (_, directory) = end_of_directory(&good).unwrap();
        let start = usize::try_from(directory.offset).unwrap();
        assert!(entries_of(&good[start..start + 30], 3).is_none());
    }

    #[test]
    fn a_zip_on_disk_is_listed_from_its_end_and_a_file_that_is_not_one_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("pack.zip");
        std::fs::write(&zip, archive()).unwrap();
        let entries = zip_contents_of_file(&zip).expect("the entries");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[1].name, "docs/readme.txt");
        assert_eq!(entries[2].size, 5000);

        let text = dir.path().join("notes.zip");
        std::fs::write(
            &text,
            b"just some text that is long enough to be read, but no archive",
        )
        .unwrap();
        assert!(zip_contents_of_file(&text).is_none());
        assert!(zip_contents_of_file(&dir.path().join("missing.zip")).is_none());
    }
}
