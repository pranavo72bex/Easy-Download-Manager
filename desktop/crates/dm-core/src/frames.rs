// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Pictures of a video, for a part saved as GIF: its H.264 samples decoded
//! by the system's own decoder (VideoToolbox on macOS, called directly; on
//! Windows Media Foundation's source reader, `frames/win.rs`), a few a
//! second, made small as each is decoded. On Linux, which has no decoder dm
//! can use, the part is taken from the video's AV1 stream and decoded by
//! rav1d (`frames/av1.rs`).

use std::path::Path;

use crate::gif::Frame;

#[cfg(any(target_os = "linux", test))]
mod av1;
#[cfg(windows)]
mod win;

/// Pictures `fps` a second of the MP4 at `path`, at most `width` wide,
/// within the part its edit list plays: their size, and the pictures.
pub fn read(path: &Path, fps: u32, width: u32) -> Result<(u16, u16, Vec<Frame>), String> {
    #[cfg(target_os = "macos")]
    {
        mac::read(path, fps.max(1), width.max(16))
    }
    #[cfg(windows)]
    {
        win::read(path, fps.max(1), width.max(16))
    }
    #[cfg(target_os = "linux")]
    {
        av1::read(path, fps.max(1), width.max(16))
    }
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        let _ = (path, fps, width);
        Err(
            "saving as GIF needs the system's video decoder, which dm uses on macOS and Windows only so far"
                .into(),
        )
    }
}

