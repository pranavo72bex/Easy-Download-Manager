//! Pictures of an AV1 video, for a part saved as GIF on Linux, where the
//! system has no video decoder dm can use (gaps row 54). AV1 is royalty-free
//! and YouTube offers most videos in it; the samples come from dm's own MP4
//! reader, within the part's edit list, and are decoded by rav1d (BSD-2, a
//! Rust port of dav1d, used without its assembly) through its dav1d calls.
//! Each picture kept is made RGB here and small with `super::shrink`.

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::ptr::NonNull;

use rav1d::include::dav1d::data::Dav1dData;
use rav1d::include::dav1d::dav1d::{Dav1dContext, Dav1dSettings};
use rav1d::include::dav1d::headers::{
    DAV1D_PIXEL_LAYOUT_I400, DAV1D_PIXEL_LAYOUT_I420, DAV1D_PIXEL_LAYOUT_I422,
};
use rav1d::include::dav1d::picture::Dav1dPicture;
use rav1d::src::lib::{
    dav1d_close, dav1d_data_create, dav1d_data_unref, dav1d_default_settings, dav1d_get_picture,
    dav1d_open, dav1d_picture_unref, dav1d_send_data,
};

use crate::gif::Frame;

/// Whether the MP4's picture is AV1 (`av01`), the only kind read here.
pub(super) fn is_av1(stsd: &[u8]) -> bool {
    stsd.get(20..24) == Some(b"av01")
}

/// The sequence header and any other configuration OBUs in the `av1C` box of
/// an `av01` sample entry, given before the first picture.
fn config_obus(stsd: &[u8]) -> Vec<u8> {
    // The full stsd box: 16 bytes to the entry, 8 of entry header, 78 of a
    // visual sample entry, then its child boxes.
    let Some(mut children) = stsd.get(16 + 8 + 78..) else {
        return Vec::new();
    };
    while children.len() >= 8 {
        let size =
            u32::from_be_bytes([children[0], children[1], children[2], children[3]]) as usize;
        if size < 8 || size > children.len() {
            break;
        }
        if &children[4..8] == b"av1C" {
            // Four bytes of marker, version and profile before the OBUs.
            return children
                .get(12..size)
                .map(<[u8]>::to_vec)
                .unwrap_or_default();
        }
        children = &children[size..];
    }
    Vec::new()
}

/// What is kept of the decoded pictures: the earliest in each tick of the
/// part, already small.
struct Held {
    start: f64,
    end: f64,
    fps: f64,
    width: usize,
    size: Option<(usize, usize)>,
    ticks: BTreeMap<u64, (f64, Vec<u8>)>,
}

