// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The ciphers against the published test vectors (RFC 2202's HMAC-SHA1, RFC 6070's
//! PBKDF2-HMAC-SHA1), and the readers against their own sealing, in pieces of every size.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn hmac_sha1_matches_rfc_2202() {
    let mut mac = HmacSha1::new(&[0x0b; 20]);
    mac.update(b"Hi There");
    assert_eq!(
        hex(&mac.finish()),
        "b617318655057264e28bc0b6fb378c8ef146be00"
    );
    let mut mac = HmacSha1::new(b"Jefe");
    mac.update(b"what do ya want ");
    mac.update(b"for nothing?");
    assert_eq!(
        hex(&mac.finish()),
        "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79"
    );
    // A key longer than SHA-1's block is hashed first.
    let mut mac = HmacSha1::new(&[0xaa; 80]);
    mac.update(b"Test Using Larger Than Block-Size Key - Hash Key First");
    assert_eq!(
        hex(&mac.finish()),
        "aa4ae5e15272d00e95705637ce8a3b55ed402112"
    );
}

#[test]
fn pbkdf2_sha1_matches_rfc_6070() {
    let mut out = [0u8; 20];
    pbkdf2_sha1(b"password", b"salt", 1, &mut out);
    assert_eq!(hex(&out), "0c60c80f961f0e71f3a9b524af6012062fe037a6");
    pbkdf2_sha1(b"password", b"salt", 2, &mut out);
    assert_eq!(hex(&out), "ea6c014dc72d6f8ccd1ed92ace1d41f0d8de8957");
    pbkdf2_sha1(b"password", b"salt", 4096, &mut out);
    assert_eq!(hex(&out), "4b007901b765489abead49d926f721d065a429c1");
    // More than one block of output.
    let mut long = [0u8; 25];
    pbkdf2_sha1(
        b"passwordPASSWORDpassword",
        b"saltSALTsaltSALTsaltSALTsaltSALTsalt",
        4096,
        &mut long,
    );
    assert_eq!(
        hex(&long),
        "3d2eec4fe41c849b80c8d83662c0e44a8b291a964cf2f07038"
    );
}

/// Reads all of `r` in pieces of `piece` bytes.
fn read_in(mut r: impl Read, piece: usize) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf = vec![0u8; piece];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            return Ok(out);
        }
        out.extend_from_slice(&buf[..n]);
    }
}

#[test]
fn a_zip_aes_entry_reads_back_at_every_strength_and_in_any_piece() {
    let plain: Vec<u8> = (0..5000u32).map(|i| (i * 7 % 251) as u8).collect();
    for strength in 1..=3u8 {
        let aes = ZipAes {
            vendor: 2,
            strength,
            method: 0,
        };
        let salt: Vec<u8> = (0..aes.salt_len() as u8).collect();
        let sealed = zip_aes_seal(aes, b"pass", &salt, &plain);
        assert_eq!(sealed.len() as u64, plain.len() as u64 + aes.overhead());
        for piece in [1, 15, 16, 17, 4096] {
            let r = ZipAesReader::open(&sealed[..], aes, sealed.len() as u64, b"pass").unwrap();
            assert_eq!(
                read_in(r, piece).unwrap(),
                plain,
                "strength {strength}, {piece}"
            );
        }
        // The wrong password is known by its check before anything is read.
        assert!(matches!(
            ZipAesReader::open(&sealed[..], aes, sealed.len() as u64, b"wrong"),
            Err(Opened::WrongPassword)
        ));
        // A changed byte fails the authentication code at the end.
        let mut bad = sealed.clone();
        let mid = aes.salt_len() + 2 + 100;
        bad[mid] ^= 1;
        let r = ZipAesReader::open(&bad[..], aes, bad.len() as u64, b"pass").unwrap();
        let e = read_in(r, 4096).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData, "{e}");
        // Cut short: an error, not an end.
        let r = ZipAesReader::open(
            &sealed[..sealed.len() - 20],
            aes,
            sealed.len() as u64,
            b"pass",
        )
        .unwrap();
        assert!(read_in(r, 4096).is_err());
    }
}

