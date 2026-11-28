// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

#[test]
fn chunks_are_read_across_reads() {
    let mut data = vec![b'$', b'H', 10, 0];
    data.extend_from_slice(&[0u8; 8]);
    data.extend_from_slice(b"ab");
    data.extend_from_slice(&[b'$', b'D', 11, 0]);
    data.extend_from_slice(&[0u8; 8]);
    data.extend_from_slice(b"xyz");
    let mut buffer = data[..20].to_vec();
    let first = take_chunks(&mut buffer);
    assert_eq!(first.len(), 1);
    assert_eq!(
        (first[0].kind, first[0].body.as_slice()),
        (b'H', &b"ab"[..])
    );
    buffer.extend_from_slice(&data[20..]);
    let second = take_chunks(&mut buffer);
    assert_eq!(
        (second[0].kind, second[0].body.as_slice()),
        (b'D', &b"xyz"[..])
    );
    assert!(buffer.is_empty());
    assert_eq!(
        http_address("mms://old.example/clip.asf").as_deref(),
        Some("http://old.example/clip.asf")
    );
    assert_eq!(http_address("rtsp://x/y"), None);
}

/// An ASF header naming a packet size of 32 and, where `streams` is set,
/// stream 1.
fn asf_header(streams: bool) -> Vec<u8> {
    let mut file = FILE_PROPERTIES.to_vec();
    file.extend_from_slice(&104u64.to_le_bytes());
    file.resize(92, 0);
    file.extend_from_slice(&32u32.to_le_bytes());
    file.resize(104, 0);
    let mut stream = STREAM_PROPERTIES.to_vec();
    stream.extend_from_slice(&78u64.to_le_bytes());
    stream.resize(72, 0);
    stream.extend_from_slice(&[0x01, 0x00]);
    stream.resize(78, 0);
    let objects: Vec<&[u8]> = if streams {
        vec![&file, &stream]
    } else {
        vec![&file]
    };
    let mut header = vec![0u8; 24];
    header.extend_from_slice(&(objects.len() as u32).to_le_bytes());
    header.extend_from_slice(&[1, 2]);
    for o in objects {
        header.extend_from_slice(o);
    }
    header
}

/// An MMSH chunk of `kind` holding `body`.
fn chunk(kind: u8, body: &[u8]) -> Vec<u8> {
    let extra = if matches!(kind, b'H' | b'D') { 8 } else { 0 };
    let mut out = vec![b'$', kind];
    out.extend_from_slice(&((body.len() + extra) as u16).to_le_bytes());
    out.extend(std::iter::repeat_n(0u8, extra));
    out.extend_from_slice(body);
    out
}

