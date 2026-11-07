// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The pictures of a part decoded by Media Foundation's H.264 decoder, which
//! dm feeds itself as VideoToolbox is fed on macOS: the samples dm's own
//! MP4 reader finds (`mp4mux::tracks_in_file`: where each is, when it plays,
//! the part's edit list), turned from the file's length-prefixed form into
//! the decoder's start-code form, each carrying its own media time. The
//! decoder gives NV12 pictures back with those times; the earliest of each
//! tick of the GIF is turned into RGB and made small. (Media Foundation's
//! own MP4 reader plays no edit list that starts after zero and stops where
//! the movie's duration says, in the media's time, so a cut part lost its
//! last pictures through it.) A Windows "N" edition has no H.264 decoder
//! until the Media Feature Pack is added; the read then fails in words and
//! `gif::possible` says no beforehand.

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::mem::ManuallyDrop;
use std::path::Path;

use windows::Win32::Media::MediaFoundation::{
    IMFSample, IMFTransform, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
    MFMediaType_Video, MFVideoFormat_H264, MFVideoFormat_NV12, MFT_MESSAGE_COMMAND_DRAIN,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
    MF_E_NOTACCEPTING, MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE,
    MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE,
    MF_MT_MINIMUM_DISPLAY_APERTURE, MF_MT_SUBTYPE, MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_YUV_MATRIX,
};

use super::{shrink, Frame};
use crate::mf;

/// `MFT_OUTPUT_STREAM_PROVIDES_SAMPLES` and `..._CAN_PROVIDE_SAMPLES`: the
/// decoder makes the buffers its pictures are in.
const PROVIDES_SAMPLES: u32 = 0x100 | 0x200;

/// Where the decoder's NV12 pictures are and how to turn them to RGB.
#[derive(Clone, Copy)]
struct Geometry {
    /// Bytes from one row to the next, and rows in the luma plane.
    stride: usize,
    rows: usize,
    /// The picture as shown (x, y, width, height): the stream's crop.
    crop: (usize, usize, usize, usize),
    bt709: bool,
    full_range: bool,
}

/// What the read keeps: one picture per tick of the GIF.
struct Collected {
    start: f64,
    end: f64,
    fps: f64,
    width: usize,
    size: Option<(usize, usize)>,
    ticks: BTreeMap<u64, (f64, Vec<u8>)>,
}

/// The decoder and what it has given so far.
struct Run<'a> {
    decoder: &'a IMFTransform,
    geometry: Geometry,
    /// Bytes an output buffer needs, where the decoder does not make them.
    buffer_size: u32,
    provides: bool,
    held: Collected,
}

