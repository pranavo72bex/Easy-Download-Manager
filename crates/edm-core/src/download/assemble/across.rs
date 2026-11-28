// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! One part copied to another drive, where no rename can take it (gaps rows 166 and 185): on
//! Windows by the system's own copy, elsewhere a piece at a time as the join copies; stopped
//! part-way by a Cancel either way.

use super::*;

/// Copies the one part `from` to `to` on another drive, held to `span` where one is given, with a
/// look at `stop` between pieces: what it says ends the copy part-way, and the file begun goes
/// with the error (the caller removes it, as it does any join's). The part stays; the caller
/// removes it once the copy is whole.
///
/// By the system's own copy (`CopyFileEx`, as `std::fs::copy` makes it), which hands the bytes
/// over in its own large blocks: its speed across drives was measured worth having (gaps row
/// 166). Its progress routine, which the system calls between blocks, is where it looks.
#[cfg(windows)]
pub(super) fn copy_across(
    from: &Path,
    to: &Path,
    span: Option<u64>,
    stop: &mut dyn FnMut() -> Result<(), Error>,
) -> Result<u64, Error> {
    let bytes = system_copy(from, to, stop)?;
    match span.filter(|&span| span < bytes) {
        Some(span) => {
            std::fs::OpenOptions::new()
                .write(true)
                .open(to)?
                .set_len(span)?;
            Ok(span)
        }
        None => Ok(bytes),
    }
}

/// Copies the one part `from` to `to` on another drive, held to `span` where one is given, with a
/// look at `stop` between pieces: what it says ends the copy part-way, and the file begun goes
/// with the error (the caller removes it, as it does any join's). The part stays; the caller
/// removes it once the copy is whole.
///
/// A piece at a time, as the join of several parts copies (`copy_pieces`): on Linux each piece
/// is the kernel's own copy, as `std::fs::copy` was; on macOS a clone cannot cross drives, and
/// the system's copy there reads and writes through a buffer as this does.
#[cfg(not(windows))]
pub(super) fn copy_across(
    from: &Path,
    to: &Path,
    span: Option<u64>,
    stop: &mut dyn FnMut() -> Result<(), Error>,
) -> Result<u64, Error> {
    use std::io::Write;
    let mut source = std::fs::File::open(from)?;
    let mut out = std::io::BufWriter::with_capacity(JOIN_BUFFER, std::fs::File::create(to)?);
    copy_pieces(&mut source, &mut out, span.unwrap_or(u64::MAX), stop)?;
    out.flush()?;
    Ok(out.get_ref().metadata()?.len())
}

/// `CopyFileEx`, as `std::fs::copy` calls it, with a progress routine that looks at `stop` as
/// the copy begins and after each block: what it says cancels the copy, and the system removes
/// the file it had begun. The bytes of the file's own stream copied.
#[cfg(windows)]
fn system_copy(
    from: &Path,
    to: &Path,
    stop: &mut dyn FnMut() -> Result<(), Error>,
) -> Result<u64, Error> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Storage::FileSystem::{
        CopyFileExW, COPYPROGRESSROUTINE_PROGRESS, LPPROGRESS_ROUTINE_CALLBACK_REASON,
        PROGRESS_CANCEL, PROGRESS_CONTINUE,
    };

    /// What the progress routine is handed: where it looks, what stopped the copy, and how much
    /// of the file's own stream is copied.
    struct Looks<'a> {
        stop: &'a mut dyn FnMut() -> Result<(), Error>,
        stopped: Option<Error>,
        copied: u64,
    }

    /// Called by the system on the copying thread as the copy begins and after each block.
    unsafe extern "system" fn progress(
        _total: i64,
        _total_copied: i64,
        _stream_length: i64,
        stream_copied: i64,
        stream: u32,
        _reason: LPPROGRESS_ROUTINE_CALLBACK_REASON,
        _from: HANDLE,
        _to: HANDLE,
        data: *const core::ffi::c_void,
    ) -> COPYPROGRESSROUTINE_PROGRESS {
        // SAFETY: `data` is the `Looks` that `system_copy` handed `CopyFileExW`, which calls this
        // only on that thread and only while it runs, while nothing else reaches the `Looks`.
        let looks = unsafe { &mut *data.cast_mut().cast::<Looks<'_>>() };
        // The file's own stream is the first; others are its named streams, if it has any.
        if stream == 1 {
            looks.copied = u64::try_from(stream_copied).unwrap_or(0);
        }
        match (looks.stop)() {
            Ok(()) => PROGRESS_CONTINUE,
            Err(stopped) => {
                looks.stopped = Some(stopped);
                PROGRESS_CANCEL
            }
        }
    }

    // A name past `MAX_PATH` in the form the system's calls take there (`\\?\…`), as
    // `std::fs::copy` hands it on; as given where it is shorter, or the system cannot say.
    let wide = |path: &Path| -> Vec<u16> {
        let path = if path.as_os_str().encode_wide().count() >= 248 {
            std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
        } else {
            path.to_path_buf()
        };
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    };
    let (from, to) = (wide(from), wide(to));
    let mut looks = Looks {
        stop,
        stopped: None,
        copied: 0,
    };
    // SAFETY: both names are NUL-terminated and outlive the call; `looks` outlives it too, and is
    // reached only by the progress routine while the call runs; no cancel flag is given.
    let done = unsafe {
        CopyFileExW(
            from.as_ptr(),
            to.as_ptr(),
            Some(progress),
            std::ptr::from_mut(&mut looks).cast::<core::ffi::c_void>(),
            std::ptr::null_mut(),
            0,
        )
    };
    let failed = (done == 0).then(std::io::Error::last_os_error);
    if let Some(stopped) = looks.stopped.take() {
        return Err(stopped);
    }
    match failed {
        Some(e) => Err(e.into()),
        None => Ok(looks.copied),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The copy no rename can make, called as the join calls it for one part on another drive
    /// (no second drive is needed for it): whole and held to its span; and stopped part-way by a
    /// Cancel, the download's control looked at between pieces and cancelled at the second look,
    /// once the first piece is copied (gaps row 185).
    #[test]
    fn a_copy_to_another_drive_is_held_to_its_span_and_stops_part_way_on_a_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("part"), dir.path().join("file"));
        let control = crate::control::Control::new();
        std::fs::write(&from, b"ffffGG").unwrap();
        let bytes =
            copy_across(&from, &to, Some(4), &mut || crate::heavy::stopped(&control)).unwrap();
        assert_eq!((bytes, std::fs::read(&to).unwrap()), (4, b"ffff".to_vec()));
        assert!(from.exists(), "the part is the caller's to remove");

        // Sparse where the file system allows: many pieces of either copy, few bytes on the disk.
        let length: u64 = 256 << 20;
        std::fs::File::create(&from)
            .unwrap()
            .set_len(length)
            .unwrap();
        let mut looks = 0;
        let outcome = copy_across(&from, &to, None, &mut || {
            looks += 1;
            if looks == 2 {
                control.cancel();
            }
            crate::heavy::stopped(&control)
        });
        assert!(matches!(outcome, Err(Error::Cancelled)), "{outcome:?}");
        assert_eq!(looks, 2, "the copy looked again after the cancel");
        // Gone (the system's copy removes it), or as far as the first piece.
        let copied = std::fs::metadata(&to).map_or(0, |m| m.len());
        assert!(copied < length, "the copy ran to its end after the cancel");
        assert!(from.exists(), "the part went with a copy cut short");
    }
}
