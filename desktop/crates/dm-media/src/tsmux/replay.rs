// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A stream put into an MP4 straight from its segments, with nothing between them and the MP4
//! on the disk (gaps row Y21).
//!
//! The MP4 is written with its index first, so every sample's size and time must be known
//! before the first is copied. That took a scratch file of each track's samples beside the MP4,
//! from a stream gathered into one `.ts` first: the segments' bytes on the disk three times
//! over, the stream, its samples and the MP4. Now the segments are read where they lie, one
//! after another as one stream (`Joined`): once for the index, the samples counted and not
//! kept, then again for each track as the writer copies it (`Replay`), its samples made afresh,
//! the same bytes in the same order. Asked to (`Segments::Freed`), each segment is freed once
//! every track has read past it (`Freer`), so the disk holds the MP4 and what is left of the
//! stream, not both whole.

use std::cell::{Cell, RefCell};
use std::io::{Seek, SeekFrom};
use std::rc::Rc;

use super::*;

/// What a remux from a stream's segments does with them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segments {
    /// Left as they are, for the caller to free once the MP4 is whole: a remux cut short (dm
    /// quit, the computer off) has lost none of them.
    Kept,
    /// Each freed once every sample in it is read into the MP4, for a disk without room for the
    /// stream and its MP4 side by side. Cut short, the MP4 is not whole and the segments freed
    /// are gone: they are fetched again.
    Freed,
}

/// Files read one after another as one stream.
pub(super) struct Joined {
    paths: Vec<PathBuf>,
    /// Which file is being read, and the file, once opened.
    at: usize,
    file: Option<File>,
    /// Told each time a file is read to its end: none where nothing is freed.
    freer: Option<Told>,
}

/// The [`Freer`] of a stream's files, and which of its readers this is.
pub(super) type Told = (Rc<RefCell<Freer>>, usize);

impl Joined {
    /// `paths`, to be read in order.
    pub(super) fn new(paths: Vec<PathBuf>, freer: Option<Told>) -> Self {
        Joined {
            paths,
            at: 0,
            file: None,
            freer,
        }
    }
}

impl Read for Joined {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if self.file.is_none() {
                let Some(path) = self.paths.get(self.at) else {
                    return Ok(0);
                };
                self.file = Some(File::open(path)?);
            }
            let Some(file) = self.file.as_mut() else {
                return Ok(0);
            };
            let read = file.read(buf)?;
            if read > 0 {
                return Ok(read);
            }
            // This file is read to its end, and closed: on to the next.
            self.file = None;
            self.at += 1;
            if let Some((freer, reader)) = &self.freer {
                freer.borrow_mut().passed(*reader, self.at);
            }
        }
    }
}

/// Frees each segment of a stream once every track's reader is past it.
pub(super) struct Freer {
    paths: Vec<PathBuf>,
    /// How far the picture's reader and the sound's are: the files before are read. A track the
    /// MP4 does not have is as far as can be.
    past: [usize; 2],
    /// The files before this one are freed.
    freed: usize,
    /// The samples' bytes the readers have handed the writer so far.
    copied: Rc<Cell<u64>>,
    /// Told before each file is freed, and once the MP4 is whole, how many bytes of samples
    /// the writer has been handed: what a test measures the disk by.
    watch: Option<Box<dyn FnMut(u64)>>,
}

impl Freer {
    /// The reader of the picture, the sound, or each: whichever of `streams` has samples.
    fn new(
        paths: Vec<PathBuf>,
        streams: &[Elementary],
        copied: Rc<Cell<u64>>,
        watch: Option<Box<dyn FnMut(u64)>>,
    ) -> Self {
        let mut past = [usize::MAX; 2];
        for stream in streams.iter().filter(|s| !s.samples.is_empty()) {
            if let Some(reader) = past.get_mut(reader_of(stream)) {
                *reader = 0;
            }
        }
        Freer {
            paths,
            past,
            freed: 0,
            copied,
            watch,
        }
    }

    /// `reader` has read every file before `at`: those every reader has are freed. One that
    /// cannot be is left, for the caller to free once the MP4 is whole.
    fn passed(&mut self, reader: usize, at: usize) {
        if let Some(past) = self.past.get_mut(reader) {
            *past = (*past).max(at);
        }
        let upto = self
            .past
            .iter()
            .copied()
            .min()
            .unwrap_or(0)
            .min(self.paths.len());
        while self.freed < upto {
            self.tell();
            if let Some(path) = self.paths.get(self.freed) {
                let _ = std::fs::remove_file(path);
            }
            self.freed += 1;
        }
    }