pub(super) fn read(path: &Path, fps: u32, width: u32) -> Result<(u16, u16, Vec<Frame>), String> {
    let tracks = crate::mp4mux::tracks_in_file(path)?;
    let video = tracks
        .into_iter()
        .find(|t| t.hdlr.get(16..20) == Some(b"vide"))
        .ok_or("the part has no picture")?;
    let (sets, nal_length) = parameter_sets(&video.stsd)?;
    let scale = f64::from(video.timescale.max(1));
    // The part the edit list plays, in the track's time.
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
    // The stream's parameter sets, in the decoder's form, before each
    // picture it can start from.
    let mut headers = Vec::new();
    for set in &sets {
        headers.extend_from_slice(&[0, 0, 0, 1]);
        headers.extend_from_slice(set);
    }
    let coded = (
        video
            .stsd
            .get(48..50)
            .map_or(0, |b| usize::from(u16::from_be_bytes([b[0], b[1]]))),
        video
            .stsd
            .get(50..52)
            .map_or(0, |b| usize::from(u16::from_be_bytes([b[0], b[1]]))),
    );

    let _on = mf::Started::new()?;
    let decoder = mf::h264_transform()?;
    let sorry = |e: windows::core::Error| format!("the system's video decoder failed ({e})");
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    // SAFETY: every COM object here is released when dropped; the out
    // pointers are valid for each call; a picture's bytes are read only
    // between the buffer's lock and unlock, within the length it reports.
    unsafe {
        let input = MFCreateMediaType().map_err(sorry)?;
        input
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(sorry)?;
        input
            .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
            .map_err(sorry)?;
        input.SetUINT32(&MF_MT_INTERLACE_MODE, 2).map_err(sorry)?;
        if coded.0 > 0 && coded.1 > 0 {
            input
                .SetUINT64(&MF_MT_FRAME_SIZE, ((coded.0 as u64) << 32) | coded.1 as u64)
                .map_err(sorry)?;
        }
        decoder
            .SetInputType(0, &input, 0)
            .map_err(|e| format!("the system's video decoder would not take this picture ({e})"))?;
        let geometry = output_type(&decoder)?;
        let info = decoder.GetOutputStreamInfo(0).map_err(sorry)?;
        let mut run = Run {
            decoder: &decoder,
            geometry,
            buffer_size: info.cbSize,
            provides: info.dwFlags & PROVIDES_SAMPLES != 0,
            held: Collected {
                start,
                end,
                fps: f64::from(fps),
                width: width as usize,
                size: None,
                ticks: BTreeMap::new(),
            },
        };
        decoder
            .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
            .map_err(sorry)?;
        decoder
            .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
            .map_err(sorry)?;

        let mut decode_time = video.start.unwrap_or(0) as i64;
        for sample in &video.samples {
            let pts = decode_time + i64::from(sample.cts);
            let dts = decode_time;
            decode_time += i64::from(sample.duration);
            // Past the part's end by more than the decoder's look-ahead.
            if dts as f64 / scale > end + 2.0 {
                break;
            }
            let mut bytes = vec![0u8; sample.size as usize];
            if file.seek(SeekFrom::Start(sample.offset)).is_err()
                || file.read_exact(&mut bytes).is_err()
            {
                return Err("the part could not be read".into());
            }
            let data = start_codes(&bytes, nal_length, sample.sync.then_some(&headers[..]));
            let time = (pts as f64 / scale * 1e7) as i64;
            let long = (f64::from(sample.duration) / scale * 1e7) as i64;
            let given = input_sample(&data, time, long)?;
            loop {
                match decoder.ProcessInput(0, &given, 0) {
                    Ok(()) => break,
                    // Its pictures are to be taken before it takes more.
                    Err(e) if e.code() == MF_E_NOTACCEPTING => run.pictures()?,
                    Err(e) => return Err(sorry(e)),
                }
            }
            run.pictures()?;
        }
        decoder
            .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)
            .map_err(sorry)?;
        run.pictures()?;
        let (w, h) = run
            .held
            .size
            .ok_or("no picture of the part could be decoded")?;
        // Hundredths of a second each, rounded so the whole keeps time
        // (12 a second is 8, 8, 9, ...).
        let frames = std::mem::take(&mut run.held.ticks)
            .into_values()
            .enumerate()
            .map(|(i, (_, rgb))| {
                let at = |n: usize| (n as u64 * 100 + u64::from(fps) / 2) / u64::from(fps);
                let delay = (at(i + 1) - at(i)).max(2) as u16;
                Frame { rgb, delay }
            })
            .collect::<Vec<_>>();
        Ok((w as u16, h as u16, frames))
    }
}

impl Run<'_> {
    /// Takes every picture the decoder has ready.
    ///
    /// # Safety
    ///
    /// The decoder must be live and its types set, as `read` leaves them.
    unsafe fn pictures(&mut self) -> Result<(), String> {
        let sorry = |e: windows::core::Error| format!("the system's video decoder failed ({e})");
        loop {
            let mut made: Option<IMFSample> = None;
            if !self.provides {
                let sample = MFCreateSample().map_err(sorry)?;
                let buffer = MFCreateMemoryBuffer(self.buffer_size.max(1)).map_err(sorry)?;
                sample.AddBuffer(&buffer).map_err(sorry)?;
                made = Some(sample);
            }
            let mut out = MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(made),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            };
            let mut status = 0u32;
            let result = self
                .decoder
                .ProcessOutput(0, std::slice::from_mut(&mut out), &mut status);
            // What came back, or what was given and not used, is released
            // here either way.
            let back = ManuallyDrop::take(&mut out.pSample);
            ManuallyDrop::drop(&mut out.pEvents);
            match result {
                Ok(()) => {
                    if let Some(sample) = back {
                        self.take(&sample);
                    }
                }
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(()),
                Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                    self.geometry = output_type(self.decoder)?;
                    let info = self.decoder.GetOutputStreamInfo(0).map_err(sorry)?;
                    self.buffer_size = info.cbSize;
                    self.provides = info.dwFlags & PROVIDES_SAMPLES != 0;
                }
                Err(e) => return Err(sorry(e)),
            }
        }
    }

    /// Keeps `sample`'s picture if it is the earliest of its tick within
    /// the part.
    ///
    /// # Safety
    ///
    /// `sample` must hold an NV12 picture as the current output type says.
    unsafe fn take(&mut self, sample: &IMFSample) {
        let Ok(stamp) = sample.GetSampleTime() else {
            return;
        };
        let at = stamp as f64 / 1e7;
        let held = &mut self.held;
        if at < held.start - 1e-6 || at >= held.end {
            return;
        }
        let tick = ((at - held.start) * held.fps).floor().max(0.0) as u64;
        if held.ticks.get(&tick).is_some_and(|(t, _)| *t <= at) {
            return;
        }
        let Ok(buffer) = sample.ConvertToContiguousBuffer() else {
            return;
        };
        let mut data = std::ptr::null_mut::<u8>();
        let mut length = 0u32;
        if buffer.Lock(&mut data, None, Some(&mut length)).is_err() {
            return;
        }
        if !data.is_null() {
            let bytes = std::slice::from_raw_parts(data, length as usize);
            if let Some(bgra) = to_bgra(bytes, &self.geometry) {
                let (_, _, cw, ch) = self.geometry.crop;
                // At most `width` across and as tall, its shape kept.
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
                let rgb = shrink(&bgra, cw * 4, (0, 0, cw, ch), w, h);
                held.ticks.insert(tick, (at, rgb));
            }
        }
        let _ = buffer.Unlock();
    }
}