/// Pictures `fps` a second of the AV1 MP4 at `path`, at most `width` wide,
/// within the part its edit list plays.
pub(super) fn read(path: &Path, fps: u32, width: u32) -> Result<(u16, u16, Vec<Frame>), String> {
    let tracks = crate::mp4mux::tracks_in_file(path)?;
    let video = tracks
        .into_iter()
        .find(|t| t.hdlr.get(16..20) == Some(b"vide"))
        .ok_or("the part has no picture")?;
    if !is_av1(&video.stsd) {
        return Err(
            "on Linux a GIF is made from a video's AV1 stream, and this part is not AV1".into(),
        );
    }
    let scale = f64::from(video.timescale.max(1));
    let (start, end) = match video.edits.iter().find(|e| e.media_time >= 0) {
        Some(edit) => {
            let from = edit.media_time as f64 / scale;
            let long = edit.duration as f64 / f64::from(video.edits_timescale.max(1));
            (from, if long > 0.0 { from + long } else { f64::MAX })
        }
        None => (0.0, f64::MAX),
    };
    if end - start > 120.0 {
        return Err("a GIF is made of two minutes at most; choose a shorter part".into());
    }
    let mut held = Held {
        start,
        end,
        fps: f64::from(fps),
        width: width as usize,
        size: None,
        ticks: BTreeMap::new(),
    };
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut decoder = Decoder::open()?;
    let mut first = config_obus(&video.stsd);
    let mut decode_time = video.start.unwrap_or(0) as i64;
    for sample in &video.samples {
        let pts = decode_time + i64::from(sample.cts);
        decode_time += i64::from(sample.duration);
        // Past the part's end, nothing more is needed.
        if pts as f64 / scale >= end {
            break;
        }
        let mut bytes = std::mem::take(&mut first);
        let at = bytes.len();
        bytes.resize(at + sample.size as usize, 0);
        file.seek(SeekFrom::Start(sample.offset))
            .and_then(|_| file.read_exact(&mut bytes[at..]))
            .map_err(|e| format!("the part could not be read: {e}"))?;
        decoder.send(&bytes, pts, |picture| take(&mut held, picture, scale))?;
    }
    decoder.drain(|picture| take(&mut held, picture, scale))?;

    let (w, h) = held.size.ok_or("no picture of the part was decoded")?;
    let ticks: Vec<_> = held.ticks.into_iter().collect();
    let mut frames = Vec::with_capacity(ticks.len());
    for (i, (tick, (_, rgb))) in ticks.iter().enumerate() {
        let next = ticks.get(i + 1).map_or(tick + 1, |(t, _)| *t);
        let delay = ((next - tick) as f64 * 100.0 / f64::from(fps.max(1)))
            .round()
            .max(2.0) as u16;
        frames.push(Frame {
            rgb: rgb.clone(),
            delay,
        });
    }
    let (w, h) = (
        u16::try_from(w).map_err(|_| "the picture is too wide")?,
        u16::try_from(h).map_err(|_| "the picture is too tall")?,
    );
    Ok((w, h, frames))
}

/// Keeps a decoded picture if it is the earliest of its tick within the part.
fn take(held: &mut Held, picture: &Picture<'_>, scale: f64) {
    let at = picture.timestamp as f64 / scale;
    if at < held.start - 1e-6 || at >= held.end {
        return;
    }
    let tick = ((at - held.start) * held.fps).floor().max(0.0) as u64;
    if held.ticks.get(&tick).is_some_and(|(t, _)| *t <= at) {
        return;
    }
    let (cw, ch) = (picture.width, picture.height);
    if cw == 0 || ch == 0 {
        return;
    }
    let (w, h) = *held.size.get_or_insert_with(|| {
        let most = held.width;
        let (w, h) = if cw >= ch {
            let w = most.min(cw);
            (w, ch * w / cw)
        } else {
            let h = most.min(ch);
            (cw * h / ch, h)
        };
        ((w & !1).max(2), (h & !1).max(2))
    });
    let bgra = picture.bgra();
    let rgb = super::shrink(&bgra, cw * 4, (0, 0, cw, ch), w, h);
    held.ticks.insert(tick, (at, rgb));
}

/// A decoded picture, borrowed from the decoder until it is let go.
struct Picture<'a> {
    width: usize,
    height: usize,
    /// Bits a sample: 8, or 10 or 12 in two bytes each.
    bits: u32,
    /// 0 for no colour (monochrome), 1 for half-width and half-height
    /// colour (4:2:0), 2 for half-width (4:2:2), 3 for full (4:4:4).
    layout: u8,
    planes: [&'a [u8]; 3],
    strides: [usize; 2],
    /// BT.601 (true) or BT.709 (false), and whether its range is full.
    bt601: bool,
    full_range: bool,
    timestamp: i64,
}

