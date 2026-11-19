// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
    assert!(encode(2, 2, &frames)
        .unwrap_err()
        .to_string()
        .contains("size"));
    assert!(encode(2, 2, &[]).is_err());
}