/// Asks the decoder for NV12 pictures and reads where they will be.
///
/// # Safety
///
/// The decoder must be live with its input type set.
unsafe fn output_type(decoder: &IMFTransform) -> Result<Geometry, String> {
    let sorry = |e: windows::core::Error| format!("the system's video decoder failed ({e})");
    let mut index = 0;
    loop {
        let kind = decoder
            .GetOutputAvailableType(0, index)
            .map_err(|_| "the system's video decoder gives no picture dm can read".to_string())?;
        if kind.GetGUID(&MF_MT_SUBTYPE).ok() == Some(MFVideoFormat_NV12) {
            decoder.SetOutputType(0, &kind, 0).map_err(sorry)?;
            break;
        }
        index += 1;
    }
    let current = decoder.GetOutputCurrentType(0).map_err(sorry)?;
    let packed = current.GetUINT64(&MF_MT_FRAME_SIZE).map_err(sorry)?;
    let (fw, fh) = ((packed >> 32) as usize, (packed & 0xFFFF_FFFF) as usize);
    if fw == 0 || fh == 0 {
        return Err("the picture has no size".into());
    }
    let stride = current
        .GetUINT32(&MF_MT_DEFAULT_STRIDE)
        .map_or(fw, |s| (s as i32).unsigned_abs() as usize)
        .max(fw);
    // The picture as shown: `MFVideoArea`, its offsets and its size.
    let mut area = [0u8; 16];
    let crop = if current
        .GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, &mut area, None)
        .is_ok()
    {
        let at = |i: usize| i16::from_le_bytes([area[i], area[i + 1]]).max(0) as usize;
        let size = |i: usize| i32::from_le_bytes([area[i], area[i + 1], area[i + 2], area[i + 3]]);
        let (x, y) = (at(2), at(6));
        let (w, h) = (size(8).max(0) as usize, size(12).max(0) as usize);
        if w > 0 && h > 0 && x + w <= fw && y + h <= fh {
            (x, y, w, h)
        } else {
            (0, 0, fw, fh)
        }
    } else {
        (0, 0, fw, fh)
    };
    // The stream's own colour matrix and range where it says; else
    // high definition is BT.709 and the rest BT.601, narrow range.
    let bt709 = match current.GetUINT32(&MF_MT_YUV_MATRIX).ok() {
        Some(1) => true,
        Some(2) => false,
        _ => fh >= 720,
    };
    let full_range = current.GetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE).ok() == Some(2);
    Ok(Geometry {
        stride,
        rows: fh,
        crop,
        bt709,
        full_range,
    })
}

/// A sample of the decoder's input holding `data`, playing at `time` for
/// `long` (100-nanosecond units).
///
/// # Safety
///
/// Calls into Media Foundation, started by the caller.
unsafe fn input_sample(data: &[u8], time: i64, long: i64) -> Result<IMFSample, String> {
    let sorry = |e: windows::core::Error| format!("the system's video decoder failed ({e})");
    let sample = MFCreateSample().map_err(sorry)?;
    let buffer = MFCreateMemoryBuffer(data.len() as u32).map_err(sorry)?;
    let mut at = std::ptr::null_mut::<u8>();
    buffer.Lock(&mut at, None, None).map_err(sorry)?;
    if !at.is_null() {
        std::ptr::copy_nonoverlapping(data.as_ptr(), at, data.len());
    }
    buffer.Unlock().map_err(sorry)?;
    buffer.SetCurrentLength(data.len() as u32).map_err(sorry)?;
    sample.AddBuffer(&buffer).map_err(sorry)?;
    sample.SetSampleTime(time).map_err(sorry)?;
    sample.SetSampleDuration(long).map_err(sorry)?;
    Ok(sample)
}