/// RGB at `w` by `h` from BGRA rows (`stride` bytes each) of the rectangle
/// `(x, y, cw, ch)`: each output pixel the average of the source pixels it
/// covers, so small text and edges do not shimmer.
#[cfg_attr(
    not(any(target_os = "macos", windows, target_os = "linux", test)),
    allow(dead_code)
)]
fn shrink(
    bgra: &[u8],
    stride: usize,
    rect: (usize, usize, usize, usize),
    w: usize,
    h: usize,
) -> Vec<u8> {
    let (x0, y0, cw, ch) = rect;
    let mut out = Vec::with_capacity(w * h * 3);
    for oy in 0..h {
        let (sy0, sy1) = (
            y0 + oy * ch / h,
            (y0 + (oy + 1) * ch / h).max(y0 + oy * ch / h + 1),
        );
        for ox in 0..w {
            let (sx0, sx1) = (
                x0 + ox * cw / w,
                (x0 + (ox + 1) * cw / w).max(x0 + ox * cw / w + 1),
            );
            let mut sum = [0u32; 3];
            let mut n = 0u32;
            for sy in sy0..sy1 {
                for sx in sx0..sx1 {
                    let at = sy * stride + sx * 4;
                    if let Some(px) = bgra.get(at..at + 3) {
                        sum[0] += u32::from(px[2]);
                        sum[1] += u32::from(px[1]);
                        sum[2] += u32::from(px[0]);
                        n += 1;
                    }
                }
            }
            let n = n.max(1);
            out.extend_from_slice(&[(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8]);
        }
    }
    out
}

/// The part of a picture track that plays, in seconds of its own time: what its first edit names,
/// or, with no edit list or an edit of no length, every picture from the first shown to the end of
/// the last (by when each is shown, its decode time and its composition offset: a stream with
/// B-frames shows its first picture after its first decode). Taken as "to the end of time" before,
/// every such video was longer than a GIF may be and refused, a one-second one too (coverage
/// tests, 2026-11-08).
pub(crate) fn played(video: &crate::mp4mux::Track) -> (f64, f64) {
    let scale = f64::from(video.timescale.max(1));
    let mut decoded = i64::try_from(video.start.unwrap_or(0)).unwrap_or(i64::MAX);
    let (mut first, mut last) = (i64::MAX, i64::MIN);
    for sample in &video.samples {
        let shown = decoded.saturating_add(i64::from(sample.cts));
        first = first.min(shown);
        last = last.max(shown.saturating_add(i64::from(sample.duration)));
        decoded = decoded.saturating_add(i64::from(sample.duration));
    }
    let (first, last) = if video.samples.is_empty() {
        (0.0, 0.0)
    } else {
        (first as f64 / scale, last as f64 / scale)
    };
    match video.edits.iter().find(|e| e.media_time >= 0) {
        Some(edit) => {
            let from = edit.media_time as f64 / scale;
            let long = edit.duration as f64 / f64::from(video.edits_timescale.max(1));
            (
                from,
                if long > 0.0 {
                    from + long
                } else {
                    last.max(from)
                },
            )
        }
        None => (first, last),
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::{shrink, Frame};
    use std::collections::BTreeMap;
    use std::ffi::c_void;
    use std::io::{Read, Seek, SeekFrom};
    use std::path::Path;

    type OsStatus = i32;
    type Ref = *const c_void;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CmTime {
        value: i64,
        timescale: i32,
        flags: u32,
        epoch: i64,
    }

    #[repr(C)]
    struct TimingInfo {
        duration: CmTime,
        presentation: CmTime,
        decode: CmTime,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CgRect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    }

    type Callback = extern "C" fn(*mut c_void, *mut c_void, OsStatus, u32, Ref, CmTime, CmTime);

    #[repr(C)]
    struct CallbackRecord {
        callback: Callback,
        refcon: *mut c_void,
    }

    #[link(name = "CoreMedia", kind = "framework")]
    extern "C" {
        fn CMVideoFormatDescriptionCreateFromH264ParameterSets(
            allocator: Ref,
            count: usize,
            pointers: *const *const u8,
            sizes: *const usize,
            nal_length: i32,
            out: *mut Ref,
        ) -> OsStatus;
        fn CMBlockBufferCreateWithMemoryBlock(
            allocator: Ref,
            memory: *mut c_void,
            length: usize,
            block_allocator: Ref,
            source: *const c_void,
            offset: usize,
            data_length: usize,
            flags: u32,
            out: *mut Ref,
        ) -> OsStatus;
        fn CMSampleBufferCreateReady(
            allocator: Ref,
            data: Ref,
            format: Ref,
            samples: isize,
            timings: isize,
            timing: *const TimingInfo,
            sizes_count: isize,
            sizes: *const usize,
            out: *mut Ref,
        ) -> OsStatus;
    }

    #[link(name = "VideoToolbox", kind = "framework")]
    extern "C" {
        fn VTDecompressionSessionCreate(
            allocator: Ref,
            format: Ref,
            spec: Ref,
            attributes: Ref,
            callback: *const CallbackRecord,
            out: *mut Ref,
        ) -> OsStatus;
        fn VTDecompressionSessionDecodeFrame(
            session: Ref,
            sample: Ref,
            flags: u32,
            refcon: *mut c_void,
            info: *mut u32,
        ) -> OsStatus;
        fn VTDecompressionSessionWaitForAsynchronousFrames(session: Ref) -> OsStatus;
        fn VTDecompressionSessionInvalidate(session: Ref);
    }

    #[link(name = "CoreVideo", kind = "framework")]
    extern "C" {
        static kCVPixelBufferPixelFormatTypeKey: Ref;
        fn CVPixelBufferLockBaseAddress(buffer: Ref, flags: u64) -> i32;
        fn CVPixelBufferUnlockBaseAddress(buffer: Ref, flags: u64) -> i32;
        fn CVPixelBufferGetBaseAddress(buffer: Ref) -> *const u8;
        fn CVPixelBufferGetBytesPerRow(buffer: Ref) -> usize;
        fn CVPixelBufferGetWidth(buffer: Ref) -> usize;
        fn CVPixelBufferGetHeight(buffer: Ref) -> usize;
        fn CVImageBufferGetCleanRect(buffer: Ref) -> CgRect;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFAllocatorNull: Ref;
        static kCFTypeDictionaryKeyCallBacks: c_void;
        static kCFTypeDictionaryValueCallBacks: c_void;
        fn CFNumberCreate(allocator: Ref, kind: isize, value: *const c_void) -> Ref;
        fn CFDictionaryCreate(
            allocator: Ref,
            keys: *const Ref,
            values: *const Ref,
            count: isize,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> Ref;
        fn CFRelease(object: Ref);
    }

    /// What the decoder's callback fills: one picture per tick of the GIF,
    /// the earliest that falls in it.
    struct Collected {
        start: f64,
        end: f64,
        fps: f64,
        width: usize,
        size: Option<(usize, usize)>,
        ticks: BTreeMap<u64, (f64, Vec<u8>)>,
    }

    extern "C" fn decoded(
        refcon: *mut c_void,
        _frame: *mut c_void,
        status: OsStatus,
        _flags: u32,
        image: Ref,
        pts: CmTime,
        _duration: CmTime,
    ) {
        if status != 0 || image.is_null() || pts.timescale <= 0 {
            return;
        }
        // SAFETY: `refcon` is the `Collected` that `read` passed and keeps
        // alive until the session is invalidated; the image buffer is the
        // decoder's, valid for this call, and its pixels are read only
        // between lock and unlock, within its rows.
        unsafe {
            let held = &mut *refcon.cast::<Collected>();
            let at = pts.value as f64 / f64::from(pts.timescale);
            if at < held.start - 1e-6 || at >= held.end {
                return;
            }
            let tick = ((at - held.start) * held.fps).floor().max(0.0) as u64;
            if held.ticks.get(&tick).is_some_and(|(t, _)| *t <= at) {
                return;
            }
            if CVPixelBufferLockBaseAddress(image, 1) != 0 {
                return;
            }
            let base = CVPixelBufferGetBaseAddress(image);
            let stride = CVPixelBufferGetBytesPerRow(image);
            let (bw, bh) = (CVPixelBufferGetWidth(image), CVPixelBufferGetHeight(image));
            // The picture as shown: the stream's crop (`crate::shape`).
            let clean = CVImageBufferGetCleanRect(image);
            let (cw, ch) = (
                (clean.width as usize).clamp(1, bw),
                (clean.height as usize).clamp(1, bh),
            );
            let x0 = (clean.x.max(0.0) as usize).min(bw - cw);
            let y0 = (clean.y.max(0.0) as usize).min(bh - ch);
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
            if !base.is_null() {
                let bytes = std::slice::from_raw_parts(base, stride * bh);
                let rgb = shrink(bytes, stride, (x0, y0, cw, ch), w, h);
                held.ticks.insert(tick, (at, rgb));
            }
            CVPixelBufferUnlockBaseAddress(image, 1);
        }
    }

    pub(super) fn read(
        path: &Path,
        fps: u32,
        width: u32,
    ) -> Result<(u16, u16, Vec<Frame>), String> {
        let tracks = crate::mp4mux::tracks_in_file(path)?;
        let video = tracks
            .into_iter()
            .find(|t| t.hdlr.get(16..20) == Some(b"vide"))
            .ok_or("the part has no picture")?;
        let (sets, nal_length) = parameter_sets(&video.stsd)?;
        let scale = f64::from(video.timescale.max(1));
        // The part the edit list plays, in the track's time (`played`).
        let (start, end) = super::played(&video);
        if end - start > 120.0 {
            return Err("a GIF is made of two minutes at most; choose a shorter part".into());
        }
        let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut held = Box::new(Collected {
            start,
            end,
            fps: f64::from(fps),
            width: width as usize,
            size: None,
            ticks: BTreeMap::new(),
        });
        // SAFETY: every Core Foundation object made here is released on
        // every path; the sample's bytes outlive the decode of it, which is
        // synchronous (flags 0); `held` outlives the session.
        unsafe {
            let pointers: Vec<*const u8> = sets.iter().map(|s| s.as_ptr()).collect();
            let sizes: Vec<usize> = sets.iter().map(Vec::len).collect();
            let mut format: Ref = std::ptr::null();
            let status = CMVideoFormatDescriptionCreateFromH264ParameterSets(
                std::ptr::null(),
                sets.len(),
                pointers.as_ptr(),
                sizes.as_ptr(),
                nal_length,
                &mut format,
            );
            if status != 0 {
                return Err(format!("the picture's settings were not taken ({status})"));
            }
            let bgra = u32::from_be_bytes(*b"BGRA");
            let number = CFNumberCreate(std::ptr::null(), 3, (&bgra as *const u32).cast());
            let key = kCVPixelBufferPixelFormatTypeKey;
            let attributes = CFDictionaryCreate(
                std::ptr::null(),
                &key,
                &number,
                1,
                std::ptr::addr_of!(kCFTypeDictionaryKeyCallBacks),
                std::ptr::addr_of!(kCFTypeDictionaryValueCallBacks),
            );
            CFRelease(number);
            let record = CallbackRecord {
                callback: decoded,
                refcon: (&mut *held as *mut Collected).cast(),
            };
            let mut session: Ref = std::ptr::null();
            let status = VTDecompressionSessionCreate(
                std::ptr::null(),
                format,
                std::ptr::null(),
                attributes,
                &record,
                &mut session,
            );
            CFRelease(attributes);
            if status != 0 {
                CFRelease(format);
                return Err(format!(
                    "the system's video decoder would not start ({status})"
                ));
            }
            let mut decode_time = video.start.unwrap_or(0) as i64;
            let mut failure = None;
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
                    failure = Some("the part could not be read".to_string());
                    break;
                }
                let mut block: Ref = std::ptr::null();
                if CMBlockBufferCreateWithMemoryBlock(
                    std::ptr::null(),
                    bytes.as_mut_ptr().cast(),
                    bytes.len(),
                    kCFAllocatorNull,
                    std::ptr::null(),
                    0,
                    bytes.len(),
                    0,
                    &mut block,
                ) != 0
                {
                    continue;
                }
                let time = |v: i64| CmTime {
                    value: v,
                    timescale: video.timescale as i32,
                    flags: 1,
                    epoch: 0,
                };
                let timing = TimingInfo {
                    duration: time(i64::from(sample.duration)),
                    presentation: time(pts),
                    decode: time(dts),
                };
                let size = bytes.len();
                let mut buffer: Ref = std::ptr::null();
                let made = CMSampleBufferCreateReady(
                    std::ptr::null(),
                    block,
                    format,
                    1,
                    1,
                    &timing,
                    1,
                    &size,
                    &mut buffer,
                );
                CFRelease(block);
                if made != 0 {
                    continue;
                }
                let mut info = 0u32;
                let _ = VTDecompressionSessionDecodeFrame(
                    session,
                    buffer,
                    0,
                    std::ptr::null_mut(),
                    &mut info,
                );
                CFRelease(buffer);
            }
            VTDecompressionSessionWaitForAsynchronousFrames(session);
            VTDecompressionSessionInvalidate(session);
            CFRelease(session);
            CFRelease(format);
            if let Some(why) = failure {
                return Err(why);
            }
        }
        let (w, h) = held.size.ok_or("no picture of the part could be decoded")?;
        // Hundredths of a second each, rounded so the whole keeps time
        // (12 a second is 8, 8, 9, ...), and held until the next picture's
        // tick: a video with fewer pictures a second than asked skips ticks,
        // and timed by its place in the list it played fast (a second of 10
        // a second asked at 12 lasted 0.83 s; coverage tests, 2026-11-08).
        let ticks: Vec<_> = std::mem::take(&mut held.ticks).into_iter().collect();
        let at = |n: u64| (n * 100 + u64::from(fps) / 2) / u64::from(fps);
        // The last is held to the end of the part it plays, not one tick.
        let span = (held.end - held.start) * f64::from(fps);
        let end = if span.is_finite() {
            span.ceil().max(0.0) as u64
        } else {
            0
        };
        let frames = ticks
            .iter()
            .enumerate()
            .map(|(i, (tick, (_, rgb)))| {
                let next = ticks.get(i + 1).map_or((tick + 1).max(end), |(t, _)| *t);
                let delay = (at(next) - at(*tick)).max(2) as u16;
                Frame {
                    rgb: rgb.clone(),
                    delay,
                }
            })
            .collect::<Vec<_>>();
        Ok((w as u16, h as u16, frames))
    }

    /// The SPS and PPS of an `avc1`/`avc3` sample description (`stsd`, a
    /// whole box), and how many bytes each sample's NAL lengths take.
    fn parameter_sets(stsd: &[u8]) -> Result<(Vec<Vec<u8>>, i32), String> {
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
                let nal_length = i32::from(avcc.get(4).ok_or("cut short")? & 3) + 1;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mean red and blue of an RGB picture.
    fn red_blue(rgb: &[u8]) -> (u32, u32) {
        let n = (rgb.len() / 3).max(1) as u32;
        let (r, b) = rgb.chunks(3).fold((0u32, 0u32), |(r, b), px| {
            (r + u32::from(px[0]), b + u32::from(px[2]))
        });
        (r / n, b / n)
    }

    /// An AV1 MP4 (one second red, one blue, made by an AV1 encoder) read by
    /// dm's AV1 reader, the Linux route to a GIF (gaps row 54): twelve
    /// pictures a second, its size kept, red then blue.
    #[test]
    fn an_av1_video_is_read_to_pictures_red_then_blue() {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mp4/red-blue.av1.mp4");
        let (w, h, frames) = av1::read(&fixture, 12, 480).unwrap();
        assert_eq!((w, h), (320, 180));
        assert!(
            (22..=25).contains(&frames.len()),
            "{} pictures",
            frames.len()
        );
        for frame in &frames {
            assert_eq!(frame.rgb.len(), 320 * 180 * 3);
        }
        let (r, b) = red_blue(&frames[1].rgb);
        assert!(
            r > 150 && b < 90,
            "the first second is not red: r {r}, b {b}"
        );
        let (r, b) = red_blue(&frames[frames.len() - 2].rgb);
        assert!(
            b > 150 && r < 90,
            "the second second is not blue: r {r}, b {b}"
        );
        // Smaller where asked: at most 160 across, its shape kept.
        let (w, h, _) = av1::read(&fixture, 12, 160).unwrap();
        assert_eq!((w, h), (160, 90));
        // And written as a GIF.
        let gif = crate::gif::encode(320, 180, &frames).unwrap();
        assert_eq!(&gif[..6], b"GIF89a");
    }

    /// A part cut by dm's own cutter (an edit list over the samples) gives
    /// the pictures of its span alone: from half a second, a second long, so
    /// red for its first half and blue after.
    #[test]
    fn a_cut_av1_part_gives_the_pictures_of_its_span_only() {
        let dir = tempfile::tempdir().unwrap();
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mp4/red-blue.av1.mp4");
        let part = dir.path().join("part.mp4");
        crate::mp4mux::join_cut(&[fixture], &[], &part, Some((500, 1500)), None).unwrap();
        let (_, _, frames) = av1::read(&part, 12, 480).unwrap();
        assert!(
            (10..=13).contains(&frames.len()),
            "{} pictures",
            frames.len()
        );
        let (r, _) = red_blue(&frames[0].rgb);
        assert!(r > 150, "the part does not start red: r {r}");
        let (_, b) = red_blue(&frames[frames.len() - 1].rgb);
        assert!(b > 150, "the part does not end blue: b {b}");
    }

    /// A picture that is not AV1 is refused in words, not decoded as noise.
    #[test]
    fn a_part_that_is_not_av1_is_refused_in_words() {
        let h264 =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mp4/picture.f137.mp4");
        let Err(said) = av1::read(&h264, 12, 480) else {
            panic!("an H.264 part was decoded as AV1");
        };
        assert!(said.contains("AV1"), "{said}");
        assert!(av1::read(Path::new("/nowhere/at/all.mp4"), 12, 480).is_err());
    }

    #[test]
    fn a_picture_shrinks_by_averaging_and_keeps_its_crop() {
        // 4x2 BGRA: left half red, right half blue; stride with padding.
        let stride = 4 * 4 + 8;
        let mut bgra = vec![0u8; stride * 2];
        for y in 0..2 {
            for x in 0..4 {
                let at = y * stride + x * 4;
                let px = if x < 2 {
                    [0, 0, 255, 255]
                } else {
                    [255, 0, 0, 255]
                };
                bgra[at..at + 4].copy_from_slice(&px);
            }
        }
        assert_eq!(
            shrink(&bgra, stride, (0, 0, 4, 2), 2, 1),
            [255, 0, 0, 0, 0, 255]
        );
        // Only the right half (a crop): all blue.
        assert_eq!(shrink(&bgra, stride, (2, 0, 2, 2), 1, 1), [0, 0, 255]);
    }

    /// The picture fixture (a real H.264 track) made into an MP4 the way a
    /// download is, read by Media Foundation and written as a GIF: the
    /// pictures have the size the GIF says, the same read gives the same
    /// GIF, and the file has its header and its end.
    #[cfg(windows)]
    #[test]
    fn a_picture_is_read_by_media_foundation_and_made_a_gif() {
        if !crate::gif::possible() {
            eprintln!("no H.264 decoder on this Windows (an N edition): nothing to check");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mp4");
        let mp4 = dir.path().join("part.mp4");
        crate::mp4mux::join(&[fixtures.join("picture.f137.mp4")], &mp4).unwrap();
        let (w, h, frames) = read(&mp4, 12, 480).unwrap();
        assert!(w > 0 && h > 0 && w <= 480, "{w} x {h}");
        assert!(!frames.is_empty(), "no picture was decoded");
        for frame in &frames {
            assert_eq!(frame.rgb.len(), usize::from(w) * usize::from(h) * 3);
            assert!(frame.delay >= 2);
        }
        let gif = dir.path().join("part.gif");
        let length = crate::gif::from_video(&mp4, &gif).unwrap();
        let bytes = std::fs::read(&gif).unwrap();
        assert_eq!(bytes.len() as u64, length);
        assert_eq!(&bytes[..6], b"GIF89a");
        assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), w);
        assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), h);
        assert_eq!(bytes.last(), Some(&0x3B), "no GIF trailer");
        assert_eq!(
            bytes,
            crate::gif::encode(w, h, &frames).unwrap(),
            "reading the same part twice gave different pictures"
        );
        // The picture's shape is the stream's: its size from the sample
        // description (`avc1`: width and height after twelve reserved
        // bytes), shrunk to at most 480 across.
        let tracks = crate::mp4mux::tracks_in_file(&mp4).unwrap();
        let video = tracks
            .iter()
            .find(|t| t.hdlr.get(16..20) == Some(b"vide"))
            .unwrap();
        let (sw, sh) = (
            u32::from(u16::from_be_bytes([video.stsd[48], video.stsd[49]])),
            u32::from(u16::from_be_bytes([video.stsd[50], video.stsd[51]])),
        );
        let (gw, gh) = (u32::from(w), u32::from(h));
        assert!(
            (gw * sh).abs_diff(gh * sw) <= sw.max(sh) * 2,
            "{gw} x {gh} for a stream of {sw} x {sh}"
        );
    }

    /// A part cut from the picture (the edit list `join_cut` writes) gives
    /// the pictures of its span only, whether Media Foundation plays the
    /// edit list itself or gives the media's own times.
    #[cfg(windows)]
    #[test]
    fn a_cut_part_gives_the_pictures_of_its_span_only() {
        if !crate::gif::possible() {
            eprintln!("no H.264 decoder on this Windows (an N edition): nothing to check");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let picture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mp4/picture.f137.mp4");
        let whole = dir.path().join("whole.mp4");
        crate::mp4mux::join(std::slice::from_ref(&picture), &whole).unwrap();
        let all = read(&whole, 12, 480).unwrap().2.len();
        let part = dir.path().join("part.mp4");
        crate::mp4mux::join_cut(&[picture], &[], &part, Some((400, 1400)), None).unwrap();
        let tracks = crate::mp4mux::tracks_in_file(&part).unwrap();
        let video = tracks
            .iter()
            .find(|t| t.hdlr.get(16..20) == Some(b"vide"))
            .unwrap();
        // The pictures the edit list plays, one per tick of the GIF, counted
        // from the samples' own times as the reader's contract says.
        let scale = f64::from(video.timescale);
        let edit = video.edits.iter().find(|e| e.media_time >= 0).unwrap();
        let from = edit.media_time as f64 / scale;
        let to = from + edit.duration as f64 / f64::from(video.edits_timescale);
        let mut decode_time = video.start.unwrap_or(0) as i64;
        let mut ticks = std::collections::BTreeSet::new();
        for sample in &video.samples {
            let at = (decode_time + i64::from(sample.cts)) as f64 / scale;
            decode_time += i64::from(sample.duration);
            if at >= from - 1e-6 && at < to {
                ticks.insert(((at - from) * 12.0).floor() as u64);
            }
        }
        let some = read(&part, 12, 480).unwrap().2.len();
        assert!(some > 0 && some < all, "{some} of {all}");
        assert_eq!(some, ticks.len(), "the span {from:.2} to {to:.2} s");
    }

    /// A file with no picture is refused in words, a sound alone, text, or
    /// a name the reader does not know the kind of; nothing panics.
    #[cfg(windows)]
    #[test]
    fn a_file_with_no_picture_is_refused_in_words() {
        let dir = tempfile::tempdir().unwrap();
        let sound = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mp4/sound.f140.m4a");
        assert!(
            read(&sound, 12, 480).is_err(),
            "a sound alone has no picture"
        );
        for name in ["not-video.mp4", "not-video.part"] {
            let source = dir.path().join(name);
            std::fs::write(&source, b"this is text, not a video").unwrap();
            let said = read(&source, 12, 480).err().expect("text is not a video");
            assert!(!said.is_empty(), "{name}");
        }
        assert!(read(&dir.path().join("missing.mp4"), 12, 480).is_err());
    }
}