    /// What the writer has been handed so far, told to the watch.
    fn tell(&mut self) {
        let copied = self.copied.get();
        if let Some(watch) = self.watch.as_mut() {
            watch(copied);
        }
    }
}

/// Which reader a stream's is: the picture's first.
fn reader_of(stream: &Elementary) -> usize {
    usize::from(!PICTURES.contains(&stream.kind))
}

/// One track's samples, made again from the stream as the MP4 writer reads them: the bytes its
/// scratch file held, in the same order, read in order.
pub(super) struct Replay {
    demux: Demux<'static>,
    /// Made and not yet read, from `at` on.
    buffer: Vec<u8>,
    at: usize,
    /// Where the next byte is among the track's samples.
    position: u64,
    /// Every reader's bytes handed on, together.
    copied: Rc<Cell<u64>>,
}

impl Replay {
    fn new(
        input: Joined,
        picture: bool,
        locks: &crate::sealed::Locks,
        copied: Rc<Cell<u64>>,
    ) -> Self {
        Replay {
            demux: Demux::new(input, Mode::Kept(picture), locks),
            buffer: Vec::new(),
            at: 0,
            position: 0,
            copied,
        }
    }

    /// Bytes to read: `false` where the stream has no more.
    fn fill(&mut self) -> std::io::Result<bool> {
        while self.at >= self.buffer.len() {
            let more = self.demux.step().map_err(std::io::Error::other)?;
            self.demux.take_kept(&mut self.buffer);
            self.at = 0;
            if self.buffer.is_empty() && !more {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

impl Read for Replay {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if out.is_empty() || !self.fill()? {
            return Ok(0);
        }
        let ready = self.buffer.get(self.at..).unwrap_or_default();
        let count = ready.len().min(out.len());
        if let (Some(to), Some(from)) = (out.get_mut(..count), ready.get(..count)) {
            to.copy_from_slice(from);
        }
        self.at += count;
        self.position += count as u64;
        self.copied.set(self.copied.get() + count as u64);
        Ok(count)
    }
}

impl Seek for Replay {
    /// Only on: the writer reads a track's samples in their order, and a sample it does not
    /// copy is made and passed over.
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let to = match to {
            SeekFrom::Start(to) => to,
            SeekFrom::Current(by) => self.position.checked_add_signed(by).ok_or_else(|| {
                std::io::Error::other("a track's samples are read from their start")
            })?,
            SeekFrom::End(_) => {
                return Err(std::io::Error::other("a track's samples have no known end"))
            }
        };
        if to < self.position {
            return Err(std::io::Error::other("a track's samples are read in order"));
        }
        while self.position < to {
            if !self.fill()? {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            let left = (self.buffer.len() - self.at) as u64;
            let step = left.min(to - self.position);
            self.at += step as usize;
            self.position += step;
        }
        Ok(self.position)
    }
}

/// How a remux reads its stream.
pub(super) struct Reading<'a> {
    /// The stream's files, one after another.
    pub(super) inputs: &'a [PathBuf],
    pub(super) locks: &'a crate::sealed::Locks,
    /// Each file freed once every track has read past it.
    pub(super) freed: bool,
    /// Told before each file is freed, and once the MP4 is whole (`Freer::watch`).
    pub(super) watch: Option<Box<dyn FnMut(u64)>>,
}

/// The stream `reading` names put into an MP4 at `out`, read twice and written once: the index
/// first, then each track's samples as the writer copies them. Returns its length.
pub(super) fn remux<F>(reading: Reading<'_>, out: &Path, place: F) -> Result<u64, String>
where
    F: FnOnce(i64) -> Vec<crate::subtitles::Subtitle>,
{
    let Reading {
        inputs,
        locks,
        freed,
        watch,
    } = reading;
    let index = Joined::new(inputs.to_vec(), None);
    let mut streams = Demux::new(index, Mode::Counted, locks).finish()?;
    let copied = Rc::new(Cell::new(0u64));
    let freer = freed.then(|| {
        let freer = Freer::new(inputs.to_vec(), &streams, Rc::clone(&copied), watch);
        Rc::new(RefCell::new(freer))
    });
    let mut source = |stream: &Elementary| -> Result<Box<dyn Source>, String> {
        let reader = freer.as_ref().map(|f| (Rc::clone(f), reader_of(stream)));
        let input = Joined::new(inputs.to_vec(), reader);
        let picture = PICTURES.contains(&stream.kind);
        Ok(Box::new(Replay::new(
            input,
            picture,
            locks,
            Rc::clone(&copied),
        )))
    };
    let made = build_from(&mut streams, out, place, &mut source)?;
    if let Some(freer) = &freer {
        freer.borrow_mut().tell();
    }
    Ok(made)
}