#[test]
fn the_aes_extra_field_is_read_and_odd_ones_are_refused() {
    let got = ZipAes::parse(&[2, 0, b'A', b'E', 3, 8, 0]).unwrap();
    assert_eq!(
        got,
        ZipAes {
            vendor: 2,
            strength: 3,
            method: 8
        }
    );
    assert_eq!(
        (got.key_len(), got.salt_len(), got.overhead()),
        (32, 16, 28)
    );
    assert!(ZipAes::parse(&[1, 0, b'A', b'E', 1, 0, 0]).is_ok());
    for bad in [
        &[2u8, 0, b'A', b'E', 4, 8, 0][..],
        &[3, 0, b'A', b'E', 3, 8, 0],
        &[2, 0, b'X', b'E', 3, 8, 0],
        &[2, 0, b'A', b'E', 3, 8],
    ] {
        assert!(ZipAes::parse(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn seven_aes_properties_are_read_as_the_format_lays_them_out() {
    // 7-Zip's usual: 2^19 rounds, no salt, a 16-byte IV.
    let mut props = vec![0x40 | 19, 0x0F];
    props.extend(1..=16u8);
    let got = SevenAes::parse(&props).unwrap();
    assert_eq!((got.cycles, got.salt.len()), (19, 0));
    assert_eq!(got.iv, core::array::from_fn::<u8, 16, _>(|i| i as u8 + 1));
    // A salt and a short IV, zero-filled.
    let got = SevenAes::parse(&[0xC0 | 5, 0x10, 9, 9, 7]).unwrap();
    assert_eq!((got.cycles, got.salt.clone()), (5, vec![9, 9]));
    assert_eq!(got.iv[..2], [7, 0]);
    // Just the rounds.
    assert_eq!(SevenAes::parse(&[19]).unwrap().salt, Vec::<u8>::new());
    for bad in [&[][..], &[0x01, 0x02], &[0x40 | 19, 0x0F, 1], &[30]] {
        assert!(SevenAes::parse(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn a_seven_key_is_sha256_over_its_rounds() {
    // Two rounds by hand: salt, password, the round's number.
    let aes = SevenAes {
        cycles: 1,
        salt: vec![1, 2],
        iv: [0; 16],
    };
    let pw = utf16le("ab");
    assert_eq!(pw, [b'a', 0, b'b', 0]);
    let mut all = Vec::new();
    for i in 0u64..2 {
        all.extend_from_slice(&[1, 2]);
        all.extend_from_slice(&pw);
        all.extend_from_slice(&i.to_le_bytes());
    }
    let want: [u8; 32] = sha2::Sha256::digest(&all).into();
    assert_eq!(aes.key(&pw), want);
    // Rounds of 63: the salt and password as the key.
    let raw = SevenAes {
        cycles: 0x3F,
        salt: vec![5],
        iv: [0; 16],
    };
    let mut want = [0u8; 32];
    want[..5].copy_from_slice(&[5, b'a', 0, b'b', 0]);
    assert_eq!(raw.key(&pw), want);
}

#[test]
fn cbc_reads_back_what_was_sealed_and_drops_the_padding() {
    let key = [3u8; 32];
    let iv = [9u8; 16];
    for len in [1usize, 15, 16, 17, 70_000] {
        let plain: Vec<u8> = (0..len).map(|i| (i % 253) as u8).collect();
        let sealed = seven_seal(&key, iv, &plain);
        for piece in [1, 16, 1000] {
            let r = CbcReader::new(
                Box::new(io::Cursor::new(sealed.clone())),
                &key,
                iv,
                len as u64,
            )
            .unwrap();
            assert_eq!(read_in(r, piece).unwrap(), plain, "{len} in {piece}");
        }
        // A packed stream that is not whole blocks is damaged.
        let r = CbcReader::new(
            Box::new(io::Cursor::new(sealed[..sealed.len() - 1].to_vec())),
            &key,
            iv,
            len as u64,
        )
        .unwrap();
        assert!(read_in(r, 4096).is_err());
    }
}
