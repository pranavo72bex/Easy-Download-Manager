// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// rtsps://: retina is handed a loopback rtsp:// address with the
/// path, query and port it needs, and what it sends there reaches the
/// server over TLS, and the server's answer comes back.
#[tokio::test]
async fn an_rtsps_address_is_carried_over_tls() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_rustls::rustls;
    let made = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let cert = made.cert.der().clone();
    let key = rustls::pki_types::PrivateKeyDer::try_from(made.signing_key.serialize_der()).unwrap();
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let server = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert.clone()], key)
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(server));
    let served = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut tls = acceptor.accept(tcp).await.unwrap();
        let mut asked = vec![0u8; 64];
        let n = tls.read(&mut asked).await.unwrap();
        tls.write_all(b"RTSP/1.0 200 OK\r\n\r\n").await.unwrap();
        tls.flush().await.unwrap();
        String::from_utf8_lossy(&asked[..n]).into_owned()
    });
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert).unwrap();
    let client = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let address = url::Url::parse(&format!("rtsps://localhost:{port}/cam/one?x=1")).unwrap();
    let (local, _task) = bridge(&address, std::sync::Arc::new(client)).await.unwrap();
    assert_eq!(local.scheme(), "rtsp");
    assert_eq!(local.host_str(), Some("127.0.0.1"));
    assert_eq!(local.path(), "/cam/one");
    assert_eq!(local.query(), Some("x=1"));
    let mut plain = tokio::net::TcpStream::connect(("127.0.0.1", local.port().unwrap()))
        .await
        .unwrap();
    plain
        .write_all(b"OPTIONS * RTSP/1.0\r\n\r\n")
        .await
        .unwrap();
    let mut answer = vec![0u8; 64];
    let n = plain.read(&mut answer).await.unwrap();
    assert!(answer[..n].starts_with(b"RTSP/1.0 200 OK"));
    assert!(served.await.unwrap().starts_with("OPTIONS * RTSP/1.0"));
}

#[test]
fn a_sign_in_is_taken_out_of_the_address() {
    use crate::test_secrets::{escaped, marked, secret};
    // A password with the characters an address must escape.
    let (user, password) = (secret("rtsp_user"), marked("rtsp"));
    let address = format!("rtsp://{user}:{}@cam.example:554/s1", escaped(&password));
    let (address, creds) = split_credentials(&address).unwrap();
    assert_eq!(address.as_str(), "rtsp://cam.example:554/s1");
    let creds = creds.unwrap();
    assert_eq!((creds.username, creds.password), (user, password));
    let (_, none) = split_credentials("rtsp://cam.example/s1").unwrap();
    assert!(none.is_none());
}

/// What the small server below does after PLAY.
#[derive(Clone, Copy)]
enum After {
    HangUp,
    Silent,
    /// Two AAC frames an hour apart in the stream's own time, then silent (gaps row H34d).
    Jump,
}