impl Picture<'_> {
    /// The picture as BGRA, row after row.
    fn bgra(&self) -> Vec<u8> {
        let (w, h) = (self.width, self.height);
        let wide = self.bits > 8;
        let max = f32::from(((1u32 << self.bits) - 1) as u16);
        let sample = |plane: &[u8], stride: usize, x: usize, y: usize| -> f32 {
            if wide {
                let at = y * stride + x * 2;
                plane
                    .get(at..at + 2)
                    .map_or(0.0, |b| f32::from(u16::from_le_bytes([b[0], b[1]])))
                    / max
            } else {
                plane.get(y * stride + x).map_or(0.0, |b| f32::from(*b)) / 255.0
            }
        };
        let (kr, kb) = if self.bt601 {
            (0.299, 0.114)
        } else {
            (0.2126, 0.0722)
        };
        let kg = 1.0 - kr - kb;
        let mut out = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let mut luma = sample(self.planes[0], self.strides[0], x, y);
                let (mut cb, mut cr) = match self.layout {
                    0 => (0.5, 0.5),
                    layout => {
                        let (cx, cy) = match layout {
                            1 => (x / 2, y / 2),
                            2 => (x / 2, y),
                            _ => (x, y),
                        };
                        (
                            sample(self.planes[1], self.strides[1], cx, cy),
                            sample(self.planes[2], self.strides[1], cx, cy),
                        )
                    }
                };
                if !self.full_range {
                    luma = (luma - 16.0 / 255.0) * 255.0 / 219.0;
                    cb = (cb - 128.0 / 255.0) * 255.0 / 224.0 + 0.5;
                    cr = (cr - 128.0 / 255.0) * 255.0 / 224.0 + 0.5;
                }
                let (pb, pr) = (cb - 0.5, cr - 0.5);
                let r = luma + 2.0 * (1.0 - kr) * pr;
                let b = luma + 2.0 * (1.0 - kb) * pb;
                let g = (luma - kr * r - kb * b) / kg;
                let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                let at = (y * w + x) * 4;
                out[at..at + 4].copy_from_slice(&[byte(b), byte(g), byte(r), 255]);
            }
        }
        out
    }
}

/// rav1d's decoder, closed when dropped.
struct Decoder {
    context: Option<Dav1dContext>,
}

/// dav1d says "not now" with -EAGAIN: send more, or take a picture first.
fn again(result: i32) -> bool {
    result < 0
        && std::io::Error::from_raw_os_error(-result).kind() == std::io::ErrorKind::WouldBlock
}

impl Decoder {
    fn open() -> Result<Decoder, String> {
        let mut settings = std::mem::MaybeUninit::<Dav1dSettings>::uninit();
        let mut context: Option<Dav1dContext> = None;
        // SAFETY: `dav1d_default_settings` writes every field of the settings
        // it is given, so they are initialised before they are read; `context`
        // is valid to write, and `dav1d_open` leaves it `None` on failure.
        let result = unsafe {
            dav1d_default_settings(NonNull::new_unchecked(settings.as_mut_ptr()));
            let mut settings = settings.assume_init();
            // One picture out for each in, on this thread's pace: a GIF needs
            // few pictures, and nothing is held back to reorder.
            settings.n_threads = 1;
            settings.max_frame_delay = 1;
            dav1d_open(
                Some(NonNull::from(&mut context)),
                Some(NonNull::from(&mut settings)),
            )
        };
        if result.0 < 0 || context.is_none() {
            return Err("the AV1 decoder could not be started".into());
        }
        Ok(Decoder { context })
    }

    /// Sends one sample, stamped `timestamp`, and hands each picture it gives
    /// to `keep`.
    fn send(
        &mut self,
        bytes: &[u8],
        timestamp: i64,
        mut keep: impl FnMut(&Picture<'_>),
    ) -> Result<(), String> {
        let mut data = Dav1dData::default();
        // SAFETY: `dav1d_data_create` makes a buffer of `bytes.len()` bytes
        // owned by `data` and returns where it starts (null on failure), which
        // is written within that length only; `data` is valid throughout, and
        // whatever of it the decoder has not taken is let go below.
        unsafe {
            let at = dav1d_data_create(Some(NonNull::from(&mut data)), bytes.len());
            if at.is_null() {
                return Err("the AV1 decoder had no room for a picture".into());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), at, bytes.len());
        }
        data.m.timestamp = timestamp;
        let outcome = loop {
            // SAFETY: the context is open (it is closed only on drop), and
            // `data` is valid to read and write.
            let result = unsafe { dav1d_send_data(self.context, Some(NonNull::from(&mut data))) };
            if result.0 >= 0 && data.sz == 0 {
                break Ok(());
            }
            if result.0 < 0 && !again(result.0) {
                break Err("the AV1 decoder could not read the part".to_string());
            }
            // Full: pictures out before more goes in.
            if let Err(e) = self.pictures(&mut keep) {
                break Err(e);
            }
        };
        if data.sz > 0 {
            // SAFETY: `data` still owns what the decoder did not take.
            unsafe { dav1d_data_unref(Some(NonNull::from(&mut data))) };
        }
        outcome?;
        self.pictures(&mut keep)
    }

