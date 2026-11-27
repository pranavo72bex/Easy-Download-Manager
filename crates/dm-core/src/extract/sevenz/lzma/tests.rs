// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The LZMA and LZMA2 readers' refusals made before any byte is unpacked.

use super::*;

/// A coder whose dictionary is over the largest dm unpacks with (the format allows 4 GiB) is
/// refused before a window is made: property bytes asking for 4 GiB for a stream of zeros held
/// that much memory as it unpacked (gaps row H35d). One at the largest is taken.
#[test]
fn a_coder_asking_for_a_larger_dictionary_than_dm_allows_is_refused() {
    let lzma = |dict: u32| {
        let [a, b, c, d] = dict.to_le_bytes();
        LzmaReader::new(Box::new(io::empty()), &[0x5d, a, b, c, d], 0).err()
    };
    let lzma2 = |prop: u8| Lzma2Reader::new(Box::new(io::empty()), prop, 0).err();
    let most = crate::extract::MOST_DICTIONARY as u32;
    assert!(lzma(most).is_none());
    // LZMA2's code 37 is 1.5 GiB, 38 is 2 GiB and 40 is 4 GiB.
    assert!(lzma2(37).is_none());
    for refused in [lzma(most + 1), lzma(u32::MAX), lzma2(38), lzma2(40)] {
        let said = refused.expect("refused").to_string();
        assert!(said.contains("dictionary is over 1.5 GiB"), "{said}");
    }
}