/// A server answering the description request with (`status`, `described`)
/// and the playing one with (`playing_status`, `played`).
async fn serve(status: u16, described: Vec<u8>, playing_status: u16, played: Vec<u8>) -> String {
    let app = axum::Router::new().fallback(move |headers: axum::http::HeaderMap| {
        let (described, played) = (described.clone(), played.clone());
        async move {
            let playing = headers
                .get_all("pragma")
                .iter()
                .any(|p| p.to_str().is_ok_and(|p| p.contains("xPlayStrm=1")));
            let (status, body) = if playing {
                (playing_status, played)
            } else {
                (status, described)
            };
            (axum::http::StatusCode::from_u16(status).unwrap(), body)
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("mms://{addr}/live/clip.asf")
}

async fn record_from(url: &str) -> (Result<u64, Error>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let got = record(
        url,
        &dir.path().join("clip.part"),
        &Control::new(),
        &Arc::new(AtomicU64::new(0)),
    )
    .await;
    (got, dir)
}

#[test]
fn a_header_says_its_packet_size_and_its_streams() {
    assert_eq!(header_facts(&asf_header(true)), (Some(32), vec![1]));
    assert_eq!(header_facts(&asf_header(false)), (Some(32), vec![]));
}

#[tokio::test]
async fn what_an_mms_server_refuses_or_leaves_out_is_said() {
    let header = chunk(b'H', &asf_header(true));
    let cases: Vec<(String, &str)> = vec![
        (
            serve(404, Vec::new(), 200, Vec::new()).await,
            "answered 404",
        ),
        (
            serve(200, chunk(b'E', b""), 200, Vec::new()).await,
            "sent no stream header",
        ),
        (
            serve(200, chunk(b'H', &asf_header(false)), 200, Vec::new()).await,
            "names no streams",
        ),
        (
            serve(200, header.clone(), 503, Vec::new()).await,
            "answered 503",
        ),
        (
            serve(200, header.clone(), 200, chunk(b'E', b"")).await,
            "before sending any of it",
        ),
    ];
    for (url, said) in cases {
        let (got, _dir) = record_from(&url).await;
        let why = got.unwrap_err().to_string();
        assert!(why.contains(said), "{why} is not {said:?}");
    }
    let (got, _dir) = record_from("http://127.0.0.1:1/clip.asf").await;
    assert!(got.unwrap_err().to_string().contains("not an mms://"));
}

#[tokio::test]
async fn packets_are_padded_to_the_headers_size_and_others_left_out() {
    let header = asf_header(true);
    let mut played = chunk(b'D', b"early");
    played.extend(chunk(b'H', &header));
    played.extend(chunk(b'C', b"change"));
    played.extend(chunk(b'D', b"12345"));
    played.extend(chunk(b'E', b""));
    played.extend(chunk(b'D', b"after the end"));
    let url = serve(200, chunk(b'H', &header), 200, played).await;
    let (got, dir) = record_from(&url).await;
    let written = got.unwrap();
    let data = std::fs::read(dir.path().join("clip.part")).unwrap();
    let mut packet = b"12345".to_vec();
    packet.resize(32, 0);
    assert_eq!(data, [header, packet].concat());
    assert_eq!(written, data.len() as u64);
}

/// A server that describes its stream and, asked to play it, answers and then sends nothing,
/// with the connection held open.
async fn serve_silent() -> String {
    let described = chunk(b'H', &asf_header(true));
    let app = axum::Router::new().fallback(move |headers: axum::http::HeaderMap| {
        let described = described.clone();
        async move {
            let playing = headers
                .get_all("pragma")
                .iter()
                .any(|p| p.to_str().is_ok_and(|p| p.contains("xPlayStrm=1")));
            if playing {
                let nothing = futures_util::stream::pending::<Result<Vec<u8>, std::io::Error>>();
                axum::body::Body::from_stream(nothing)
            } else {
                axum::body::Body::from(described)
            }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("mms://{addr}/live/clip.asf")
}

/// The silence is counted from the last bytes, not from the stop check's last look: a server
/// silent for longer than it is said to be (it was once waited on for ever).
#[tokio::test]
async fn a_server_that_goes_silent_is_said_to_have() {
    let url = serve_silent().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("clip.part");
    let (control, bytes) = (Control::new(), Arc::new(AtomicU64::new(0)));
    let recording = record_within(&url, &path, &control, &bytes, Duration::from_millis(900));
    let got = tokio::time::timeout(Duration::from_secs(20), recording)
        .await
        .expect("the silence was waited on for ever");
    let why = got.unwrap_err().to_string();
    assert_eq!(why, "the MMS server sent nothing for 30 seconds");
}

proptest::proptest! {
    #[test]
    fn any_bytes_are_read_without_a_panic(data in proptest::collection::vec(proptest::num::u8::ANY, 0..800)) {
        let _ = header_facts(&data);
        let mut buffer = data.clone();
        let _ = take_chunks(&mut buffer);
    }
}

/// An object size near the top of the address space ends the header's reading, where `at + 16`
/// overflowed and panicked; a packet size over what a chunk can carry is not taken (gaps row
/// H10).
#[test]
fn a_huge_object_or_packet_size_is_refused_not_panicked_on() {
    let mut header = vec![0u8; 24];
    header.extend_from_slice(&2u32.to_le_bytes());
    header.extend_from_slice(&[1, 2]);
    let mut object = vec![0u8; 16];
    object.extend_from_slice(&((usize::MAX - 30) as u64).to_le_bytes());
    header.extend_from_slice(&object);
    assert_eq!(header_facts(&header), (None, Vec::new()));

    let mut file = FILE_PROPERTIES.to_vec();
    file.extend_from_slice(&104u64.to_le_bytes());
    file.resize(92, 0);
    file.extend_from_slice(&(1u32 << 20).to_le_bytes());
    file.resize(104, 0);
    let mut big = vec![0u8; 24];
    big.extend_from_slice(&1u32.to_le_bytes());
    big.extend_from_slice(&[1, 2]);
    big.extend_from_slice(&file);
    assert_eq!(header_facts(&big).0, None, "a 1 MiB packet size was taken");
}
