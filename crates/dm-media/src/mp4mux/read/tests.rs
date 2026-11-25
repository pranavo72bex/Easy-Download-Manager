// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use std::io::Write;

#[test]
fn a_box_size_that_wraps_the_walk_ends_it_instead_of_looping() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&16u32.to_be_bytes());
    bytes.extend_from_slice(b"free");
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(b"free");
    bytes.extend_from_slice(&(u64::MAX - 15).to_be_bytes());
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(&bytes).unwrap();
    let length = bytes.len() as u64;
    let said = read_input(&mut file, length, 0).unwrap_err().to_string();
    assert!(said.contains("no index"), "{said}");
}

#[test]
fn a_table_of_one_size_cannot_claim_billions_of_samples() {
    let mut stsz = Vec::new();
    stsz.extend_from_slice(&20u32.to_be_bytes());
    stsz.extend_from_slice(b"stsz");
    stsz.extend_from_slice(&0u32.to_be_bytes());
    stsz.extend_from_slice(&1u32.to_be_bytes());
    stsz.extend_from_slice(&u32::MAX.to_be_bytes());
    let said = plain_samples(&stsz).unwrap_err().to_string();
    assert!(said.contains("too many samples"), "{said}");
}

/// Runs of samples with no per-sample fields, each within the cap alone, are bounded
/// together: a hundred of them asked for some 38 GB from a few KB (gaps rows H10, H35).
#[test]
fn a_tracks_runs_together_cannot_claim_billions_of_samples() {
    fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }
    let mut tfhd = 0u32.to_be_bytes().to_vec();
    tfhd.extend_from_slice(&1u32.to_be_bytes());
    let mut trun = 0u32.to_be_bytes().to_vec();
    trun.extend_from_slice(&(10_000_000u32).to_be_bytes());
    let mut traf = boxed(b"tfhd", &tfhd);
    for _ in 0..3 {
        traf.extend(boxed(b"trun", &trun));
    }
    let moof = boxed(b"traf", &traf);
    let mut tracks = vec![crate::mp4mux::Track {
        id: 1,
        ..Default::default()
    }];
    assert!(read_fragment(0, &moof, &[], &mut tracks).is_err());
    assert!(tracks[0].samples.len() <= MOST_SAMPLES);
}

/// Many tracks, each a one-size table under the cap alone, are refused together before any
/// table is made (gaps row H35a): 64 such tracks asked for gigabytes from a few KB.
#[test]
fn many_tracks_of_one_size_cannot_claim_billions_of_samples_together() {
    fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }
    let mut stsz = 0u32.to_be_bytes().to_vec();
    stsz.extend_from_slice(&1u32.to_be_bytes());
    stsz.extend_from_slice(&(MOST_SAMPLES as u32).to_be_bytes());
    let trak = boxed(
        b"trak",
        &boxed(
            b"mdia",
            &boxed(b"minf", &boxed(b"stbl", &boxed(b"stsz", &stsz))),
        ),
    );
    let moov: Vec<u8> = std::iter::repeat_n(trak, 64).flatten().collect();
    assert_eq!(claimed_samples(&moov), 64 * MOST_SAMPLES);
    let said = tracks_of(&moov, 0).unwrap_err().to_string();
    assert!(said.contains("too many samples together"), "{said}");
}

/// A fragment's runs are bounded over every track together, not each track alone (gaps
/// row H35b): two tracks each just under the cap made twice it.
#[test]
fn runs_over_many_tracks_together_cannot_claim_billions_of_samples() {
    fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }
    let traf = |id: u32, count: u32| {
        let mut tfhd = 0u32.to_be_bytes().to_vec();
        tfhd.extend_from_slice(&id.to_be_bytes());
        let mut trun = 0u32.to_be_bytes().to_vec();
        trun.extend_from_slice(&count.to_be_bytes());
        let mut body = boxed(b"tfhd", &tfhd);
        body.extend(boxed(b"trun", &trun));
        boxed(b"traf", &body)
    };
    let most = MOST_SAMPLES as u32;
    let mut tracks: Vec<crate::mp4mux::Track> = (1..=2)
        .map(|id| crate::mp4mux::Track {
            id,
            ..Default::default()
        })
        .collect();
    // The first track takes most of the cap; the second's run passes it together.
    read_fragment(0, &traf(1, most - 10), &[], &mut tracks).unwrap();
    assert!(read_fragment(0, &traf(2, 20), &[], &mut tracks).is_err());
    let all: usize = tracks.iter().map(|t| t.samples.len()).sum();
    assert!(all <= MOST_SAMPLES, "{all} samples");
}
