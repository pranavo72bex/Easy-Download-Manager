// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A name under `.test` asked of a DNS server the benchmark names, in place of the system (its
//! `slowname` case, gaps row W2): the harness answers `*.bench.test` after a delay, so a lookup
//! costs what a slow resolver costs, and how often EDM asks is counted at the server. edm-core
//! reads the switch (`EDM_BENCH_DNS`) and hands the server here ([`ask_test_names_of`]) before
//! the first client; with none given, every name goes to the system as before.
//!
//! One question over UDP for the name's IPv4 addresses (type A), the answer read for them; no
//! retry, no TCP, no IPv6: what the benchmark's server answers. The same client asks the
//! computer's name servers from the held network's address under "Only through" (`held.rs`). `.test` is reserved for testing
//! (RFC 6761), so no real name is ever sent here.

use std::io::ErrorKind;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::OnceLock;
use std::time::Duration;

/// The DNS server names under `.test` are asked of ([`ask_test_names_of`]).
static TEST_SERVER: OnceLock<SocketAddr> = OnceLock::new();

/// How long an answer is waited for before the lookup fails.
const WAIT: Duration = Duration::from_secs(5);

/// The most bytes an answer over UDP has (RFC 1035 4.2.1, without EDNS).
const MOST: usize = 512;

/// Has every name under `.test` asked of the DNS server at `server` from now on, in place of the
/// system: the benchmark's `slowname` case. Once a program: false, and nothing changed, where a
/// server was already given.
pub fn ask_test_names_of(server: SocketAddr) -> bool {
    TEST_SERVER.set(server).is_ok()
}

/// The server `name` is asked of in place of the system: the one given, for a name under
/// `.test`.
pub(super) fn server_for(name: &str) -> Option<SocketAddr> {
    let server = *TEST_SERVER.get()?;
    under_test(name).then_some(server)
}

/// Whether `name` is under `.test`, the names reserved for testing.
fn under_test(name: &str) -> bool {
    let name = name.trim_end_matches('.');
    name == "test" || name.ends_with(".test")
}

/// `name`'s IPv4 addresses, asked of the DNS server at `server`; the port is the connector's
/// to set.
pub(super) async fn ask(server: SocketAddr, name: &str) -> std::io::Result<Vec<SocketAddr>> {
    ask_from(server, name, None).await
}

/// [`ask`], the question sent from `from` where given: the address of the network downloads
/// are held to, so the question cannot leave by another (gaps row H23d).
pub(super) async fn ask_from(
    server: SocketAddr,
    name: &str,
    from: Option<std::net::IpAddr>,
) -> std::io::Result<Vec<SocketAddr>> {
    let id = question_id();
    let asked = question(id, name)?;
    let any: SocketAddr = match from {
        Some(from) => (from, 0).into(),
        None if server.is_ipv4() => (Ipv4Addr::UNSPECIFIED, 0).into(),
        None => (Ipv6Addr::UNSPECIFIED, 0).into(),
    };
    let socket = tokio::net::UdpSocket::bind(any).await?;
    socket.connect(server).await?;
    socket.send(&asked).await?;
    let mut answer = [0u8; MOST];
    let read = tokio::time::timeout(WAIT, socket.recv(&mut answer))
        .await
        .map_err(|_| failed(ErrorKind::TimedOut, name, "the DNS server did not answer"))??;
    let found = addresses(answer.get(..read).unwrap_or_default(), id)
        .map_err(|why| failed(ErrorKind::InvalidData, name, why))?;
    if found.is_empty() {
        return Err(failed(ErrorKind::NotFound, name, "no address"));
    }
    Ok(found
        .into_iter()
        .map(|ip| SocketAddr::new(ip.into(), 0))
        .collect())
}

/// A lookup's failure, said with its name.
fn failed(kind: std::io::ErrorKind, name: &str, why: &str) -> std::io::Error {
    std::io::Error::new(kind, format!("{name}: {why}"))
}

/// A question's number, different from one lookup to the next, which its answer must carry.
fn question_id() -> u16 {
    use std::sync::atomic::{AtomicU16, Ordering};
    static NEXT: AtomicU16 = AtomicU16::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    // The low bits of the clock, so two runs of EDM do not start from the same number.
    NEXT.fetch_add(1, Ordering::Relaxed) ^ (nanos as u16)
}

/// The question for `name`'s IPv4 addresses (type A, class IN), recursion asked for.
fn question(id: u16, name: &str) -> std::io::Result<Vec<u8>> {
    let bad = |why: &str| failed(ErrorKind::InvalidInput, name, why);
    let mut out = Vec::with_capacity(18 + name.len());
    out.extend_from_slice(&id.to_be_bytes());
    // Flags: a query, recursion desired. One question, nothing else.
    out.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);
    let mut length = 1;
    for label in name.trim_end_matches('.').split('.') {
        let size = u8::try_from(label.len())
            .ok()
            .filter(|size| (1..=63).contains(size))
            .ok_or_else(|| bad("a part of the name is empty or longer than 63 bytes"))?;
        out.push(size);
        out.extend_from_slice(label.as_bytes());
        length += 1 + label.len();
    }
    if length > 255 {
        return Err(bad("the name is longer than 255 bytes"));
    }
    out.push(0);
    // Type A, class IN.
    out.extend_from_slice(&[0, 1, 0, 1]);
    Ok(out)
}

