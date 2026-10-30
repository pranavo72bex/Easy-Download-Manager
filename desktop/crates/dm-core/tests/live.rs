//! Streams from a media server: an RTMP address (and RTMPT, the same
//! through HTTP) fetched by dm's own client, an RTSP camera and an MMS
//! server, each placed as a file; and the refusals where one cannot be — an
//! encrypted scheme, or a proxy it would go around. Every server is the
//! test's own, playing a small clip kept under `tests/fixtures/media/live`,
//! and what dm saved is read back sample by sample and held to the clip.

mod common;

#[path = "common/mp4_samples.rs"]
mod mp4_samples;
#[path = "common/rtmp_server.rs"]
mod rtmp_server;

use dm_core::{client, download, Options};
use futures_util::StreamExt;

use common::{options, tempdir};
use mp4_samples::{length_prefixed_units, tracks};
use rtmp_server::{flv_tags, serve_rtmp, Tag};

/// A clip the test servers play.
fn clip(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/media/live")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Holds the MP4 dm recorded to the FLV clip's tags: the same picture and
/// sound setup, every frame's bytes in order, each at its own time.
fn assert_recorded_as_sent(recorded: &[u8], tags: &[Tag]) {
    let i24 =
        |b: &[u8]| ((i32::from(b[0]) << 16 | i32::from(b[1]) << 8 | i32::from(b[2])) << 8) >> 8;
    let video: Vec<&Tag> = tags.iter().filter(|t| t.kind == 9).collect();
    let audio: Vec<&Tag> = tags.iter().filter(|t| t.kind == 8).collect();
    // FLV: H.264 is codec 7, packet 0 its avcC record, 1 a frame after a
    // composition offset; AAC is format 10, packet 0 its setup.
    let avcc = video.iter().find(|t| t.body[1] == 0).unwrap().body[5..].to_vec();
    let frames: Vec<&Tag> = video.iter().copied().filter(|t| t.body[1] == 1).collect();
    let asc = audio.iter().find(|t| t.body[1] == 0).unwrap().body[2..].to_vec();
    let sounds: Vec<&Tag> = audio.iter().copied().filter(|t| t.body[1] == 1).collect();
    assert!(
        frames.len() >= 20 && sounds.len() >= 80,
        "a clip too short to judge by"
    );

    let got = tracks(recorded);
    assert_eq!(got.len(), 2, "a picture track and a sound track");
    let picture = got
        .iter()
        .find(|t| &t.handler == b"vide")
        .expect("pictures");
    let sound = got.iter().find(|t| &t.handler == b"soun").expect("sound");

    assert_eq!(&picture.codec, b"avc1");
    assert_eq!(picture.config, avcc, "not the clip's H.264 setup");
    assert_eq!((picture.width, picture.height), (160, 120));
    assert_eq!(picture.timescale, 1000);
    assert_eq!(
        picture.samples.len(),
        frames.len(),
        "pictures lost or added"
    );
    let (first_ms, first_dts) = (frames[0].ms, picture.samples[0].dts);
    for (n, (sample, tag)) in picture.samples.iter().zip(&frames).enumerate() {
        assert!(sample.data == tag.body[5..], "picture {n} differs");
        assert_eq!(
            sample.dts - first_dts,
            u64::from(tag.ms - first_ms),
            "picture {n} decoded at another time"
        );
        assert_eq!(
            sample.offset,
            i64::from(i24(&tag.body[2..])),
            "picture {n} shown at another time"
        );
        assert_eq!(sample.sync, tag.body[0] >> 4 == 1, "picture {n}'s key flag");
    }

    assert_eq!(&sound.codec, b"mp4a");
    assert_eq!(sound.config, asc, "not the clip's AAC setup");
    assert_eq!(sound.timescale, 44100);
    assert_eq!(
        sound.samples.len(),
        sounds.len(),
        "sound frames lost or added"
    );
    for (n, (sample, tag)) in sound.samples.iter().zip(&sounds).enumerate() {
        assert!(sample.data == tag.body[2..], "sound frame {n} differs");
        assert_eq!(sample.duration, 1024, "sound frame {n}'s length");
    }
    // About the clip's two seconds, both of them.
    let seconds = |t: &mp4_samples::Track| {
        t.samples.iter().map(|s| u64::from(s.duration)).sum::<u64>() as f64 / f64::from(t.timescale)
    };
    for track in [picture, sound] {
        let s = seconds(track);
        assert!(
            (1.8..2.3).contains(&s),
            "{s} s of {}",
            String::from_utf8_lossy(&track.codec)
        );
    }
}

#[tokio::test]
async fn an_rtmp_stream_is_fetched_by_dm_and_saved_with_every_frame_as_sent() {
    let tags = flv_tags(&clip("clip.flv"));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(serve_rtmp(listener, tags.clone()));

    let dir = tempdir();
    let done = download(
        &client(),
        &format!("rtmp://127.0.0.1:{port}/live/show"),
        &options(dir.path()),
    )
    .await;
    let done = done.expect("the RTMP stream");
    assert_eq!(
        server.await.unwrap(),
        "show",
        "asked to play another stream"
    );
    assert_eq!(done.path.file_name().unwrap(), "show.mp4");
    let recorded = std::fs::read(&done.path).unwrap();
    assert_eq!(done.bytes, recorded.len() as u64);
    assert_recorded_as_sent(&recorded, &tags);
}

/// An RTMPT server in front of an RTMP one, as a web-only network's
/// streaming server is: each POST's bytes go to the RTMP server, and each
/// answer is a polling byte and whatever the RTMP server has sent since.
async fn relay_rtmpt(listener: tokio::net::TcpListener, rtmp_port: u16) {
    use axum::extract::{Path, State};
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    type Shared = Arc<(
        tokio::sync::Mutex<Option<tokio::net::tcp::OwnedWriteHalf>>,
        Mutex<Vec<u8>>,
        std::sync::atomic::AtomicBool,
    )>;
    let shared: Shared = Arc::new((
        tokio::sync::Mutex::new(None),
        Mutex::new(Vec::new()),
        std::sync::atomic::AtomicBool::new(false),
    ));
    /// The bytes waiting, or, once the RTMP server has hung up and all
    /// were taken, a 404: the session is over.
    async fn drained(shared: &Shared) -> axum::response::Response {
        use axum::response::IntoResponse;
        // A moment for the RTMP server's answer to arrive.
        tokio::time::sleep(std::time::Duration::from_millis(15)).await;
        let mut waiting = shared.1.lock().unwrap();
        if waiting.is_empty() && shared.2.load(std::sync::atomic::Ordering::SeqCst) {
            return axum::http::StatusCode::NOT_FOUND.into_response();
        }
        let mut answer = vec![1u8];
        answer.append(&mut waiting);
        answer.into_response()
    }
    let open = move |State(shared): State<Shared>| async move {
        let tcp = tokio::net::TcpStream::connect(("127.0.0.1", rtmp_port))
            .await
            .unwrap();
        let (mut read, write) = tcp.into_split();
        *shared.0.lock().await = Some(write);
        let from = Arc::clone(&shared);
        tokio::spawn(async move {
            let mut buffer = vec![0u8; 65536];
            while let Ok(n) = read.read(&mut buffer).await {
                if n == 0 {
                    break;
                }
                from.1.lock().unwrap().extend_from_slice(&buffer[..n]);
            }
            from.2.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        "dmsession\n"
    };
    let send = |State(shared): State<Shared>,
                Path((_, _)): Path<(String, String)>,
                body: axum::body::Bytes| async move {
        if let Some(write) = shared.0.lock().await.as_mut() {
            let _ = write.write_all(&body).await;
        }
        drained(&shared).await
    };
    let idle = |State(shared): State<Shared>, Path((_, _)): Path<(String, String)>| async move {
        drained(&shared).await
    };
    let app = axum::Router::new()
        .route("/open/1", axum::routing::post(open))
        .route("/send/{id}/{n}", axum::routing::post(send))
        .route("/idle/{id}/{n}", axum::routing::post(idle))
        .route("/close/{id}/{n}", axum::routing::post(|| async { "" }))
        .with_state(shared);
    axum::serve(listener, app).await.unwrap();
}

#[tokio::test]
async fn an_rtmpt_stream_is_fetched_by_dm_through_http() {
    let tags = flv_tags(&clip("clip.flv"));
    let rtmp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let rtmp_port = rtmp.local_addr().unwrap().port();
    let server = tokio::spawn(serve_rtmp(rtmp, tags.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_port = listener.local_addr().unwrap().port();
    let relay = tokio::spawn(relay_rtmpt(listener, rtmp_port));

    let dir = tempdir();
    let done = download(
        &client(),
        &format!("rtmpt://127.0.0.1:{http_port}/live/show"),
        &options(dir.path()),
    )
    .await;
    relay.abort();
    let done = done.expect("the RTMPT stream");
    assert_eq!(
        server.await.unwrap(),
        "show",
        "asked to play another stream"
    );
    assert_eq!(done.path.file_name().unwrap(), "show.mp4");
    assert_recorded_as_sent(&std::fs::read(&done.path).unwrap(), &tags);
}

#[tokio::test]
async fn an_encrypted_rtmp_stream_is_refused_plainly() {
    let dir = tempdir();
    // RTMPE (Flash's encrypted RTMP) is one dm does not speak.
    let outcome = download(
        &client(),
        "rtmpe://127.0.0.1:1/live/x",
        &options(dir.path()),
    )
    .await;
    let said = outcome.unwrap_err().to_string();
    assert!(said.contains("encrypted stream dm cannot record"), "{said}");
}

#[tokio::test]
async fn a_stream_is_not_sent_around_a_proxy() {
    let dir = tempdir();
    let behind = Options {
        proxy: Some("socks5://127.0.0.1:1".into()),
        ..options(dir.path())
    };
    let outcome = download(&client(), "mms://127.0.0.1:1/old.asf", &behind).await;
    let said = outcome.unwrap_err().to_string();
    assert!(said.contains("proxy"), "{said}");
}

/// The NAL units of an H.264 byte stream.
fn nal_units(data: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if data[i..i + 3] == [0, 0, 1] {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    starts
        .iter()
        .enumerate()
        .map(|(n, &s)| {
            let mut end = starts.get(n + 1).map_or(data.len(), |e| e - 3);
            while end > s && data[end - 1] == 0 {
                end -= 1;
            }
            &data[s..end]
        })
        .collect()
}

/// A one-client RTSP server for the test: DESCRIBE, SETUP (RTP interleaved
/// on the connection), PLAY, then each picture of `units` as RTP — a
/// small unit as it is, a large one in FU-A fragments — 25 a second; then
/// it hangs up.
async fn serve_rtsp(listener: tokio::net::TcpListener, units: Vec<Vec<u8>>) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (socket, _) = listener.accept().await.unwrap();
    let (read, mut write) = socket.into_split();
    let mut lines = BufReader::new(read).lines();
    let sps = units.iter().find(|u| u[0] & 0x1f == 7).unwrap().clone();
    let pps = units.iter().find(|u| u[0] & 0x1f == 8).unwrap().clone();
    use base64::Engine as _;
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    let sdp = format!(
        "v=0\r\no=- 0 0 IN IP4 127.0.0.1\r\ns=dm test\r\nt=0 0\r\nm=video 0 RTP/AVP 96\r\nc=IN IP4 0.0.0.0\r\na=rtpmap:96 H264/90000\r\na=fmtp:96 packetization-mode=1;sprop-parameter-sets={},{}\r\na=control:track1\r\n",
        b64(&sps),
        b64(&pps)
    );
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
        let method = first.split(' ').next().unwrap_or("").to_string();
        let reply = match method.as_str() {
            "OPTIONS" => format!("RTSP/1.0 200 OK\r\nCSeq: {cseq}\r\nPublic: DESCRIBE, SETUP, PLAY, TEARDOWN\r\n\r\n"),
            "DESCRIBE" => format!(
                "RTSP/1.0 200 OK\r\nCSeq: {cseq}\r\nContent-Base: {}/\r\nContent-Type: application/sdp\r\nContent-Length: {}\r\n\r\n{sdp}",
                first.split(' ').nth(1).unwrap_or(""),
                sdp.len()
            ),
            "SETUP" => format!("RTSP/1.0 200 OK\r\nCSeq: {cseq}\r\nSession: 12345678;timeout=60\r\nTransport: RTP/AVP/TCP;unicast;interleaved=0-1;ssrc=0000ABCD\r\n\r\n"),
            "PLAY" => format!("RTSP/1.0 200 OK\r\nCSeq: {cseq}\r\nSession: 12345678\r\nRTP-Info: url=track1;seq=1;rtptime=0\r\n\r\n"),
            _ => format!("RTSP/1.0 200 OK\r\nCSeq: {cseq}\r\nSession: 12345678\r\n\r\n"),
        };
        write.write_all(reply.as_bytes()).await.unwrap();
        if method == "PLAY" {
            break;
        }
    }
    // The pictures: units grouped at each access unit delimiter.
    let mut pictures: Vec<Vec<&Vec<u8>>> = Vec::new();
    for unit in &units {
        if unit[0] & 0x1f == 9 || pictures.is_empty() {
            pictures.push(Vec::new());
        }
        if unit[0] & 0x1f != 9 {
            pictures.last_mut().unwrap().push(unit);
        }
    }
    let mut seq: u16 = 1;
    for (n, picture) in pictures.iter().enumerate() {
        let time = (n as u32) * 3600;
        let mut packets: Vec<Vec<u8>> = Vec::new();
        for unit in picture {
            if unit.len() <= 1200 {
                packets.push(unit.to_vec());
            } else {
                let (head, body) = (unit[0], &unit[1..]);
                let parts: Vec<&[u8]> = body.chunks(1200).collect();
                for (i, part) in parts.iter().enumerate() {
                    let mut fu = vec![(head & 0xe0) | 28, head & 0x1f];
                    if i == 0 {
                        fu[1] |= 0x80;
                    }
                    if i == parts.len() - 1 {
                        fu[1] |= 0x40;
                    }
                    fu.extend_from_slice(part);
                    packets.push(fu);
                }
            }
        }
        let count = packets.len();
        for (i, payload) in packets.into_iter().enumerate() {
            let mut rtp = vec![0x80, 96 | if i == count - 1 { 0x80 } else { 0 }];
            rtp.extend_from_slice(&seq.to_be_bytes());
            rtp.extend_from_slice(&time.to_be_bytes());
            rtp.extend_from_slice(&0xABCDu32.to_be_bytes());
            rtp.extend_from_slice(&payload);
            seq = seq.wrapping_add(1);
            let mut frame = vec![b'$', 0];
            frame.extend_from_slice(&(rtp.len() as u16).to_be_bytes());
            frame.extend_from_slice(&rtp);
            if write.write_all(&frame).await.is_err() {
                return;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let _ = write.shutdown().await;
}

#[tokio::test]
async fn an_rtsp_stream_is_recorded_by_dm_picture_for_picture() {
    // Two seconds of H.264 at 25 pictures a second, each picture led by an
    // access unit delimiter, no B-frames.
    let data = clip("clip.h264");
    let units: Vec<Vec<u8>> = nal_units(&data).into_iter().map(<[u8]>::to_vec).collect();
    let kind = |u: &[u8]| u[0] & 0x1f;
    let sps = units.iter().find(|u| kind(u) == 7).unwrap().clone();
    let pps = units.iter().find(|u| kind(u) == 8).unwrap().clone();
    // The pictures as sent: each one's slices.
    let mut sent: Vec<Vec<Vec<u8>>> = Vec::new();
    for unit in &units {
        if kind(unit) == 9 {
            sent.push(Vec::new());
        } else if matches!(kind(unit), 1 | 5) {
            sent.last_mut().unwrap().push(unit.clone());
        }
    }
    assert_eq!(sent.len(), 50);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(serve_rtsp(listener, units.clone()));

    let dir = tempdir();
    let done = download(
        &client(),
        &format!("rtsp://127.0.0.1:{port}/cam/show"),
        &options(dir.path()),
    )
    .await;
    let done = done.expect("the RTSP stream");
    let _ = server.await;
    assert_eq!(done.path.file_name().unwrap(), "show.mp4");
    let got = tracks(&std::fs::read(&done.path).unwrap());
    assert_eq!(got.len(), 1, "one picture track");
    let picture = &got[0];
    assert_eq!(&picture.codec, b"avc1");
    assert_eq!((picture.width, picture.height), (320, 180));
    // avcC: its length size, then the SPS and PPS the camera described.
    let avcc = &picture.config;
    let length_size = usize::from(avcc[4] & 3) + 1;
    let sps_len = usize::from(u16::from_be_bytes([avcc[6], avcc[7]]));
    assert_eq!(avcc[8..8 + sps_len], sps[..], "not the camera's SPS");
    let pps_at = 8 + sps_len + 1;
    let pps_len = usize::from(u16::from_be_bytes([avcc[pps_at], avcc[pps_at + 1]]));
    assert_eq!(
        avcc[pps_at + 2..pps_at + 2 + pps_len],
        pps[..],
        "not the camera's PPS"
    );

    let recorded: Vec<Vec<Vec<u8>>> = picture
        .samples
        .iter()
        .map(|s| {
            length_prefixed_units(&s.data, length_size)
                .into_iter()
                .filter(|u| matches!(kind(u), 1 | 5))
                .map(<[u8]>::to_vec)
                .collect()
        })
        .collect();
    assert_eq!(recorded.len(), sent.len(), "pictures lost or added");
    for (n, (got, sent)) in recorded.iter().zip(&sent).enumerate() {
        assert!(got == sent, "picture {n} is not the one sent");
    }
    for (n, sample) in picture.samples.iter().enumerate() {
        assert_eq!(
            sample.sync,
            kind(&sent[n][0]) == 5,
            "picture {n}'s key flag"
        );
        if n + 1 < picture.samples.len() {
            // 3600 of RTP's 90 kHz apart: 40 ms.
            let gap =
                (picture.samples[n + 1].dts - sample.dts) as f64 / f64::from(picture.timescale);
            assert!((gap - 0.04).abs() < 0.002, "picture {n} lasts {gap} s");
        }
    }
}

/// An MMS stream asked for over HTTP, as a Windows Media server serves one:
/// the header alone, then on `xPlayStrm=1` the header and every packet,
/// each framed, padding trimmed. Saved, it is the file the server streamed.
#[tokio::test]
async fn an_mms_stream_is_fetched_over_http_by_dm_byte_for_byte() {
    let file = clip("clip.wmv");
    // Windows Media: an ASF header object, then the data object.
    assert_eq!(
        file[..16],
        [
            0x30, 0x26, 0xb2, 0x75, 0x8e, 0x66, 0xcf, 0x11, 0xa6, 0xd9, 0x00, 0xaa, 0x00, 0x62,
            0xce, 0x6c
        ]
    );
    let header_len = u64::from_le_bytes(file[16..24].try_into().unwrap()) as usize;
    let data_len =
        u64::from_le_bytes(file[header_len + 16..header_len + 24].try_into().unwrap()) as usize;
    let packet = 3200usize;
    // The header and the Data Object's own start; then its packets.
    let head = file[..header_len + 50].to_vec();
    let packets: Vec<Vec<u8>> = file[header_len + 50..header_len + data_len]
        .chunks(packet)
        .map(|p| {
            let mut p = p.to_vec();
            while p.last() == Some(&0) {
                p.pop();
            }
            p
        })
        .collect();
    let streamed = file[..header_len + data_len].to_vec();
    let frame = |kind: u8, body: &[u8]| {
        let mut out = vec![b'$', kind];
        let extra = if kind == b'E' { 0 } else { 8 };
        out.extend_from_slice(&((body.len() + extra) as u16).to_le_bytes());
        if extra > 0 {
            out.extend_from_slice(&[0, 0, 0, 0, 0, 0x0c]);
            out.extend_from_slice(&((body.len() + 8) as u16).to_le_bytes());
        }
        out.extend_from_slice(body);
        out
    };
    let app = axum::Router::new().route(
        "/old/clip.asf",
        axum::routing::get(move |headers: axum::http::HeaderMap| {
            let (head, packets) = (head.clone(), packets.clone());
            async move {
                let pragmas: Vec<String> = headers
                    .get_all("pragma")
                    .iter()
                    .filter_map(|v| v.to_str().ok().map(str::to_string))
                    .collect();
                let mut body = frame(b'H', &head);
                if pragmas.iter().any(|p| p.contains("xPlayStrm=1")) {
                    assert!(
                        pragmas
                            .iter()
                            .any(|p| p.contains("stream-switch-entry=ffff:1:0 ffff:2:0")),
                        "both streams were not asked for: {pragmas:?}"
                    );
                    for p in &packets {
                        body.extend(frame(b'D', p));
                    }
                    body.extend(frame(b'E', &[]));
                }
                axum::response::Response::builder()
                    .header("content-type", "application/vnd.ms.wms-hdr.asfv1")
                    .body(axum::body::Body::from(body))
                    .unwrap()
            }
        }),
    );
    let addr = common::serve(app).await;
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("mms://{addr}/old/clip.asf"),
        &options(dir.path()),
    )
    .await;
    let done = done.expect("the MMS stream");
    assert_eq!(done.path.file_name().unwrap(), "clip.wmv");
    assert!(
        std::fs::read(&done.path).unwrap() == streamed,
        "not the stream the server sent"
    );
}

#[tokio::test]
async fn a_recording_limit_ends_an_endless_stream_and_keeps_what_was_recorded() {
    let file = clip("clip.wmv");
    let header_len = u64::from_le_bytes(file[16..24].try_into().unwrap()) as usize;
    let data_len =
        u64::from_le_bytes(file[header_len + 16..header_len + 24].try_into().unwrap()) as usize;
    let head = file[..header_len + 50].to_vec();
    let packets: Vec<Vec<u8>> = file[header_len + 50..header_len + data_len]
        .chunks(3200)
        .map(|p| {
            let mut p = p.to_vec();
            while p.last() == Some(&0) {
                p.pop();
            }
            p
        })
        .collect();
    let streamed = file[..header_len + data_len].to_vec();
    let frame = |kind: u8, body: &[u8]| {
        let mut out = vec![b'$', kind];
        let extra = if kind == b'E' { 0 } else { 8 };
        out.extend_from_slice(&((body.len() + extra) as u16).to_le_bytes());
        if extra > 0 {
            out.extend_from_slice(&[0, 0, 0, 0, 0, 0x0c]);
            out.extend_from_slice(&((body.len() + 8) as u16).to_le_bytes());
        }
        out.extend_from_slice(body);
        out
    };
    let app = axum::Router::new().route(
        "/live/endless.asf",
        axum::routing::get(move |headers: axum::http::HeaderMap| {
            let (head, packets) = (head.clone(), packets.clone());
            async move {
                let playing = headers
                    .get_all("pragma")
                    .iter()
                    .any(|p| p.to_str().is_ok_and(|p| p.contains("xPlayStrm=1")));
                let mut first = frame(b'H', &head);
                if playing {
                    for p in &packets {
                        first.extend(frame(b'D', p));
                    }
                }
                // What there is, then no more and no end: a live stream.
                let once = futures_util::stream::once(async move {
                    Ok::<_, std::io::Error>(bytes::Bytes::from(first))
                });
                // The description request ends; the playing one does not.
                let body = if playing {
                    once.chain(futures_util::stream::pending()).boxed()
                } else {
                    once.boxed()
                };
                axum::response::Response::builder()
                    .header("content-type", "application/vnd.ms.wms-hdr.asfv1")
                    .body(axum::body::Body::from_stream(body))
                    .unwrap()
            }
        }),
    );
    let addr = common::serve(app).await;
    let dir = tempdir();
    let mut opts = options(dir.path());
    opts.record_limit = Some(std::time::Duration::from_secs(3));
    let started = std::time::Instant::now();
    let done = download(&client(), &format!("mms://{addr}/live/endless.asf"), &opts)
        .await
        .expect("the stream, ended by its limit");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(30),
        "it did not end"
    );
    assert!(
        started.elapsed() >= std::time::Duration::from_secs(3),
        "it ended early"
    );
    assert_eq!(std::fs::read(&done.path).unwrap(), streamed);
}
