//! dm: BitTorrent Message Stream Encryption (MSE, also called PE), the
//! obfuscated handshake and RC4 stream other clients use, so a peer that
//! accepts only encrypted connections is reached, and one that connects
//! encrypted is taken (desktop/vendor/README.md, dm's gaps row 150).
//!
//! The exchange, as the specification lays it out (A connects, B answers):
//!
//! 1. A→B: Diffie-Hellman Ya, PadA
//! 2. B→A: Diffie-Hellman Yb, PadB
//! 3. A→B: HASH('req1', S), HASH('req2', SKEY) xor HASH('req3', S),
//!    ENCRYPT(VC, crypto_provide, len(PadC), PadC, len(IA)), ENCRYPT(IA)
//! 4. B→A: ENCRYPT(VC, crypto_select, len(PadD), PadD), ENCRYPT2(payload)
//! 5. A→B: ENCRYPT2(payload)
//!
//! The group is the 768-bit prime below with generator 2; HASH is SHA-1; SKEY
//! is the torrent's info hash; VC is eight zero bytes; ENCRYPT is RC4 keyed
//! HASH('keyA', S, SKEY) from A and HASH('keyB', S, SKEY) from B, with the
//! first 1024 bytes of each keystream thrown away. This side always asks for
//! RC4 (crypto 2) and sends its BitTorrent handshake after the exchange, not
//! inside it (len(IA) 0); as B it takes an initial payload a peer sends.

use std::io::IoSliceMut;
use std::pin::Pin;
use std::task::{Context, Poll};

use anyhow::{Context as _, bail};
use librqbit_core::hash_id::Id20;
use num_bigint::BigUint;
use rc4::{KeyInit, Rc4, StreamCipher};
use sha1::{Digest, Sha1};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};

use crate::type_aliases::{BoxAsyncReadVectored, BoxAsyncWrite};
use crate::vectored_traits::AsyncReadVectored;

/// The specification's prime, 768 bits.
const PRIME: &str = "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9A63A36210000000000090563";
/// A public key and the shared secret, in bytes.
const KEY_LEN: usize = 96;
/// The verification constant.
const VC: [u8; 8] = [0; 8];
/// Crypto methods, as bits of `crypto_provide` and `crypto_select`.
const CRYPTO_PLAINTEXT: u32 = 0x01;
const CRYPTO_RC4: u32 = 0x02;
/// The longest any of the pads may be.
const MAX_PAD: usize = 512;
/// What the plaintext BitTorrent handshake starts with.
const PLAINTEXT_START: &[u8; 20] = b"\x13BitTorrent protocol";

fn prime() -> BigUint {
    BigUint::parse_bytes(PRIME.as_bytes(), 16).expect("the specification's prime is valid hex")
}

/// `n` as `KEY_LEN` big-endian bytes.
fn to_key_bytes(n: &BigUint) -> [u8; KEY_LEN] {
    let bytes = n.to_bytes_be();
    let mut out = [0u8; KEY_LEN];
    out[KEY_LEN - bytes.len()..].copy_from_slice(&bytes);
    out
}

/// A private key of 160 bits and its public key.
fn key_pair() -> (BigUint, [u8; KEY_LEN]) {
    let secret = BigUint::from_bytes_be(&rand::random::<[u8; 20]>());
    let public = BigUint::from(2u32).modpow(&secret, &prime());
    (secret, to_key_bytes(&public))
}

/// The shared secret from the peer's public key; a key that would make it
/// trivial (0, 1, or P-1 and above) is refused.
fn shared_secret(secret: &BigUint, theirs: &[u8]) -> anyhow::Result<[u8; KEY_LEN]> {
    let p = prime();
    let y = BigUint::from_bytes_be(theirs);
    if y <= BigUint::from(1u32) || y >= &p - 1u32 {
        bail!("the peer's encryption key is not one that can be used");
    }
    Ok(to_key_bytes(&y.modpow(secret, &p)))
}

fn hash(parts: &[&[u8]]) -> [u8; 20] {
    let mut sha = Sha1::new();
    for part in parts {
        sha.update(part);
    }
    let mut out = [0u8; 20];
    out.copy_from_slice(&sha.finalize());
    out
}

/// RC4 keyed `key`, past the first 1024 bytes of its keystream.
fn cipher(key: &[u8; 20]) -> Rc4 {
    let mut rc4 = Rc4::new_from_slice(key).expect("a 20-byte key is a valid RC4 key");
    let mut discard = [0u8; 1024];
    rc4.apply_keystream(&mut discard);
    rc4
}