    /// Takes every picture the decoder has ready.
    fn pictures(&mut self, keep: &mut impl FnMut(&Picture<'_>)) -> Result<(), String> {
        loop {
            let mut picture = Dav1dPicture::default();
            // SAFETY: the context is open, and `picture` is valid to write.
            let result =
                unsafe { dav1d_get_picture(self.context, Some(NonNull::from(&mut picture))) };
            if again(result.0) {
                return Ok(());
            }
            if result.0 < 0 {
                return Err("the AV1 decoder could not make a picture of the part".into());
            }
            // SAFETY: a picture the decoder gave holds planes of the size its
            // parameters say, at the strides it says, valid until it is let go
            // below; the slices made here do not outlive that.
            unsafe {
                if let Some(view) = view(&picture) {
                    keep(&view);
                }
                dav1d_picture_unref(Some(NonNull::from(&mut picture)));
            }
        }
    }

    /// Takes the pictures still held once every sample is in.
    fn drain(&mut self, mut keep: impl FnMut(&Picture<'_>)) -> Result<(), String> {
        self.pictures(&mut keep)
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        if self.context.is_some() {
            // SAFETY: the context came from `dav1d_open` and is closed once.
            unsafe { dav1d_close(Some(NonNull::from(&mut self.context))) };
        }
    }
}

/// A borrowed view of a picture's planes.
///
/// # Safety
///
/// `picture` must be one `dav1d_get_picture` gave and not yet let go.
unsafe fn view(picture: &Dav1dPicture) -> Option<Picture<'_>> {
    let (w, h) = (
        usize::try_from(picture.p.w).ok()?,
        usize::try_from(picture.p.h).ok()?,
    );
    let bits = u32::try_from(picture.p.bpc).ok()?;
    let layout = match picture.p.layout {
        l if l == DAV1D_PIXEL_LAYOUT_I400 => 0,
        l if l == DAV1D_PIXEL_LAYOUT_I420 => 1,
        l if l == DAV1D_PIXEL_LAYOUT_I422 => 2,
        _ => 3,
    };
    let strides = [
        usize::try_from(picture.stride[0]).ok()?,
        usize::try_from(picture.stride[1]).ok()?,
    ];
    let chroma_rows = match layout {
        0 => 0,
        1 => h.div_ceil(2),
        _ => h,
    };
    let plane = |i: usize, rows: usize, stride: usize| -> &[u8] {
        match picture.data[i] {
            // SAFETY: the decoder's plane `i` is `rows` rows of `stride`
            // bytes, valid while the picture is held (the caller's promise).
            Some(at) if rows > 0 => unsafe {
                std::slice::from_raw_parts(at.as_ptr().cast::<u8>(), rows * stride)
            },
            _ => &[],
        }
    };
    // The colour matrix: BT.601 for 5 and 6 (as standard definition is), else BT.709.
    // SAFETY: the sequence header, where given, lives as long as the picture.
    let header = picture.seq_hdr.map(|s| unsafe { s.as_ref() });
    let bt601 = header.is_some_and(|s| matches!(s.mtrx, 5 | 6));
    let full_range = header.is_some_and(|s| s.color_range != 0);
    Some(Picture {
        width: w,
        height: h,
        bits,
        layout,
        planes: [
            plane(0, h, strides[0]),
            plane(1, chroma_rows, strides[1]),
            plane(2, chroma_rows, strides[1]),
        ],
        strides,
        bt601,
        full_range,
        timestamp: picture.m.timestamp,
    })
}
