//! Signing in to a server that asks for NTLM, or for Negotiate and takes
//! NTLM under it: the sign-in of Windows servers and of company intranets.
//!
//! NTLMv2 as MS-NLMP sets it out: a negotiate message, the server's
//! challenge, and an answer computed from the password's NT hash, the
//! server's challenge and one of dm's own. Only the sign-in: no signing or
//! sealing of what follows, which HTTP does not use. The older LM and
//! NTLMv1 answers are not offered: they are broken, and a server that
//! still takes only them is not one to send a password's hash to.
//!
//! NTLM signs in a connection, not a request: the answer is good only on
//! the connection the challenge came on. [`crate::auth::send`] reads each
//! reply whole before sending the next, so the connection goes back to the
//! pool and the next request takes it again.

// Reads outside input (the server's challenge): an index out of range here
// is a crash a server could cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

use md4::Md4;
use md5::Md5;
use sha2::Digest as _;

const SIGNATURE: &[u8; 8] = b"NTLMSSP\0";

const UNICODE: u32 = 0x0000_0001;
const OEM: u32 = 0x0000_0002;
const REQUEST_TARGET: u32 = 0x0000_0004;
const NTLM: u32 = 0x0000_0200;
const ALWAYS_SIGN: u32 = 0x0000_8000;
const EXTENDED_SESSION_SECURITY: u32 = 0x0008_0000;
const TARGET_INFO: u32 = 0x0080_0000;

/// What dm asks for: Unicode or OEM names, the server's target, NTLM with
/// extended session security.
const ASKED: u32 = UNICODE | OEM | REQUEST_TARGET | NTLM | ALWAYS_SIGN | EXTENDED_SESSION_SECURITY;

/// The target info's timestamp, MsvAvTimestamp.
const AV_TIMESTAMP: u16 = 7;
const AV_EOL: u16 = 0;

/// The first message: what dm can do, and no names yet.
pub fn negotiate() -> Vec<u8> {
    let mut m = Vec::with_capacity(32);
    m.extend_from_slice(SIGNATURE);
    m.extend_from_slice(&1u32.to_le_bytes());
    m.extend_from_slice(&ASKED.to_le_bytes());
    // Domain and workstation: empty, at the message's end.
    for _ in 0..2 {
        m.extend_from_slice(&0u16.to_le_bytes());
        m.extend_from_slice(&0u16.to_le_bytes());
        m.extend_from_slice(&32u32.to_le_bytes());
    }
    m
}

/// The server's challenge, as read from its message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    pub flags: u32,
    pub server_challenge: [u8; 8],
    /// Its target info, sent back inside the answer as it came.
    pub target_info: Vec<u8>,
}