/// Random padding, 0 to `MAX_PAD` bytes.
fn padding() -> Vec<u8> {
    let len = usize::from(rand::random::<u16>()) % (MAX_PAD + 1);
    (0..len).map(|_| rand::random::<u8>()).collect()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Reads until `got` holds at least `need` bytes.
async fn fill(
    read: &mut BoxAsyncReadVectored,
    got: &mut Vec<u8>,
    need: usize,
) -> anyhow::Result<()> {
    let mut chunk = [0u8; 1024];
    while got.len() < need {
        let n = read
            .read(&mut chunk)
            .await
            .context("reading the encryption handshake")?;
        if n == 0 {
            bail!("the peer closed the connection during the encryption handshake");
        }
        got.extend_from_slice(&chunk[..n]);
    }
    Ok(())
}

/// Reads at least one more byte into `got`.
async fn more(read: &mut BoxAsyncReadVectored, got: &mut Vec<u8>) -> anyhow::Result<()> {
    let need = got.len() + 1;
    fill(read, got, need).await
}

/// Whether a plaintext handshake failed the way a peer that wants encryption
/// refuses one: it closed or reset the connection instead of answering.
pub(crate) fn plaintext_refused(e: &crate::Error) -> bool {
    matches!(
        e,
        crate::Error::PeerDisconnectedReadingHandshake
            | crate::Error::ReadHandshake(_)
            | crate::Error::WriteHandshake(_)
    )
}

/// The connecting side of the exchange: returns the connection's halves,
/// encrypted, for the BitTorrent handshake to go over.
pub(crate) async fn initiate(
    mut read: BoxAsyncReadVectored,
    mut write: BoxAsyncWrite,
    info_hash: Id20,
) -> anyhow::Result<(BoxAsyncReadVectored, BoxAsyncWrite)> {
    let skey = info_hash.0;
    let (secret, public) = key_pair();
    // 1: our public key and padding.
    write.write_all(&[&public[..], &padding()].concat()).await?;
    // 2: theirs, then padding of a length we are not told.
    let mut got = Vec::new();
    fill(&mut read, &mut got, KEY_LEN).await?;
    let s = shared_secret(&secret, &got[..KEY_LEN])?;
    let mut encrypt = cipher(&hash(&[b"keyA", &s, &skey]));
    let decrypt_key = hash(&[b"keyB", &s, &skey]);

    // 3: which torrent, proven by the shared secret; what we offer.
    let mut message = Vec::with_capacity(40 + 16);
    message.extend_from_slice(&hash(&[b"req1", &s]));
    let req2 = hash(&[b"req2", &skey]);
    let req3 = hash(&[b"req3", &s]);
    message.extend(req2.iter().zip(req3.iter()).map(|(a, b)| a ^ b));
    let mut offer = Vec::with_capacity(16);
    offer.extend_from_slice(&VC);
    offer.extend_from_slice(&CRYPTO_RC4.to_be_bytes());
    offer.extend_from_slice(&0u16.to_be_bytes()); // len(PadC)
    offer.extend_from_slice(&0u16.to_be_bytes()); // len(IA)
    encrypt.apply_keystream(&mut offer);
    message.extend_from_slice(&offer);
    write.write_all(&message).await?;
    write.flush().await?;

    // 4: their verification constant, encrypted, after their padding: the
    // first eight bytes of their keystream, as VC is zeros.
    let mut decrypt = cipher(&decrypt_key);
    let mut vc = VC;
    decrypt.apply_keystream(&mut vc);
    let at = loop {
        if let Some(i) = find(&got[KEY_LEN..], &vc) {
            break KEY_LEN + i;
        }
        if got.len() > KEY_LEN + MAX_PAD + VC.len() {
            bail!("the peer did not answer the encryption handshake");
        }
        more(&mut read, &mut got).await?;
    };
    let fields = at + VC.len();
    fill(&mut read, &mut got, fields + 6).await?;
    let mut head = got[fields..fields + 6].to_vec();
    decrypt.apply_keystream(&mut head);
    let select = u32::from_be_bytes([head[0], head[1], head[2], head[3]]);
    let pad_d = usize::from(u16::from_be_bytes([head[4], head[5]]));
    if pad_d > MAX_PAD {
        bail!("the peer's encryption handshake is malformed");
    }
    if select != CRYPTO_RC4 {
        bail!("the peer did not choose encryption");
    }
    let payload = fields + 6 + pad_d;
    fill(&mut read, &mut got, payload).await?;
    let mut pad = got[fields + 6..payload].to_vec();
    decrypt.apply_keystream(&mut pad);
    let mut early = got[payload..].to_vec();
    decrypt.apply_keystream(&mut early);
    Ok((
        Box::new(EncryptedRead::new(read, Some(decrypt), early)),
        Box::new(EncryptedWrite::new(write, Some(encrypt))),
    ))
}

/// The answering side: a plaintext handshake is passed through untouched
/// (unless `require` says only encrypted ones are taken); an encrypted one is
/// answered for whichever of `info_hashes` it names. Returns the halves for
/// the BitTorrent handshake to be read from.
pub(crate) async fn accept(
    mut read: BoxAsyncReadVectored,
    mut write: BoxAsyncWrite,
    require: bool,
    info_hashes: &[Id20],
) -> anyhow::Result<(BoxAsyncReadVectored, BoxAsyncWrite)> {
    let mut got = Vec::new();
    fill(&mut read, &mut got, PLAINTEXT_START.len()).await?;
    if got[..PLAINTEXT_START.len()] == PLAINTEXT_START[..] {
        if require {
            bail!("a plaintext connection, where only encrypted ones are taken");
        }
        // Read for the encryption check: given back to the handshake reader.
        return Ok((Box::new(EncryptedRead::new(read, None, got)), write));
    }

    // 1: their public key, then padding.
    fill(&mut read, &mut got, KEY_LEN).await?;
    let (secret, public) = key_pair();
    let s = shared_secret(&secret, &got[..KEY_LEN])?;
    // 2: ours.
    write.write_all(&[&public[..], &padding()].concat()).await?;
    write.flush().await?;

    // 3: found by HASH('req1', S), after their padding.
    let req1 = hash(&[b"req1", &s]);
    let at = loop {
        if let Some(i) = find(&got[KEY_LEN..], &req1) {
            break KEY_LEN + i;
        }
        if got.len() > KEY_LEN + MAX_PAD + req1.len() {
            bail!("the peer's encryption handshake named nothing");
        }
        more(&mut read, &mut got).await?;
    };
    let named = at + 20;
    fill(&mut read, &mut got, named + 20).await?;
    let req3 = hash(&[b"req3", &s]);
    let skey = info_hashes
        .iter()
        .map(|h| h.0)
        .find(|skey| {
            let req2 = hash(&[b"req2", skey]);
            req2.iter()
                .zip(req3.iter())
                .map(|(a, b)| a ^ b)
                .eq(got[named..named + 20].iter().copied())
        })
        .context("the encrypted connection is for no torrent here")?;
    let mut decrypt = cipher(&hash(&[b"keyA", &s, &skey]));
    let mut encrypt = cipher(&hash(&[b"keyB", &s, &skey]));

    let fields = named + 20;
    fill(&mut read, &mut got, fields + 14).await?;
    let mut head = got[fields..fields + 14].to_vec();
    decrypt.apply_keystream(&mut head);
    if head[..8] != VC {
        bail!("the peer's encryption handshake did not verify");
    }
    let provide = u32::from_be_bytes([head[8], head[9], head[10], head[11]]);
    let pad_c = usize::from(u16::from_be_bytes([head[12], head[13]]));
    if pad_c > MAX_PAD {
        bail!("the peer's encryption handshake is malformed");
    }
    let ia_len_at = fields + 14 + pad_c;
    fill(&mut read, &mut got, ia_len_at + 2).await?;
    let mut tail = got[fields + 14..ia_len_at + 2].to_vec();
    decrypt.apply_keystream(&mut tail);
    let ia_len = usize::from(u16::from_be_bytes([tail[pad_c], tail[pad_c + 1]]));
    let payload = ia_len_at + 2 + ia_len;
    fill(&mut read, &mut got, payload).await?;
    // The initial payload, encrypted whatever is chosen after.
    let mut early = got[ia_len_at + 2..payload].to_vec();
    decrypt.apply_keystream(&mut early);

    let select = if provide & CRYPTO_RC4 != 0 {
        CRYPTO_RC4
    } else if provide & CRYPTO_PLAINTEXT != 0 && !require {
        CRYPTO_PLAINTEXT
    } else {
        bail!("the peer offered no encryption this side takes");
    };
    // 4: our verification constant and choice, no padding.
    let mut reply = Vec::with_capacity(14);
    reply.extend_from_slice(&VC);
    reply.extend_from_slice(&select.to_be_bytes());
    reply.extend_from_slice(&0u16.to_be_bytes());
    encrypt.apply_keystream(&mut reply);
    write.write_all(&reply).await?;
    write.flush().await?;

    let mut rest = got[payload..].to_vec();
    if select == CRYPTO_RC4 {
        decrypt.apply_keystream(&mut rest);
        early.extend_from_slice(&rest);
        Ok((
            Box::new(EncryptedRead::new(read, Some(decrypt), early)),
            Box::new(EncryptedWrite::new(write, Some(encrypt))),
        ))
    } else {
        early.extend_from_slice(&rest);
        Ok((Box::new(EncryptedRead::new(read, None, early)), write))
    }
}

/// A connection's reading half: bytes already read (and decrypted) given
/// first, then the rest, decrypted where there is a cipher.
pub(crate) struct EncryptedRead<R> {
    inner: R,
    cipher: Option<Rc4>,
    early: Vec<u8>,
    at: usize,
}

impl<R> EncryptedRead<R> {
    fn new(inner: R, cipher: Option<Rc4>, early: Vec<u8>) -> Self {
        Self {
            inner,
            cipher,
            early,
            at: 0,
        }
    }

    /// Copies what is left of the early bytes into `out`; how many.
    fn give_early(&mut self, out: &mut [u8]) -> usize {
        let n = out.len().min(self.early.len() - self.at);
        out[..n].copy_from_slice(&self.early[self.at..self.at + n]);
        self.at += n;
        if self.at == self.early.len() {
            self.early = Vec::new();
            self.at = 0;
        }
        n
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for EncryptedRead<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        if this.at < this.early.len() {
            let n = this.give_early(buf.initialize_unfilled());
            buf.advance(n);
            return Poll::Ready(Ok(()));
        }
        let before = buf.filled().len();
        std::task::ready!(Pin::new(&mut this.inner).poll_read(cx, buf))?;
        if let Some(cipher) = this.cipher.as_mut() {
            cipher.apply_keystream(&mut buf.filled_mut()[before..]);
        }
        Poll::Ready(Ok(()))
    }
}

impl<R: AsyncReadVectored + Unpin> AsyncReadVectored for EncryptedRead<R> {
    fn poll_read_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        vec: &mut [IoSliceMut<'_>],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        if this.at < this.early.len() {
            let mut total = 0;
            for slice in vec.iter_mut() {
                if this.at >= this.early.len() {
                    break;
                }
                total += this.give_early(slice);
            }
            return Poll::Ready(Ok(total));
        }
        let n = std::task::ready!(Pin::new(&mut this.inner).poll_read_vectored(cx, vec))?;
        if let Some(cipher) = this.cipher.as_mut() {
            let mut left = n;
            for slice in vec.iter_mut() {
                if left == 0 {
                    break;
                }
                let k = slice.len().min(left);
                cipher.apply_keystream(&mut slice[..k]);
                left -= k;
            }
        }
        Poll::Ready(Ok(n))
    }
}

