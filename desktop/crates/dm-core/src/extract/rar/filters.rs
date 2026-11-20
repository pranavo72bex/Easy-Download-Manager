// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! RAR 5's filters, undone over a block of unpacked bytes before it is written: DELTA (bytes
//! kept as differences, channel by channel), E8 and E8E9 (x86 calls and jumps whose relative
//! addresses were made absolute) and ARM (the same for ARM's BL). Each works on a copy, so
//! the window later matches copy from keeps the bytes as they were packed.

/// The filters' numbers in the packed stream.
pub(super) const DELTA: u8 = 0;
pub(super) const E8: u8 = 1;
pub(super) const E8E9: u8 = 2;
pub(super) const ARM: u8 = 3;

/// Undoes filter `kind` over `data`, a block that starts `offset` bytes into its file;
/// `channels` is DELTA's. Where DELTA needs a second buffer, `spare` is it.
pub(super) fn undo(
    kind: u8,
    channels: usize,
    offset: u64,
    data: &mut Vec<u8>,
    spare: &mut Vec<u8>,
) {
    // The format counts the offset in 32 bits.
    let offset = offset as u32;
    match kind {
        DELTA => delta(channels.max(1), data, spare),
        E8 => x86(data, offset, false),
        E8E9 => x86(data, offset, true),
        ARM => arm(data, offset),
        _ => {}
    }
}

/// DELTA: the block holds each channel's bytes together, each the difference from the one
/// before it in that channel; they are put back between each other.
fn delta(channels: usize, data: &mut Vec<u8>, spare: &mut Vec<u8>) {
    spare.clear();
    spare.resize(data.len(), 0);
    let mut src = data.iter();
    for channel in 0..channels {
        let mut prev = 0u8;
        for slot in spare.iter_mut().skip(channel).step_by(channels) {
            prev = prev.wrapping_sub(src.next().copied().unwrap_or(0));
            *slot = prev;
        }
    }
    std::mem::swap(data, spare);
}

/// E8 and E8E9: after each 0xE8 (and 0xE9) byte, a 32-bit address made absolute against a
/// 16 MiB file is made relative again.
fn x86(data: &mut [u8], offset: u32, e9: bool) {
    const FILE_SIZE: u32 = 0x0100_0000;
    let mut at = 0usize;
    while at + 4 < data.len() {
        let byte = data.get(at).copied().unwrap_or(0);
        at += 1;
        if byte != 0xe8 && !(e9 && byte == 0xe9) {
            continue;
        }
        let Some(slot) = data.get_mut(at..at + 4) else {
            break;
        };
        let here = (at as u32).wrapping_add(offset) % FILE_SIZE;
        let addr = u32::from_le_bytes([slot[0], slot[1], slot[2], slot[3]]);
        if addr & 0x8000_0000 != 0 {
            // Negative: it stays so unless adding the place makes it not.
            if addr.wrapping_add(here) & 0x8000_0000 == 0 {
                slot.copy_from_slice(&addr.wrapping_add(FILE_SIZE).to_le_bytes());
            }
        } else if addr.wrapping_sub(FILE_SIZE) & 0x8000_0000 != 0 {
            // Below the file's size.
            slot.copy_from_slice(&addr.wrapping_sub(here).to_le_bytes());
        }
        at += 4;
    }
}

/// ARM: each four-byte BL whose condition is "always" has its 24-bit offset, counted in
/// words, made relative again.
fn arm(data: &mut [u8], offset: u32) {
    for (i, word) in data.chunks_exact_mut(4).enumerate() {
        if word[3] != 0xeb {
            continue;
        }
        let at = (i * 4) as u32;
        let mut v = u32::from(word[0]) | (u32::from(word[1]) << 8) | (u32::from(word[2]) << 16);
        v = v.wrapping_sub(offset.wrapping_add(at) / 4);
        word[0] = v as u8;
        word[1] = (v >> 8) as u8;
        word[2] = (v >> 16) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn undone(kind: u8, channels: usize, offset: u64, data: &[u8]) -> Vec<u8> {
        let (mut data, mut spare) = (data.to_vec(), Vec::new());
        undo(kind, channels, offset, &mut data, &mut spare);
        data
    }

    #[test]
    fn delta_puts_each_channel_back_between_the_others() {
        // Two channels: 10, 20, 30 (as 0-10, 10-20, 20-30) and 1, 2, 3.
        let packed = [246, 246, 246, 255, 255, 255];
        assert_eq!(undone(DELTA, 2, 0, &packed), [10, 1, 20, 2, 30, 3]);
    }

    #[test]
    fn e8_makes_an_absolute_call_relative_again_and_leaves_others() {
        // A call at 0 whose address 0x105 was made absolute: 0x105 + 1 (the place after
        // the 0xE8 byte) = 0x106; 16 bytes in, the same call is at 0x116 - 0x11 = 0x105.
        let mut data = vec![0xe8, 0x06, 0x01, 0, 0, 0x90, 0x90, 0x90, 0x90];
        assert_eq!(
            undone(E8, 0, 0, &data),
            [0xe8, 0x05, 0x01, 0, 0, 0x90, 0x90, 0x90, 0x90]
        );
        data[1] = 0x16;
        assert_eq!(undone(E8, 0, 16, &data)[1], 0x05);
        // E9 is a jump only E8E9 undoes.
        data[0] = 0xe9;
        assert_eq!(undone(E8, 0, 16, &data), data);
        assert_eq!(undone(E8E9, 0, 16, &data)[1], 0x05);
        // An address past the file's 16 MiB is left as it is.
        let far = [0xe8, 0, 0, 0, 0x02, 0x90];
        assert_eq!(undone(E8, 0, 0, &far), far);
    }

    #[test]
    fn arm_makes_a_bl_relative_again() {
        // A BL 8 bytes into a block 8 bytes into its file: (8 + 8) / 4 = 4 words.
        let data = [0, 0, 0, 0, 0, 0, 0, 0, 0x0a, 0, 0, 0xeb, 0x0a, 0, 0, 0xea];
        let got = undone(ARM, 0, 8, &data);
        assert_eq!(got[8..12], [0x06, 0, 0, 0xeb]);
        assert_eq!(got[12..], [0x0a, 0, 0, 0xea]);
    }
}