/// The IPv4 addresses in the answer `answer` to the question numbered `id`; why not, where it is
/// not one, or says the name has none (any code but "no error").
fn addresses(answer: &[u8], id: u16) -> Result<Vec<Ipv4Addr>, &'static str> {
    let short = "the answer is cut short";
    let word = |at: usize| -> Result<u16, &'static str> {
        match answer.get(at..at + 2) {
            Some(&[high, low]) => Ok(u16::from_be_bytes([high, low])),
            _ => Err(short),
        }
    };
    if word(0)? != id {
        return Err("the answer is to another question");
    }
    let flags = word(2)?;
    if flags & 0x8000 == 0 {
        return Err("the answer is not an answer");
    }
    if flags & 0x000f != 0 {
        return Err("the DNS server gave no address for the name");
    }
    let questions = word(4)?;
    let records = word(6)?;
    let mut at = 12;
    for _ in 0..questions {
        at = past_name(answer, at).ok_or(short)? + 4;
    }
    let mut found = Vec::new();
    for _ in 0..records {
        at = past_name(answer, at).ok_or(short)?;
        let kind = word(at)?;
        let class = word(at + 2)?;
        let size = usize::from(word(at + 8)?);
        let data = answer.get(at + 10..at + 10 + size).ok_or(short)?;
        if let (1, 1, &[a, b, c, d]) = (kind, class, data) {
            found.push(Ipv4Addr::new(a, b, c, d));
        }
        at += 10 + size;
    }
    Ok(found)
}

/// Where the name that starts at `at` in `message` ends: its labels and their end, or a pointer
/// to a name written before.
fn past_name(message: &[u8], mut at: usize) -> Option<usize> {
    loop {
        let size = *message.get(at)?;
        match size {
            0 => return Some(at + 1),
            pointer if pointer & 0xc0 == 0xc0 => {
                message.get(at + 1)?;
                return Some(at + 2);
            }
            label if label <= 63 => at += 1 + usize::from(label),
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An answer to question `id` for `a.bench.test`: its question, then each of `ips` as an A
    /// record pointing back at the question's name, with `code` as its answer code.
    fn answer(id: u16, code: u8, ips: &[[u8; 4]]) -> Vec<u8> {
        let mut out = question(id, "a.bench.test").unwrap();
        out[2] = 0x81;
        out[3] = 0x80 | code;
        out[7] = u8::try_from(ips.len()).unwrap();
        for ip in ips {
            out.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 0, 0, 4]);
            out.extend_from_slice(ip);
        }
        out
    }

    #[test]
    fn the_question_asks_for_the_names_ipv4_addresses() {
        let asked = question(0x1234, "a.bench.test.").unwrap();
        let mut want = vec![0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        want.extend_from_slice(b"\x01a\x05bench\x04test\x00\x00\x01\x00\x01");
        assert_eq!(asked, want);
        assert!(question(1, "a..test").is_err());
        assert!(question(1, &format!("{}.test", "x".repeat(64))).is_err());
        assert!(question(1, &format!("{}test", "x.".repeat(130))).is_err());
    }

    #[test]
    fn an_answer_gives_its_addresses_and_a_wrong_one_none() {
        let two = answer(7, 0, &[[127, 0, 0, 1], [10, 0, 0, 2]]);
        assert_eq!(
            addresses(&two, 7),
            Ok(vec![Ipv4Addr::LOCALHOST, Ipv4Addr::new(10, 0, 0, 2)])
        );
        assert_eq!(addresses(&answer(7, 0, &[]), 7), Ok(vec![]));
        assert!(addresses(&two, 8).is_err(), "another question's answer");
        assert!(addresses(&answer(7, 3, &[]), 7).is_err(), "no such name");
        assert!(
            addresses(&question(7, "a.test").unwrap(), 7).is_err(),
            "a question"
        );
        for cut in [0, 5, 12, two.len() - 1] {
            assert!(addresses(&two[..cut], 7).is_err(), "cut at {cut}");
        }
    }

    #[test]
    fn only_names_under_test_are_asked_of_the_server() {
        assert!(under_test("a.bench.test"));
        assert!(under_test("a.bench.test."));
        assert!(!under_test("example.com"));
        assert!(!under_test("contest"));
    }

    #[tokio::test]
    async fn a_name_is_asked_of_the_server_and_its_answer_taken() {
        let server = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let at = server.local_addr().unwrap();
        tokio::spawn(async move {
            let mut asked = [0u8; MOST];
            let (read, from) = server.recv_from(&mut asked).await.unwrap();
            let id = u16::from_be_bytes([asked[0], asked[1]]);
            assert_eq!(
                &asked[2..read],
                &question(id, "slow.bench.test").unwrap()[2..]
            );
            let mut said = asked[..read].to_vec();
            said[2] = 0x81;
            said[3] = 0x80;
            said[7] = 1;
            said.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 0, 0, 4, 127, 0, 0, 1]);
            server.send_to(&said, from).await.unwrap();
        });
        let found = ask(at, "slow.bench.test").await.unwrap();
        assert_eq!(found, vec![SocketAddr::from(([127, 0, 0, 1], 0))]);
    }
}
