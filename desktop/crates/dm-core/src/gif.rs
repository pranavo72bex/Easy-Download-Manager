//! An animated GIF writer of dm's own, for a part of a video saved as GIF.
//!
//! Frames come in as RGB pictures of one size. One palette of 256 colours
//! is made for them all by median cut over a sample of their pixels, each
//! frame is dithered to it (Floyd–Steinberg) and compressed with GIF's LZW,
//! and the frames are written as a GIF89a that loops for ever.

/// Whether dm can make a GIF here: the system has a video decoder dm
/// reads H.264 with (macOS for now).
pub fn possible() -> bool {
    cfg!(target_os = "macos")
}

/// Frames a second, and the most pixels across, of a GIF made of a part.
const FPS: u32 = 12;
const WIDTH: u32 = 480;

/// The part of a video at `mp4` (an H.264 MP4) saved as a GIF at `target`:
/// its pictures read by the system's decoder (`crate::frames`), twelve a
/// second at most 480 wide, then written here. Returns the GIF's length.
pub fn from_video(mp4: &std::path::Path, target: &std::path::Path) -> Result<u64, String> {
    let (width, height, frames) = crate::frames::read(mp4, FPS, WIDTH)?;
    let bytes = encode(width, height, &frames)?;
    crate::write_atomic(target, &bytes)
        .map_err(|e| format!("the GIF could not be written: {e}"))?;
    Ok(bytes.len() as u64)
}

/// [`from_video`], off the async runtime.
pub async fn from_video_async(
    mp4: std::path::PathBuf,
    target: std::path::PathBuf,
) -> Result<u64, String> {
    tokio::task::spawn_blocking(move || from_video(&mp4, &target))
        .await
        .map_err(|e| format!("making the GIF stopped: {e}"))?
}

/// One picture: `width` by `height` pixels, RGB, row by row.
pub struct Frame {
    pub rgb: Vec<u8>,
    /// How long it shows, in hundredths of a second.
    pub delay: u16,
}