fn u16_at(m: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(m.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(m: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(m.get(at..at + 4)?.try_into().ok()?))
}

/// The bytes a message's field (length, room, offset) points at.
fn field(m: &[u8], at: usize) -> Option<&[u8]> {
    let len = usize::from(u16_at(m, at)?);
    let offset = usize::try_from(u32_at(m, at + 4)?).ok()?;
    m.get(offset..offset.checked_add(len)?)
}

/// Reads the server's challenge message, or nothing for one that is not.
pub fn challenge(m: &[u8]) -> Option<Challenge> {
    if m.get(..8)? != SIGNATURE || u32_at(m, 8)? != 2 {
        return None;
    }
    let flags = u32_at(m, 20)?;
    let server_challenge: [u8; 8] = m.get(24..32)?.try_into().ok()?;
    let target_info = if flags & TARGET_INFO != 0 {
        field(m, 40)?.to_vec()
    } else {
        Vec::new()
    };
    Some(Challenge {
        flags,
        server_challenge,
        target_info,
    })
}

/// The timestamp a server put in its target info, if any.
fn timestamp_in(target_info: &[u8]) -> Option<[u8; 8]> {
    let mut at = 0usize;
    loop {
        let id = u16_at(target_info, at)?;
        let len = usize::from(u16_at(target_info, at + 2)?);
        if id == AV_EOL {
            return None;
        }
        let value = target_info.get(at + 4..at + 4 + len)?;
        if id == AV_TIMESTAMP {
            return value.try_into().ok();
        }
        at += 4 + len;
    }
}

/// Now as Windows counts time: hundreds of nanoseconds since 1601.
fn filetime_now() -> [u8; 8] {
    const FROM_1601: u64 = 11_644_473_600;
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let ticks = (since.as_secs() + FROM_1601)
        .saturating_mul(10_000_000)
        .saturating_add(u64::from(since.subsec_nanos() / 100));
    ticks.to_le_bytes()
}

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn hmac_md5(key: &[u8], parts: &[&[u8]]) -> [u8; 16] {
    let mut block = [0u8; 64];
    let hashed;
    let key: &[u8] = if key.len() > 64 {
        hashed = Md5::digest(key);
        &hashed
    } else {
        key
    };
    for (slot, byte) in block.iter_mut().zip(key) {
        *slot = *byte;
    }
    let mut inner = Md5::new();
    inner.update(block.map(|b| b ^ 0x36));
    for part in parts {
        inner.update(part);
    }
    let mut outer = Md5::new();
    outer.update(block.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

/// NTOWFv2: the password's NT hash, keyed to the user and the domain.
pub fn ntowf_v2(user: &str, domain: &str, password: &str) -> [u8; 16] {
    let nt_hash = Md4::digest(utf16(password));
    let who = utf16(&format!("{}{domain}", user.to_uppercase()));
    hmac_md5(&nt_hash, &[&who])
}

/// The NTLMv2 answer's two halves to a challenge, with the client's
/// challenge and time given: (LMv2, NTLMv2).
pub fn responses(
    key: &[u8; 16],
    server_challenge: &[u8; 8],
    client_challenge: &[u8; 8],
    time: &[u8; 8],
    target_info: &[u8],
) -> ([u8; 24], Vec<u8>) {
    let mut temp = vec![1u8, 1, 0, 0, 0, 0, 0, 0];
    temp.extend_from_slice(time);
    temp.extend_from_slice(client_challenge);
    temp.extend_from_slice(&[0; 4]);
    temp.extend_from_slice(target_info);
    temp.extend_from_slice(&[0; 4]);
    let proof = hmac_md5(key, &[server_challenge, &temp]);
    let mut nt = proof.to_vec();
    nt.extend_from_slice(&temp);
    let mut lm = [0u8; 24];
    let lm_proof = hmac_md5(key, &[server_challenge, client_challenge]);
    for (slot, byte) in lm.iter_mut().zip(lm_proof.iter().chain(client_challenge)) {
        *slot = *byte;
    }
    (lm, nt)
}

/// Who signs in: `DOMAIN\user` split at the backslash; `user@domain` and a
/// bare name are the user alone, the server's own domain then applying.
pub fn split_user(login: &str) -> (&str, &str) {
    match login.split_once('\\') {
        Some((domain, user)) => (domain, user),
        None => ("", login),
    }
}

/// The last message: the answer to `challenge` for this login.
pub fn authenticate(challenge: &Challenge, login: &str, password: &str) -> Option<Vec<u8>> {
    let (domain, user) = split_user(login);
    let key = ntowf_v2(user, domain, password);
    let mut client_challenge = [0u8; 8];
    getrandom::fill(&mut client_challenge).ok()?;
    // A timestamp from the server is the one to answer with, and then the
    // LMv2 half is sent empty, as MS-NLMP asks.
    let served = timestamp_in(&challenge.target_info);
    let time = served.unwrap_or_else(filetime_now);
    let (lm, nt) = responses(
        &key,
        &challenge.server_challenge,
        &client_challenge,
        &time,
        &challenge.target_info,
    );
    let lm: &[u8] = if served.is_some() { &[0; 24] } else { &lm };
    let unicode = challenge.flags & UNICODE != 0;
    let text = |s: &str| -> Vec<u8> {
        if unicode {
            utf16(s)
        } else {
            s.as_bytes().to_vec()
        }
    };
    let flags = (challenge.flags & ASKED) | NTLM | if unicode { UNICODE } else { OEM };
    let fields: [Vec<u8>; 6] = [
        lm.to_vec(),
        nt,
        text(domain),
        text(user),
        text(""),
        Vec::new(),
    ];
    // Six fields of 8 bytes and the flags after the 12-byte head.
    const HEAD: usize = 64;
    let mut m = Vec::with_capacity(HEAD + fields.iter().map(Vec::len).sum::<usize>());
    m.extend_from_slice(SIGNATURE);
    m.extend_from_slice(&3u32.to_le_bytes());
    let mut offset = HEAD;
    for f in &fields {
        let len = u16::try_from(f.len()).ok()?;
        m.extend_from_slice(&len.to_le_bytes());
        m.extend_from_slice(&len.to_le_bytes());
        m.extend_from_slice(&u32::try_from(offset).ok()?.to_le_bytes());
        offset += f.len();
    }
    m.extend_from_slice(&flags.to_le_bytes());
    for f in &fields {
        m.extend_from_slice(f);
    }
    Some(m)
}

/// The server's challenge in a `WWW-Authenticate` value of `scheme`
/// ("NTLM" or "Negotiate") followed by its base64 message.
pub fn challenge_in(header: &str, scheme: &str) -> Option<Challenge> {
    use base64::Engine as _;
    let (named, token) = header.trim().split_once(' ')?;
    if !named.eq_ignore_ascii_case(scheme) {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(token.trim())
        .ok()?;
    challenge(&bytes)
}

/// Which of the two a `WWW-Authenticate` value offers, bare: the server's
/// first word to a request that carried no sign-in.
pub fn offered(header: &str) -> Option<&'static str> {
    let word = header.trim();
    if word.eq_ignore_ascii_case("NTLM") {
        Some("NTLM")
    } else if word.eq_ignore_ascii_case("Negotiate") {
        Some("Negotiate")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(hex: &str) -> Vec<u8> {
        hex::decode(hex.replace(' ', "")).unwrap()
    }

    /// MS-NLMP 4.2.4's target info: NetBIOS domain "Domain", computer
    /// "Server".
    fn spec_target_info() -> Vec<u8> {
        let mut t = Vec::new();
        for (id, value) in [(2u16, "Domain"), (1, "Server")] {
            let v = utf16(value);
            t.extend_from_slice(&id.to_le_bytes());
            t.extend_from_slice(&(v.len() as u16).to_le_bytes());
            t.extend_from_slice(&v);
        }
        t.extend_from_slice(&[0, 0, 0, 0]);
        t
    }

    /// The worked example in MS-NLMP 4.2.4 (NTLMv2): NTOWFv2, the LMv2
    /// answer and the NTLMv2 proof.
    #[test]
    fn the_specifications_example_is_answered_as_it_says() {
        let key = ntowf_v2("User", "Domain", "Password");
        assert_eq!(key.to_vec(), bytes("0c868a403bfd7a93a3001ef22ef02e3f"));
        let server = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];
        let client = [0xaa; 8];
        let (lm, nt) = responses(&key, &server, &client, &[0; 8], &spec_target_info());
        assert_eq!(
            lm.to_vec(),
            bytes("86c35097ac9cec102554764a57cccc19aaaaaaaaaaaaaaaa")
        );
        assert_eq!(nt[..16].to_vec(), bytes("68cd0ab851e51c96aabc927bebef6a1c"));
    }

    #[test]
    fn a_challenge_is_read_and_answered() {
        let info = spec_target_info();
        let mut m = Vec::new();
        m.extend_from_slice(SIGNATURE);
        m.extend_from_slice(&2u32.to_le_bytes());
        m.extend_from_slice(&[0, 0, 0, 0, 56, 0, 0, 0]);
        m.extend_from_slice(&(ASKED | TARGET_INFO).to_le_bytes());
        m.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        m.extend_from_slice(&[0; 8]);
        m.extend_from_slice(&(info.len() as u16).to_le_bytes());
        m.extend_from_slice(&(info.len() as u16).to_le_bytes());
        m.extend_from_slice(&56u32.to_le_bytes());
        m.extend_from_slice(&[0; 8]);
        m.extend_from_slice(&info);
        let c = challenge(&m).unwrap();
        assert_eq!(c.server_challenge, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(c.target_info, info);
        let answer = authenticate(&c, r"CORP\someone", "a password").unwrap();
        assert_eq!(&answer[..8], SIGNATURE);
        assert_eq!(u32_at(&answer, 8), Some(3));
        assert_eq!(field(&answer, 28).unwrap(), utf16("CORP").as_slice());
        assert_eq!(field(&answer, 36).unwrap(), utf16("someone").as_slice());
        // The NTLMv2 answer carries the server's target info back.
        let nt = field(&answer, 20).unwrap();
        assert!(nt.windows(info.len()).any(|w| w == info.as_slice()));
    }

    #[test]
    fn a_short_or_foreign_message_is_not_a_challenge() {
        assert_eq!(challenge(b""), None);
        assert_eq!(challenge(b"NTLMSSP\0\x02\0\0\0"), None);
        assert_eq!(challenge(&negotiate()), None);
        // A target info field that points past the end.
        let mut m = Vec::new();
        m.extend_from_slice(SIGNATURE);
        m.extend_from_slice(&2u32.to_le_bytes());
        m.extend_from_slice(&[0; 8]);
        m.extend_from_slice(&TARGET_INFO.to_le_bytes());
        m.extend_from_slice(&[0; 16]);
        m.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f]);
        assert_eq!(challenge(&m), None);
    }

    #[test]
    fn the_scheme_is_told_apart() {
        assert_eq!(offered("NTLM"), Some("NTLM"));
        assert_eq!(offered(" negotiate "), Some("Negotiate"));
        assert_eq!(offered("Basic realm=\"x\""), None);
        assert_eq!(split_user(r"CORP\me"), ("CORP", "me"));
        assert_eq!(split_user("me@corp.example"), ("", "me@corp.example"));
    }
}