/// A connection's writing half, encrypting where there is a cipher. What it
/// has encrypted is written whole before it takes more, so the keystream
/// never runs ahead of what the peer gets.
pub(crate) struct EncryptedWrite<W> {
    inner: W,
    cipher: Option<Rc4>,
    pending: Vec<u8>,
    at: usize,
    /// The length of the caller's buffer the pending bytes were made from,
    /// answered once they are all written.
    taken: Option<usize>,
}

impl<W> EncryptedWrite<W> {
    fn new(inner: W, cipher: Option<Rc4>) -> Self {
        Self {
            inner,
            cipher,
            pending: Vec::new(),
            at: 0,
            taken: None,
        }
    }
}

impl<W: AsyncWrite + Unpin> EncryptedWrite<W> {
    /// Writes out what is pending.
    fn drain(&mut self, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        while self.at < self.pending.len() {
            let n = std::task::ready!(
                Pin::new(&mut self.inner).poll_write(cx, &self.pending[self.at..])
            )?;
            if n == 0 {
                return Poll::Ready(Err(std::io::ErrorKind::WriteZero.into()));
            }
            self.at += n;
        }
        self.pending.clear();
        self.at = 0;
        Poll::Ready(Ok(()))
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for EncryptedWrite<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        let Some(cipher) = this.cipher.as_mut() else {
            return Pin::new(&mut this.inner).poll_write(cx, buf);
        };
        if this.taken.is_none() {
            if buf.is_empty() {
                return Poll::Ready(Ok(0));
            }
            this.pending.clear();
            this.pending.extend_from_slice(buf);
            cipher.apply_keystream(&mut this.pending);
            this.at = 0;
            this.taken = Some(buf.len());
        }
        std::task::ready!(this.drain(cx))?;
        Poll::Ready(Ok(this.taken.take().unwrap_or(0)))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        std::task::ready!(this.drain(cx))?;
        Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        std::task::ready!(this.drain(cx))?;
        Pin::new(&mut this.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vectored_traits::AsyncReadVectoredIntoCompat;

    fn halves(stream: tokio::io::DuplexStream) -> (BoxAsyncReadVectored, BoxAsyncWrite) {
        let (r, w) = tokio::io::split(stream);
        (Box::new(r.into_vectored_compat()), Box::new(w))
    }

    fn id(byte: u8) -> Id20 {
        Id20::new([byte; 20])
    }

    /// Both sides of the exchange agree, and what each writes after it the
    /// other reads, while the bytes on the wire are not the bytes sent.
    #[tokio::test]
    async fn an_encrypted_connection_carries_both_ways() {
        let (a, b) = tokio::io::duplex(64 * 1024);
        let (ar, aw) = halves(a);
        let (br, bw) = halves(b);
        let torrents = [id(1), id(7), id(9)];
        let (initiated, accepted) =
            tokio::join!(initiate(ar, aw, id(7)), accept(br, bw, true, &torrents));
        let (mut ar, mut aw) = initiated.unwrap();
        let (mut br, mut bw) = accepted.unwrap();
        let hello = b"\x13BitTorrent protocol and the rest of a handshake";
        aw.write_all(hello).await.unwrap();
        aw.flush().await.unwrap();
        let mut got = vec![0u8; hello.len()];
        br.read_exact(&mut got).await.unwrap();
        assert_eq!(&got[..], &hello[..]);
        let answer: Vec<u8> = (0..20_000u32).map(|i| (i % 251) as u8).collect();
        bw.write_all(&answer).await.unwrap();
        bw.flush().await.unwrap();
        let mut got = vec![0u8; answer.len()];
        ar.read_exact(&mut got).await.unwrap();
        assert_eq!(got, answer);
    }

    /// A plaintext handshake is passed through whole where it is taken, and
    /// refused where only encrypted connections are.
    #[tokio::test]
    async fn a_plaintext_handshake_is_passed_through_or_refused() {
        for require in [false, true] {
            let (a, b) = tokio::io::duplex(1024);
            let (_ar, mut aw) = halves(a);
            let (br, bw) = halves(b);
            let mut handshake = PLAINTEXT_START.to_vec();
            handshake.extend_from_slice(&[0u8; 48]);
            aw.write_all(&handshake).await.unwrap();
            aw.flush().await.unwrap();
            match accept(br, bw, require, &[id(1)]).await {
                Ok((mut br, _)) => {
                    assert!(
                        !require,
                        "a plaintext handshake was taken where it is refused"
                    );
                    let mut got = vec![0u8; handshake.len()];
                    br.read_exact(&mut got).await.unwrap();
                    assert_eq!(got, handshake);
                }
                Err(e) => assert!(require, "refused: {e:#}"),
            }
        }
    }

    /// An encrypted connection for a torrent this side does not have is
    /// refused, not answered for another.
    #[tokio::test]
    async fn an_encrypted_connection_for_another_torrent_is_refused() {
        let (a, b) = tokio::io::duplex(64 * 1024);
        let (ar, aw) = halves(a);
        let (br, bw) = halves(b);
        let here = [id(1), id(2)];
        let (initiated, accepted) =
            tokio::join!(initiate(ar, aw, id(5)), accept(br, bw, false, &here));
        assert!(accepted.is_err());
        drop(initiated);
    }

    /// The keystream is RC4 past its first 1024 bytes, and the specification's
    /// prime is the one written down (its top and bottom bytes).
    #[test]
    fn the_constants_are_the_specifications() {
        let p = to_key_bytes(&prime());
        assert_eq!(p[..8], [0xFF; 8]);
        assert_eq!(p[KEY_LEN - 4..], [0x00, 0x09, 0x05, 0x63]);
        let mut plain = cipher(&[3u8; 20]);
        let mut full = Rc4::new_from_slice(&[3u8; 20]).unwrap();
        let mut skipped = [0u8; 1024 + 16];
        full.apply_keystream(&mut skipped);
        let mut ours = [0u8; 16];
        plain.apply_keystream(&mut ours);
        assert_eq!(ours[..], skipped[1024..]);
    }
}