/// The GIF of `frames`, each `width` by `height`.
pub fn encode(width: u16, height: u16, frames: &[Frame]) -> Result<Vec<u8>, String> {
    let pixels = usize::from(width) * usize::from(height);
    if frames.is_empty() || pixels == 0 {
        return Err("there are no pictures to make a GIF of".into());
    }
    if frames.iter().any(|f| f.rgb.len() != pixels * 3) {
        return Err("the pictures are not all of one size".into());
    }
    let palette = palette_of(frames);
    let mut out = Vec::new();
    out.extend_from_slice(b"GIF89a");
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    // A global table of 256 colours (2^(7+1)).
    out.extend_from_slice(&[0xF7, 0, 0]);
    for colour in &palette {
        out.extend_from_slice(colour);
    }
    // Loop for ever (the NETSCAPE2.0 extension every viewer reads).
    out.extend_from_slice(&[0x21, 0xFF, 11]);
    out.extend_from_slice(b"NETSCAPE2.0");
    out.extend_from_slice(&[3, 1, 0, 0, 0]);
    let lookup = Lookup::new(&palette);
    for frame in frames {
        // The graphic control extension: the frame's delay.
        out.extend_from_slice(&[0x21, 0xF9, 4, 0]);
        out.extend_from_slice(&frame.delay.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        // The image, at 0,0, the whole size, the global table.
        out.push(0x2C);
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.push(0);
        let indices = dither(&frame.rgb, usize::from(width), &palette, &lookup);
        out.push(8); // the LZW minimum code size
        let data = lzw(&indices, 8);
        for block in data.chunks(255) {
            out.push(block.len() as u8);
            out.extend_from_slice(block);
        }
        out.push(0);
    }
    out.push(0x3B);
    Ok(out)
}

/// 256 colours for the frames: median cut over a sample of their pixels
/// (every one in small pictures, else every seventh: a stride that a
/// regular pattern does not line up with as a power of two does).
fn palette_of(frames: &[Frame]) -> Vec<[u8; 3]> {
    let step = (frames.len() / 16).max(1);
    let total: usize = frames.iter().step_by(step).map(|f| f.rgb.len() / 3).sum();
    let stride = if total <= 200_000 { 1 } else { 7 };
    let mut sample: Vec<[u8; 3]> = Vec::new();
    for frame in frames.iter().step_by(step) {
        for pixel in frame.rgb.as_chunks::<3>().0.iter().step_by(stride) {
            sample.push([pixel[0], pixel[1], pixel[2]]);
        }
    }
    let mut boxes = vec![sample];
    while boxes.len() < 256 {
        // The box with the widest spread of any channel is cut at its median.
        let Some((at, channel, spread)) = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .map(|(i, b)| {
                let (c, s) = widest(b);
                (i, c, s)
            })
            .max_by_key(|(_, _, s)| *s)
        else {
            break;
        };
        if spread == 0 {
            break;
        }
        let mut whole = boxes.swap_remove(at);
        whole.sort_unstable_by_key(|p| p[channel]);
        let upper = whole.split_off(whole.len() / 2);
        boxes.push(whole);
        boxes.push(upper);
    }
    let mut palette: Vec<[u8; 3]> = boxes
        .iter()
        .filter(|b| !b.is_empty())
        .map(|b| {
            let mut sum = [0u64; 3];
            for p in b {
                for c in 0..3 {
                    sum[c] += u64::from(p[c]);
                }
            }
            let n = b.len() as u64;
            [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8]
        })
        .collect();
    palette.resize(256, [0, 0, 0]);
    palette
}

/// The channel whose values spread widest in `pixels`, and the spread.
fn widest(pixels: &[[u8; 3]]) -> (usize, u8) {
    let mut best = (0, 0);
    for c in 0..3 {
        let (lo, hi) = pixels
            .iter()
            .fold((255u8, 0u8), |(lo, hi), p| (lo.min(p[c]), hi.max(p[c])));
        let spread = hi.saturating_sub(lo);
        if spread > best.1 {
            best = (c, spread);
        }
    }
    best
}

/// The nearest palette colour for any colour, by a 32-level cube of
/// answers worked out once.
struct Lookup {
    cube: Vec<u8>,
}

impl Lookup {
    fn new(palette: &[[u8; 3]]) -> Lookup {
        let mut cube = vec![0u8; 32 * 32 * 32];
        for r in 0..32 {
            for g in 0..32 {
                for b in 0..32 {
                    let colour = [r as i32 * 8 + 4, g as i32 * 8 + 4, b as i32 * 8 + 4];
                    cube[(r * 32 + g) * 32 + b] = nearest(palette, colour);
                }
            }
        }
        Lookup { cube }
    }

    fn get(&self, colour: [i32; 3]) -> u8 {
        let level = |v: i32| (v.clamp(0, 255) >> 3) as usize;
        self.cube[(level(colour[0]) * 32 + level(colour[1])) * 32 + level(colour[2])]
    }
}

fn nearest(palette: &[[u8; 3]], colour: [i32; 3]) -> u8 {
    let mut best = (0u8, i32::MAX);
    for (i, p) in palette.iter().enumerate() {
        let d: i32 = (0..3)
            .map(|c| {
                let e = i32::from(p[c]) - colour[c];
                e * e
            })
            .sum();
        if d < best.1 {
            best = (i as u8, d);
        }
    }
    best.0
}

/// A frame's colours as palette indices, the error of each spread to its
/// neighbours (Floyd–Steinberg), so gradients do not band.
fn dither(rgb: &[u8], width: usize, palette: &[[u8; 3]], lookup: &Lookup) -> Vec<u8> {
    let height = rgb.len() / 3 / width;
    let mut error = vec![[0i32; 3]; (width + 2) * 2];
    let mut out = Vec::with_capacity(width * height);
    for y in 0..height {
        let (this, next) = if y % 2 == 0 { (0, 1) } else { (1, 0) };
        for e in &mut error[next * (width + 2)..(next + 1) * (width + 2)] {
            *e = [0; 3];
        }
        for x in 0..width {
            let at = (y * width + x) * 3;
            let carried = error[this * (width + 2) + x + 1];
            let want = [
                i32::from(rgb[at]) + carried[0] / 16,
                i32::from(rgb[at + 1]) + carried[1] / 16,
                i32::from(rgb[at + 2]) + carried[2] / 16,
            ];
            let index = lookup.get(want);
            out.push(index);
            let got = palette[usize::from(index)];
            for c in 0..3 {
                let e = want[c].clamp(0, 255) - i32::from(got[c]);
                error[this * (width + 2) + x + 2][c] += e * 7;
                error[next * (width + 2) + x][c] += e * 3;
                error[next * (width + 2) + x + 1][c] += e * 5;
                error[next * (width + 2) + x + 2][c] += e;
            }
        }
    }
    out
}

/// GIF's LZW: variable-length codes, least significant bit first, a clear
/// code at the start and whenever the table fills.
fn lzw(indices: &[u8], min_size: u32) -> Vec<u8> {
    let clear = 1u32 << min_size;
    let end = clear + 1;
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    let mut emit = |code: u32, size: u32, out: &mut Vec<u8>| {
        acc |= code << bits;
        bits += size;
        while bits >= 8 {
            out.push((acc & 0xFF) as u8);
            acc >>= 8;
            bits -= 8;
        }
    };
    let mut table: std::collections::HashMap<(u32, u8), u32> = std::collections::HashMap::new();
    let mut next = end + 1;
    let mut size = min_size + 1;
    emit(clear, size, &mut out);
    let mut current: Option<u32> = None;
    for &k in indices {
        let Some(prefix) = current else {
            current = Some(u32::from(k));
            continue;
        };
        if let Some(&code) = table.get(&(prefix, k)) {
            current = Some(code);
            continue;
        }
        emit(prefix, size, &mut out);
        if next < 4096 {
            table.insert((prefix, k), next);
            next += 1;
            if next > (1 << size) && size < 12 {
                size += 1;
            }
        } else {
            emit(clear, size, &mut out);
            table.clear();
            next = end + 1;
            size = min_size + 1;
        }
        current = Some(u32::from(k));
    }
    if let Some(code) = current {
        emit(code, size, &mut out);
    }
    emit(end, size, &mut out);
    if bits > 0 {
        out.push((acc & 0xFF) as u8);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An LZW stream read back as GIF's decoder reads it.
    fn unlzw(data: &[u8], min_size: u32) -> Vec<u8> {
        let clear = 1u32 << min_size;
        let end = clear + 1;
        let mut table: Vec<Vec<u8>> = (0..clear).map(|i| vec![i as u8]).collect();
        table.push(Vec::new());
        table.push(Vec::new());
        let mut size = min_size + 1;
        let (mut acc, mut bits, mut at) = (0u32, 0u32, 0usize);
        let mut prev: Option<Vec<u8>> = None;
        let mut out = Vec::new();
        loop {
            while bits < size {
                acc |= u32::from(data[at]) << bits;
                at += 1;
                bits += 8;
            }
            let code = acc & ((1 << size) - 1);
            acc >>= size;
            bits -= size;
            if code == clear {
                table.truncate(end as usize + 1);
                size = min_size + 1;
                prev = None;
                continue;
            }
            if code == end {
                break;
            }
            let entry = if (code as usize) < table.len() {
                table[code as usize].clone()
            } else {
                let p = prev.clone().unwrap();
                let mut e = p.clone();
                e.push(p[0]);
                e
            };
            out.extend_from_slice(&entry);
            if let Some(p) = prev {
                let mut e = p.clone();
                e.push(entry[0]);
                table.push(e);
                if table.len() == (1 << size) && size < 12 {
                    size += 1;
                }
            }
            prev = Some(entry);
        }
        out
    }

    #[test]
    fn lzw_reads_back_what_went_in_long_and_short() {
        let mut data: Vec<u8> = (0..70_000u32)
            .map(|i| ((i * 7 + i / 13) % 251) as u8)
            .collect();
        data.extend(std::iter::repeat_n(9u8, 5000));
        for input in [vec![], vec![3u8], data] {
            assert_eq!(unlzw(&lzw(&input, 8), 8), input);
        }
    }

    #[test]
    fn a_gif_has_its_frames_its_loop_and_its_end() {
        let (w, h) = (32u16, 20u16);
        let frames: Vec<Frame> = (0..5u8)
            .map(|n| Frame {
                rgb: (0..u32::from(w) * u32::from(h))
                    .flat_map(|i| [(i % 256) as u8, n * 40, 255 - (i % 256) as u8])
                    .collect(),
                delay: 10,
            })
            .collect();
        let gif = encode(w, h, &frames).unwrap();
        assert_eq!(&gif[..6], b"GIF89a");
        assert_eq!(gif.last(), Some(&0x3B));
        assert!(gif.windows(11).any(|x| x == b"NETSCAPE2.0"));
        assert_eq!(gif.windows(2).filter(|x| x == &[0x21, 0xF9]).count(), 5);
    }

    #[test]
    fn a_picture_of_a_few_colours_keeps_them_exactly() {
        let colours = [[255u8, 0, 0], [0, 255, 0], [0, 0, 255], [250, 250, 250]];
        let rgb: Vec<u8> = (0..64).flat_map(|i| colours[i % 4]).collect();
        let frame = Frame {
            rgb: rgb.clone(),
            delay: 5,
        };
        let palette = palette_of(std::slice::from_ref(&frame));
        let lookup = Lookup::new(&palette);
        let indices = dither(&rgb, 8, &palette, &lookup);
        for (i, index) in indices.iter().enumerate() {
            assert_eq!(palette[usize::from(*index)], colours[i % 4], "pixel {i}");
        }
    }

    #[test]
    fn frames_of_other_sizes_are_refused_in_words() {
        let frames = [Frame {
            rgb: vec![0; 3],
            delay: 1,
        }];
        assert!(encode(2, 2, &frames).unwrap_err().contains("size"));
        assert!(encode(2, 2, &[]).is_err());
    }
}