/// An `avc1` sample (each NAL unit behind its length) as the decoder takes
/// it: each behind a start code, the parameter sets first where given.
fn start_codes(sample: &[u8], nal_length: usize, headers: Option<&[u8]>) -> Vec<u8> {
    let mut out = Vec::with_capacity(sample.len() + 64);
    if let Some(headers) = headers {
        out.extend_from_slice(headers);
    }
    let mut at = 0;
    while at + nal_length <= sample.len() {
        let len = sample[at..at + nal_length]
            .iter()
            .fold(0usize, |n, b| (n << 8) | usize::from(*b));
        at += nal_length;
        let Some(nal) = sample.get(at..at + len) else {
            break;
        };
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(nal);
        at += len;
    }
    out
}

/// The shown part of an NV12 picture as BGRA rows, top first.
fn to_bgra(data: &[u8], g: &Geometry) -> Option<Vec<u8>> {
    let (x0, y0, cw, ch) = g.crop;
    let chroma = g.stride * g.rows;
    if data.len() < chroma + g.stride * g.rows.div_ceil(2) {
        return None;
    }
    // Fixed-point (8 bits) coefficients: red from V, green from U and V, blue
    // from U; narrow range scales luma by 298/256 after taking off 16.
    let (rv, gu, gv, bu) = match (g.bt709, g.full_range) {
        (false, false) => (409, 100, 208, 516),
        (true, false) => (459, 55, 136, 541),
        (false, true) => (359, 88, 183, 454),
        (true, true) => (403, 48, 120, 475),
    };
    let mut out = Vec::with_capacity(cw * ch * 4);
    for y in 0..ch {
        let sy = y0 + y;
        let luma = &data[sy * g.stride..];
        let color = &data[chroma + (sy / 2) * g.stride..];
        for x in 0..cw {
            let sx = x0 + x;
            let yy = i32::from(luma[sx]);
            let u = i32::from(color[sx & !1]) - 128;
            let v = i32::from(color[(sx & !1) + 1]) - 128;
            let c = if g.full_range {
                yy * 256
            } else {
                (yy - 16) * 298
            };
            let clamp = |n: i32| n.clamp(0, 255) as u8;
            out.extend_from_slice(&[
                clamp((c + bu * u + 128) >> 8),
                clamp((c - gu * u - gv * v + 128) >> 8),
                clamp((c + rv * v + 128) >> 8),
                255,
            ]);
        }
    }
    Some(out)
}

/// The SPS and PPS of an `avc1`/`avc3` sample description (`stsd`, a
/// whole box), and how many bytes each sample's NAL lengths take.
fn parameter_sets(stsd: &[u8]) -> Result<(Vec<Vec<u8>>, usize), String> {
    let entry = stsd.get(16..).ok_or("no picture description")?;
    let kind = entry.get(4..8).ok_or("no picture description")?;
    if kind != b"avc1" && kind != b"avc3" {
        return Err(format!(
            "only an H.264 picture can be made a GIF; this one is {}",
            String::from_utf8_lossy(kind)
        ));
    }
    let size = u32::from_be_bytes(
        entry
            .get(..4)
            .ok_or("cut short")?
            .try_into()
            .map_err(|_| "cut short")?,
    ) as usize;
    let mut at = 86;
    while at + 8 <= size.min(entry.len()) {
        let len =
            u32::from_be_bytes(entry[at..at + 4].try_into().map_err(|_| "cut short")?) as usize;
        if len < 8 {
            break;
        }
        if &entry[at + 4..at + 8] == b"avcC" {
            let avcc = entry.get(at + 8..at + len).ok_or("cut short")?;
            let nal_length = usize::from(avcc.get(4).ok_or("cut short")? & 3) + 1;
            let mut sets = Vec::new();
            let mut p = 5;
            for kind in 0..2 {
                let count = usize::from(*avcc.get(p).ok_or("cut short")?)
                    & if kind == 0 { 0x1f } else { 0xff };
                p += 1;
                for _ in 0..count {
                    let n = usize::from(u16::from_be_bytes([
                        *avcc.get(p).ok_or("cut short")?,
                        *avcc.get(p + 1).ok_or("cut short")?,
                    ]));
                    sets.push(avcc.get(p + 2..p + 2 + n).ok_or("cut short")?.to_vec());
                    p += 2 + n;
                }
            }
            return Ok((sets, nal_length));
        }
        at += len;
    }
    Err("the picture has no H.264 settings (avcC)".into())
}
