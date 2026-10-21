//! Streams from a media server: an RTMP address fetched by dm's own client
//! and placed as a file, and the refusals where one cannot be — a scheme
//! left to ffmpeg with no ffmpeg, or a proxy it would go around. The server
//! is ffmpeg itself, which can serve one client over RTMP.

mod common;

use dm_core::{client, download, Options};

use common::{options, tempdir};

/// The tests that set `DM_FFMPEG` take turns: it is the process's.
static FFMPEG_ENV: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The ffmpeg dm would find, if any.
fn ffmpeg() -> Option<std::path::PathBuf> {
    let found = [
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/usr/bin/ffmpeg",
    ]
    .iter()
    .map(std::path::PathBuf::from)
    .find(|p| p.is_file());
    assert!(
        found.is_some() || !(cfg!(target_os = "linux") && std::env::var_os("CI").is_some()),
        "no ffmpeg in Linux CI: the stream tests would pass without running"
    );
    found
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test]
async fn an_rtmp_stream_is_fetched_by_dm_and_saved_as_a_file_that_decodes_the_same() {
    let _turn = FFMPEG_ENV.lock().await;
    let Some(real) = ffmpeg() else {
        eprintln!("no ffmpeg installed — skipping");
        return;
    };
    let site = tempdir();
    let clip = site.path().join("clip.flv");
    let made = std::process::Command::new(&real)
        .args(["-y", "-loglevel", "error", "-f", "lavfi", "-i"])
        .arg("testsrc=duration=3:size=160x120:rate=10")
        .args(["-f", "lavfi", "-i", "sine=duration=3"])
        .args([
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(&clip)
        .status();
    if !made.is_ok_and(|s| s.success()) {
        eprintln!("this ffmpeg cannot make an H.264 clip — skipping");
        return;
    }
    let port = free_port();
    let address = format!("rtmp://127.0.0.1:{port}/live/show");
    // The server: ffmpeg playing the clip to the one client that comes.
    let mut server = tokio::process::Command::new(&real)
        .args(["-loglevel", "error", "-re", "-i"])
        .arg(&clip)
        .args(["-c", "copy", "-f", "flv", "-listen", "1"])
        .arg(&address)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    // Given a moment to listen: knocking on its port would be the one
    // client it serves.
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    // dm is told there is no ffmpeg: the stream is its own client's.
    std::env::set_var("DM_FFMPEG", "");
    let dir = tempdir();
    let done = download(&client(), &address, &options(dir.path())).await;
    std::env::remove_var("DM_FFMPEG");
    let done = done.expect("the RTMP stream");
    let _ = server.kill().await;
    assert_eq!(done.path.file_name().unwrap(), "show.mp4");
    assert!(done.bytes > 10_000, "{} bytes", done.bytes);
    // A file that plays: ffprobe reads a duration near the clip's.
    let probed = std::process::Command::new(real.with_file_name("ffprobe"))
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(&done.path)
        .output();
    if let Ok(out) = probed {
        let seconds: f64 = String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse()
            .unwrap_or(0.0);
        assert!(seconds > 2.5, "{seconds} s of stream saved");
    }
    // Every picture and sound the server sent, as they were.
    let md5 = |file: &std::path::Path, stream: &str| {
        let out = std::process::Command::new(&real)
            .args(["-v", "error", "-i"])
            .arg(file)
            .args(["-map", stream, "-fps_mode", "passthrough", "-f", "md5", "-"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    assert_eq!(
        md5(&done.path, "0:v"),
        md5(&clip, "0:v"),
        "the pictures differ"
    );
    assert_eq!(
        md5(&done.path, "0:a"),
        md5(&clip, "0:a"),
        "the sound differs"
    );
    let kinds = std::fs::read(&done.path).unwrap();
    assert_eq!(kinds.get(4..8), Some(&b"ftyp"[..]));
    assert!(
        !kinds.windows(4).any(|w| w == b"moof"),
        "left fragmented, not rewritten plain"
    );
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
        .route("/send/:id/:n", axum::routing::post(send))
        .route("/idle/:id/:n", axum::routing::post(idle))
        .route("/close/:id/:n", axum::routing::post(|| async { "" }))
        .with_state(shared);
    axum::serve(listener, app).await.unwrap();
}

#[tokio::test]
async fn an_rtmpt_stream_is_fetched_by_dm_through_http() {
    let _turn = FFMPEG_ENV.lock().await;
    let Some(real) = ffmpeg() else {
        eprintln!("no ffmpeg installed — skipping");
        return;
    };
    let site = tempdir();
    let clip = site.path().join("clip.flv");
    let made = std::process::Command::new(&real)
        .args(["-y", "-loglevel", "error", "-f", "lavfi", "-i"])
        .arg("testsrc=duration=3:size=160x120:rate=10")
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .arg(&clip)
        .status();
    if !made.is_ok_and(|s| s.success()) {
        eprintln!("this ffmpeg cannot make an H.264 clip — skipping");
        return;
    }
    let rtmp_port = free_port();
    let mut server = tokio::process::Command::new(&real)
        .args(["-loglevel", "error", "-re", "-i"])
        .arg(&clip)
        .args(["-c", "copy", "-f", "flv", "-listen", "1"])
        .arg(format!("rtmp://127.0.0.1:{rtmp_port}/live/show"))
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_port = listener.local_addr().unwrap().port();
    let relay = tokio::spawn(relay_rtmpt(listener, rtmp_port));

    std::env::set_var("DM_FFMPEG", "");
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("rtmpt://127.0.0.1:{http_port}/live/show"),
        &options(dir.path()),
    )
    .await;
    std::env::remove_var("DM_FFMPEG");
    relay.abort();
    let _ = server.kill().await;
    let done = done.expect("the RTMPT stream");
    assert_eq!(done.path.file_name().unwrap(), "show.mp4");
    let md5 = |file: &std::path::Path| {
        let out = std::process::Command::new(&real)
            .args(["-v", "error", "-i"])
            .arg(file)
            .args(["-map", "0:v", "-fps_mode", "passthrough", "-f", "md5", "-"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    assert_eq!(md5(&done.path), md5(&clip), "the pictures differ");
}

#[tokio::test]
async fn without_ffmpeg_a_stream_is_refused_plainly() {
    let _turn = FFMPEG_ENV.lock().await;
    std::env::set_var("DM_FFMPEG", "");
    let dir = tempdir();
    // RTMPE (Flash's encrypted RTMP) is the one dm leaves to an ffmpeg.
    let outcome = download(
        &client(),
        "rtmpe://127.0.0.1:1/live/x",
        &options(dir.path()),
    )
    .await;
    std::env::remove_var("DM_FFMPEG");
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
    let _turn = FFMPEG_ENV.lock().await;
    let Some(real) = ffmpeg() else {
        eprintln!("no ffmpeg installed — skipping");
        return;
    };
    let site = tempdir();
    let stream = site.path().join("clip.h264");
    let made = std::process::Command::new(&real)
        .args(["-y", "-loglevel", "error", "-f", "lavfi", "-i"])
        .arg("testsrc2=duration=3:size=320x180:rate=25")
        .args([
            "-c:v", "libx264", "-bf", "0", "-g", "25", "-pix_fmt", "yuv420p",
        ])
        .args(["-x264-params", "aud=1", "-f", "h264"])
        .arg(&stream)
        .status();
    if !made.is_ok_and(|s| s.success()) {
        eprintln!("this ffmpeg cannot make an H.264 stream — skipping");
        return;
    }
    let data = std::fs::read(&stream).unwrap();
    let units: Vec<Vec<u8>> = nal_units(&data).into_iter().map(<[u8]>::to_vec).collect();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(serve_rtsp(listener, units));

    std::env::set_var("DM_FFMPEG", "");
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("rtsp://127.0.0.1:{port}/cam/show"),
        &options(dir.path()),
    )
    .await;
    std::env::remove_var("DM_FFMPEG");
    let done = done.expect("the RTSP stream");
    let _ = server.await;
    assert_eq!(done.path.file_name().unwrap(), "show.mp4");
    let hashes = |file: &std::path::Path| -> Vec<String> {
        let out = std::process::Command::new(&real)
            .args(["-v", "error", "-i"])
            .arg(file)
            .args(["-map", "0:v", "-f", "framemd5", "-"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.rsplit(',').next().map(|h| h.trim().to_string()))
            .collect()
    };
    let sent = hashes(&stream);
    let got = hashes(&done.path);
    assert_eq!(sent.len(), 75);
    assert_eq!(got, sent, "the recorded pictures are not the ones sent");
}

/// An MMS stream asked for over HTTP, as a Windows Media server serves one:
/// the header alone, then on `xPlayStrm=1` the header and every packet,
/// each framed, padding trimmed. Saved, it is the file the server streamed.
#[tokio::test]
async fn an_mms_stream_is_fetched_over_http_by_dm_byte_for_byte() {
    let _turn = FFMPEG_ENV.lock().await;
    let Some(real) = ffmpeg() else {
        eprintln!("no ffmpeg installed — skipping");
        return;
    };
    let site = tempdir();
    let clip = site.path().join("clip.wmv");
    let made = std::process::Command::new(&real)
        .args(["-y", "-loglevel", "error", "-f", "lavfi", "-i"])
        .arg("testsrc=duration=3:size=160x120:rate=10")
        .args(["-f", "lavfi", "-i", "sine=duration=3"])
        .args(["-c:v", "wmv2", "-c:a", "wmav2", "-f", "asf"])
        .arg(&clip)
        .status();
    if !made.is_ok_and(|s| s.success()) {
        eprintln!("this ffmpeg cannot make Windows Media — skipping");
        return;
    }
    let file = std::fs::read(&clip).unwrap();
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
    std::env::set_var("DM_FFMPEG", "");
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("mms://{addr}/old/clip.asf"),
        &options(dir.path()),
    )
    .await;
    std::env::remove_var("DM_FFMPEG");
    let done = done.expect("the MMS stream");
    assert_eq!(done.path.file_name().unwrap(), "clip.wmv");
    assert!(
        std::fs::read(&done.path).unwrap() == streamed,
        "not the stream the server sent"
    );
    let decoded = std::process::Command::new(&real)
        .args(["-v", "error", "-i"])
        .arg(&done.path)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        decoded.status.success() && decoded.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
}
