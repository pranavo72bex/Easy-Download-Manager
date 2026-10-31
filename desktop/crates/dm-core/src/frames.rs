//! Pictures of a video, for a part saved as GIF: its H.264 samples decoded
//! by the system's own decoder (VideoToolbox on macOS, called directly),
//! a few a second, made small as each is decoded. Elsewhere there is none
//! dm can use yet, and that is said in words.

use std::path::Path;

use crate::gif::Frame;

/// Pictures `fps` a second of the MP4 at `path`, at most `width` wide,
/// within the part its edit list plays: their size, and the pictures.
pub fn read(path: &Path, fps: u32, width: u32) -> Result<(u16, u16, Vec<Frame>), String> {
    #[cfg(target_os = "macos")]
    {
        mac::read(path, fps.max(1), width.max(16))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (path, fps, width);
        Err(
            "saving as GIF needs the system's video decoder, which dm uses on macOS only so far"
                .into(),
        )
    }
}

/// RGB at `w` by `h` from BGRA rows (`stride` bytes each) of the rectangle
/// `(x, y, cw, ch)`: each output pixel the average of the source pixels it
/// covers, so small text and edges do not shimmer.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
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
        // (12 a second is 8, 8, 9, ...).
        let frames = std::mem::take(&mut held.ticks)
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
}