/// A one-client RTSP server describing `sdp` (or refusing DESCRIBE when there is none),
/// answering SETUP and PLAY, then sending no picture.
async fn serve(sdp: Option<&'static str>, after: After) -> u16 {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read, mut write) = socket.into_split();
        let mut lines = BufReader::new(read).lines();
        loop {
            let mut request = Vec::new();
            while let Ok(Some(line)) = lines.next_line().await {
                if line.is_empty() {
                    break;
                }
                request.push(line);
            }
            let Some(first) = request.first().cloned() else {
                return;
            };
            let cseq = request
                .iter()
                .find_map(|l| l.strip_prefix("CSeq: "))
                .unwrap_or("0")
                .to_string();
            let url = first.split(' ').nth(1).unwrap_or("").to_string();
            let method = first.split(' ').next().unwrap_or("").to_string();
            let reply = match (method.as_str(), sdp) {
                ("DESCRIBE", None) => {
                    format!("RTSP/1.0 404 Not Found\r\nCSeq: {cseq}\r\n\r\n")
                }
                ("DESCRIBE", Some(sdp)) => format!(
                    "RTSP/1.0 200 OK\r\nCSeq: {cseq}\r\nContent-Base: {url}/\r\nContent-Type: application/sdp\r\nContent-Length: {}\r\n\r\n{sdp}",
                    sdp.len()
                ),
                ("SETUP", _) => format!("RTSP/1.0 200 OK\r\nCSeq: {cseq}\r\nSession: 12345678;timeout=60\r\nTransport: RTP/AVP/TCP;unicast;interleaved=0-1;ssrc=0000ABCD\r\n\r\n"),
                ("PLAY", _) => format!("RTSP/1.0 200 OK\r\nCSeq: {cseq}\r\nSession: 12345678\r\n\r\n"),
                _ => format!("RTSP/1.0 200 OK\r\nCSeq: {cseq}\r\nSession: 12345678\r\n\r\n"),
            };
            if write.write_all(reply.as_bytes()).await.is_err() {
                return;
            }
            if method == "PLAY" {
                break;
            }
        }
        match after {
            After::HangUp => {
                let _ = write.shutdown().await;
            }
            After::Silent => {
                // Kept open, sending nothing, until the client lets go.
                while let Ok(Some(_)) = lines.next_line().await {}
            }
            After::Jump => {
                for (seq, at) in [(1u16, 0u32), (2, 44_100 * 3600)] {
                    // RFC 3640's AAC-hbr: AU-headers-length (16 bits), one AU-header (a 13-bit
                    // size and a 3-bit index), then the frame.
                    let frame = [0x21u8; 64];
                    let header = (frame.len() as u16) << 3;
                    let mut rtp = vec![0x80, 0x80 | 97];
                    rtp.extend_from_slice(&seq.to_be_bytes());
                    rtp.extend_from_slice(&at.to_be_bytes());
                    rtp.extend_from_slice(&[0, 0, 0xAB, 0xCD]);
                    rtp.extend_from_slice(&16u16.to_be_bytes());
                    rtp.extend_from_slice(&header.to_be_bytes());
                    rtp.extend_from_slice(&frame);
                    let mut packet = vec![b'$', 0];
                    packet.extend_from_slice(&(rtp.len() as u16).to_be_bytes());
                    packet.extend_from_slice(&rtp);
                    if write.write_all(&packet).await.is_err() {
                        return;
                    }
                }
                while let Ok(Some(_)) = lines.next_line().await {}
            }
        }
    });
    port
}

const PICTURE: &str = "v=0\r\no=- 0 0 IN IP4 127.0.0.1\r\ns=dm test\r\nt=0 0\r\nm=video 0 RTP/AVP 96\r\nc=IN IP4 0.0.0.0\r\na=rtpmap:96 H264/90000\r\na=fmtp:96 packetization-mode=1\r\na=control:track1\r\n";
const AAC_SOUND: &str = "v=0\r\no=- 0 0 IN IP4 127.0.0.1\r\ns=dm test\r\nt=0 0\r\nm=audio 0 RTP/AVP 97\r\nc=IN IP4 0.0.0.0\r\na=rtpmap:97 MPEG4-GENERIC/44100/2\r\na=fmtp:97 streamtype=5;profile-level-id=15;mode=AAC-hbr;config=1210;SizeLength=13;IndexLength=3;IndexDeltaLength=3\r\na=control:track1\r\n";
const TELEPHONE_SOUND: &str = "v=0\r\no=- 0 0 IN IP4 127.0.0.1\r\ns=dm test\r\nt=0 0\r\nm=audio 0 RTP/AVP 0\r\nc=IN IP4 0.0.0.0\r\na=rtpmap:0 PCMU/8000\r\na=control:track1\r\n";

async fn record_from(port: u16, control: &Control) -> (Result<u64, Error>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("cam.part");
    let got = record(
        &format!("rtsp://127.0.0.1:{port}/cam"),
        &part,
        control,
        &Arc::new(AtomicU64::new(0)),
    )
    .await;
    (got, dir)
}

#[tokio::test]
async fn a_stream_with_nothing_dm_can_record_is_refused_and_says_so() {
    let port = serve(Some(TELEPHONE_SOUND), After::HangUp).await;
    let (got, _dir) = record_from(port, &Control::new()).await;
    let why = got.unwrap_err().to_string();
    assert!(why.contains("no H.264, H.265 or AAC"), "{why}");
}

#[tokio::test]
async fn a_refused_description_is_the_servers_error() {
    let port = serve(None, After::HangUp).await;
    let (got, _dir) = record_from(port, &Control::new()).await;
    let why = got.unwrap_err().to_string();
    assert!(why.contains("the RTSP server:"), "{why}");
}

#[tokio::test]
async fn a_stream_ended_before_any_picture_is_an_error_and_writes_no_file() {
    let port = serve(Some(PICTURE), After::HangUp).await;
    let (got, dir) = record_from(port, &Control::new()).await;
    let why = got.unwrap_err().to_string();
    assert!(why.contains("before sending any of it"), "{why}");
    assert!(!dir.path().join("cam.part").exists());
}

#[tokio::test]
async fn a_silent_stream_is_stopped_by_its_control() {
    let port = serve(Some(PICTURE), After::Silent).await;
    let control = Control::new();
    let stopper = control.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        stopper.cancel();
    });
    let started = std::time::Instant::now();
    let (got, _dir) = record_from(port, &control).await;
    assert!(matches!(got, Err(Error::Cancelled)), "{got:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
}

/// The silence is counted from the last frame, not from the stop check's last look: a server
/// silent for longer than it is said to be (it was once waited on for ever).
#[tokio::test]
async fn a_server_that_goes_silent_is_said_to_have() {
    let port = serve(Some(PICTURE), After::Silent).await;
    let dir = tempfile::tempdir().unwrap();
    let url = format!("rtsp://127.0.0.1:{port}/cam");
    let path = dir.path().join("cam.part");
    let (control, bytes) = (Control::new(), Arc::new(AtomicU64::new(0)));
    let recording = record_within(&url, &path, &control, &bytes, Duration::from_millis(900));
    let got = tokio::time::timeout(Duration::from_secs(20), recording)
        .await
        .expect("the silence was waited on for ever");
    let why = got.unwrap_err().to_string();
    assert_eq!(why, "the RTSP server sent nothing for 30 seconds");
    assert!(!dir.path().join("cam.part").exists());
}

#[tokio::test]
async fn an_rtsps_server_that_does_not_answer_is_an_error() {
    // A port nothing listens on: taken, then let go.
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let dir = tempfile::tempdir().unwrap();
    let got = record(
        &format!("rtsps://127.0.0.1:{port}/cam"),
        &dir.path().join("cam.part"),
        &Control::new(),
        &Arc::new(AtomicU64::new(0)),
    )
    .await;
    assert!(got.is_err());
    assert!(!dir.path().join("cam.part").exists());
}

/// Frames held for a stream that never says its shape are kept up to `MOST_HELD_BYTES` and no
/// further (gaps row H34c): ten seconds of a fast stream were held whatever their size.
#[test]
fn frames_held_for_a_stream_that_never_says_its_shape_are_bounded_by_bytes() {
    let mut held = HeldFrames::default();
    let frame = || (0usize, vec![0u8; 8 << 20], 0i64, true, None);
    let kept = (0..12).take_while(|_| held.push(frame())).count();
    assert_eq!(kept, MOST_HELD_BYTES / (8 << 20), "frames kept");
    assert!(held.bytes <= MOST_HELD_BYTES);
    assert_eq!(held.take().len(), kept);
    assert_eq!(held.bytes, 0, "the count kept after they were taken");
    assert!(held.push(frame()), "nothing held, a frame is kept again");
}

/// A server whose stream's times leap an hour ahead ends the recording at once (gaps row H34d):
/// retina is held to jumps of `MOST_JUMP_SECS`; unheld, the recording went on until the server
/// was taken to have gone silent.
#[tokio::test]
async fn a_timestamp_jump_of_hours_ends_the_recording_with_why() {
    let port = serve(Some(AAC_SOUND), After::Jump).await;
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("cam.part");
    let began = std::time::Instant::now();
    let silent_for = Duration::from_secs(20);
    let got = record_within(
        &format!("rtsp://127.0.0.1:{port}/cam"),
        &part,
        &Control::new(),
        &Arc::new(AtomicU64::new(0)),
        silent_for,
    )
    .await;
    assert!(
        began.elapsed() < Duration::from_secs(10),
        "the jump did not end it: {:?} ({got:?})",
        began.elapsed()
    );
}
