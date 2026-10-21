//! Fetching an adaptive stream, from the playlist down to the saved file.
//!
//! The playlists here are synthetic, but nothing else is: a real server, a
//! real master playlist pointing at a real media playlist, real segments, and
//! real AES-128 with the key served from its own URL. The parser has unit
//! tests; this is the part where the segments have to come back in order and
//! decrypt.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::Response;
use cbc::cipher::{block_padding::Pkcs7, BlockModeEncrypt, InnerIvInit, KeyInit};
use dm_core::{client, download, Options};

mod common;
use common::tempdir;

const KEY: [u8; 16] = [
    0x1a, 0x2b, 0x3c, 0x4d, 0x5e, 0x6f, 0x70, 0x81, 0x92, 0xa3, 0xb4, 0xc5, 0xd6, 0xe7, 0xf8, 0x09,
];
const SEGMENTS: usize = 5;

/// What one segment carries, made distinct so an out-of-order join shows up.
fn segment(index: usize) -> Vec<u8> {
    let mut out = vec![0u8; 64 * 1024];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = ((index * 31 + i) % 251) as u8;
    }
    out
}

fn whole() -> Vec<u8> {
    (0..SEGMENTS).flat_map(segment).collect()
}

fn sealed(index: usize, sequence: u64) -> Vec<u8> {
    let mut iv = [0u8; 16];
    iv[8..].copy_from_slice(&sequence.to_be_bytes());
    let cipher = aes::Aes128::new_from_slice(&KEY).unwrap();
    cbc::Encryptor::<aes::Aes128>::inner_iv_slice_init(cipher, &iv)
        .unwrap()
        .encrypt_padded_vec::<Pkcs7>(&segment(index))
}

#[derive(Clone)]
struct Stream {
    encrypted: bool,
    /// The page every request must name, where the server is the kind that
    /// serves a stream only to its own player.
    page: Option<&'static str>,
    /// Every path asked for, in order, so the test can say what was fetched.
    asked: Arc<std::sync::Mutex<Vec<String>>>,
}

async fn serve(encrypted: bool) -> (SocketAddr, Arc<std::sync::Mutex<Vec<String>>>) {
    serve_for(encrypted, None).await
}

async fn serve_for(
    encrypted: bool,
    page: Option<&'static str>,
) -> (SocketAddr, Arc<std::sync::Mutex<Vec<String>>>) {
    let asked = Arc::new(std::sync::Mutex::new(Vec::new()));
    let state = Stream {
        encrypted,
        page,
        asked: Arc::clone(&asked),
    };

    async fn handler(State(s): State<Stream>, uri: Uri, headers: HeaderMap) -> Response {
        let path = uri.path().to_string();
        s.asked.lock().unwrap().push(path.clone());

        // Hotlink protection, on every request: a CDN that checks the page
        // on the manifest checks it on the key and the segments too.
        if let Some(page) = s.page {
            let named = headers.get("referer").and_then(|v| v.to_str().ok());
            if named != Some(page) {
                return Response::builder()
                    .status(StatusCode::FORBIDDEN)
                    .body(Body::from("not from the page"))
                    .unwrap();
            }
        }

        let text = |body: String, kind: &str| {
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", kind)
                .body(Body::from(body))
                .unwrap()
        };

        // A master playlist, so the rendition choice is exercised too.
        if path.ends_with("/master.m3u8") {
            return text(
                "#EXTM3U\n\
                 #EXT-X-STREAM-INF:BANDWIDTH=400000,RESOLUTION=640x360\n\
                 low/index.m3u8\n\
                 #EXT-X-STREAM-INF:BANDWIDTH=4000000,RESOLUTION=1920x1080\n\
                 high/index.m3u8\n"
                    .into(),
                "application/vnd.apple.mpegurl",
            );
        }

        if path.ends_with("/index.m3u8") {
            let key = if s.encrypted {
                "#EXT-X-KEY:METHOD=AES-128,URI=\"../key.bin\"\n".to_string()
            } else {
                String::new()
            };
            let body: String = (0..SEGMENTS)
                .map(|i| format!("#EXTINF:4.0,\nseg{i}.ts\n"))
                .collect();
            return text(
                format!("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:0\n{key}{body}#EXT-X-ENDLIST\n"),
                "application/vnd.apple.mpegurl",
            );
        }

        if path.ends_with("/key.bin") {
            return Response::builder()
                .status(StatusCode::OK)
                .body(Body::from(KEY.to_vec()))
                .unwrap();
        }

        if let Some(index) = path
            .rsplit('/')
            .next()
            .and_then(|f| f.strip_prefix("seg"))
            .and_then(|f| f.strip_suffix(".ts"))
            .and_then(|n| n.parse::<usize>().ok())
        {
            let body = if s.encrypted {
                sealed(index, index as u64)
            } else {
                segment(index)
            };
            return Response::builder()
                .status(StatusCode::OK)
                .header("content-length", body.len())
                .body(Body::from(body))
                .unwrap();
        }

        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap()
    }

    let addr = common::serve(axum::Router::new().fallback(handler).with_state(state)).await;
    (addr, asked)
}

fn options(dir: &std::path::Path, quality: dm_core::stream::Quality) -> Options {
    Options {
        quality,
        ..common::options(dir)
    }
}

#[tokio::test]
async fn a_playlist_is_saved_as_one_file_in_the_right_order() {
    let (addr, asked) = serve(false).await;
    let dir = tempdir();

    let done = download(
        &client(),
        &format!("http://{addr}/v/master.m3u8"),
        &options(dir.path(), dm_core::stream::Quality::Best),
    )
    .await
    .expect("the stream failed");

    // Named after the directory, because the playlist is called master.m3u8
    // and that tells nobody anything.
    assert_eq!(done.path.file_name().unwrap(), "v.ts");
    assert_eq!(done.bytes, whole().len() as u64);
    assert_eq!(
        std::fs::read(&done.path).unwrap(),
        whole(),
        "the segments were joined in the wrong order or a byte was lost"
    );

    // The tallest rendition, and no part files left behind.
    let asked = asked.lock().unwrap().clone();
    assert!(asked.iter().any(|p| p.contains("/high/")), "{asked:?}");
    assert!(!asked.iter().any(|p| p.contains("/low/")), "{asked:?}");
    assert_eq!(
        std::fs::read_dir(dir.path().join("parts")).unwrap().count(),
        0,
        "part files outlived the download"
    );
}

#[tokio::test]
async fn a_cap_takes_the_rendition_under_it() {
    let (addr, asked) = serve(false).await;
    let dir = tempdir();

    download(
        &client(),
        &format!("http://{addr}/v/master.m3u8"),
        &options(dir.path(), dm_core::stream::Quality::UpTo(720)),
    )
    .await
    .expect("the stream failed");

    let asked = asked.lock().unwrap().clone();
    assert!(asked.iter().any(|p| p.contains("/low/")), "{asked:?}");
    assert!(!asked.iter().any(|p| p.contains("/high/")), "{asked:?}");
}

#[tokio::test]
async fn an_encrypted_playlist_is_decrypted_segment_by_segment() {
    /* Each segment has its own initialisation vector, taken from its media
     * sequence number, so decrypting them all with the first one's produces a
     * file that is the right length and rubbish after the first segment. */
    let (addr, asked) = serve(true).await;
    let dir = tempdir();

    let done = download(
        &client(),
        &format!("http://{addr}/v/master.m3u8"),
        &options(dir.path(), dm_core::stream::Quality::Best),
    )
    .await
    .expect("the encrypted stream failed");

    assert_eq!(
        std::fs::read(&done.path).unwrap(),
        whole(),
        "the stream did not decrypt to what was encrypted"
    );

    // Once, not once per segment.
    let keys = asked
        .lock()
        .unwrap()
        .iter()
        .filter(|p| p.ends_with("key.bin"))
        .count();
    assert_eq!(keys, 1, "the key was fetched {keys} times");
}

#[tokio::test]
async fn a_stream_served_only_to_its_page_downloads_with_the_referrer() {
    /* The probe carried the referrer and nothing after it did: the playlist
     * re-fetch, the key and every segment went out bare, so a stream the
     * browser had just played failed in the app one request after the
     * probe passed. */
    let page = "https://player.example/watch";
    let (addr, asked) = serve_for(true, Some(page)).await;
    let dir = tempdir();
    let options = Options {
        referrer: Some(page.to_string()),
        ..options(dir.path(), dm_core::stream::Quality::Best)
    };
    let url = format!("http://{addr}/v/master.m3u8");

    let done = download(&client(), &url, &options)
        .await
        .expect("the referrer was dropped somewhere after the probe");
    assert_eq!(std::fs::read(&done.path).unwrap(), whole());
    // Manifest, rendition, key and segments: all of them went through.
    let asked = asked.lock().unwrap().clone();
    assert!(asked.iter().any(|p| p.ends_with("key.bin")), "{asked:?}");
}

#[tokio::test]
async fn a_rendition_change_does_not_splice_two_streams_together() {
    // Segments of the 1080p rendition are staged; the quality setting now
    // says 480p. Joining the two is a file that does not play, reported
    // Completed. The staged ones belong to another rendition and go.
    let (addr, asked) = serve(false).await;
    let dir = tempdir();
    let options = options(dir.path(), dm_core::stream::Quality::UpTo(480));
    let url = format!("http://{addr}/v/master.m3u8");

    std::fs::create_dir_all(&options.parts_dir).unwrap();
    std::fs::write(
        options.parts_dir.join("rendition"),
        format!("http://{addr}/v/high/index.m3u8"),
    )
    .unwrap();
    // Not the bytes the low rendition serves, so adopting them would show.
    std::fs::write(options.parts_dir.join("part-0000"), b"1080p bytes").unwrap();
    asked.lock().unwrap().clear();

    let done = download(&client(), &url, &options)
        .await
        .expect("download failed");
    assert_eq!(std::fs::read(&done.path).unwrap(), whole());
    let asked = asked.lock().unwrap().clone();
    assert!(
        asked.iter().any(|p| p.ends_with("low/seg0.ts")),
        "the other rendition's segment was kept: {asked:?}"
    );
}

#[tokio::test]
async fn a_stream_resumes_from_the_segments_it_already_has() {
    /* A segment has no declared length, so a part file that exists says
     * nothing about whether it finished — which is why only completed
     * segments are given their real name. */
    let (addr, asked) = serve(false).await;
    let dir = tempdir();
    let options = options(dir.path(), dm_core::stream::Quality::Best);
    let url = format!("http://{addr}/v/master.m3u8");

    // Stand in for a run that stopped after two segments, plus a half-written
    // third under its temporary name — and the rendition it wrote beside them.
    std::fs::create_dir_all(&options.parts_dir).unwrap();
    std::fs::write(
        options.parts_dir.join("rendition"),
        format!("http://{addr}/v/high/index.m3u8"),
    )
    .unwrap();
    for i in 0..2 {
        std::fs::write(options.parts_dir.join(format!("part-{i:04}")), segment(i)).unwrap();
    }
    std::fs::write(options.parts_dir.join("part-0002.partial"), b"half").unwrap();
    asked.lock().unwrap().clear();

    let done = download(&client(), &url, &options)
        .await
        .expect("resume failed");
    assert_eq!(std::fs::read(&done.path).unwrap(), whole());

    let asked = asked.lock().unwrap().clone();
    for already in ["seg0.ts", "seg1.ts"] {
        assert!(
            !asked.iter().any(|p| p.ends_with(already)),
            "{already} was fetched again: {asked:?}"
        );
    }
    assert!(
        asked.iter().any(|p| p.ends_with("seg2.ts")),
        "the half-written segment was not fetched again: {asked:?}"
    );
}

/// A real transport stream, made by the ffmpeg on this machine, served from
/// a folder: what a site's HLS looks like, down to the bytes.
async fn serve_folder(dir: std::path::PathBuf) -> SocketAddr {
    async fn handler(State(dir): State<Arc<std::path::PathBuf>>, uri: Uri) -> Response {
        let name = uri.path().trim_start_matches('/').to_string();
        match std::fs::read(dir.join(&name)) {
            Ok(bytes) => {
                let kind = if name.ends_with(".m3u8") {
                    "application/vnd.apple.mpegurl"
                } else {
                    "video/mp2t"
                };
                Response::builder()
                    .header("content-type", kind)
                    .body(Body::from(bytes))
                    .unwrap()
            }
            Err(_) => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .unwrap(),
        }
    }
    common::serve(
        axum::Router::new()
            .fallback(handler)
            .with_state(Arc::new(dir)),
    )
    .await
}

/// The ffmpeg dm would find, if any: the same homes the engine searches.
fn ffmpeg() -> Option<std::path::PathBuf> {
    if let Some(named) = std::env::var_os("DM_FFMPEG") {
        return (!named.is_empty()).then(|| named.into());
    }
    let found = [
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/usr/bin/ffmpeg",
    ]
    .iter()
    .map(std::path::PathBuf::from)
    .find(|p| p.is_file());
    // CI installs it on Linux: there, a missing ffmpeg is a broken job, not
    // a reason for the remux tests to pass without running.
    assert!(
        found.is_some() || !(cfg!(target_os = "linux") && std::env::var_os("CI").is_some()),
        "no ffmpeg in Linux CI: the remux tests would pass without running"
    );
    found
}

/// The remux tests set `DM_FFMPEG`, which is process-wide: they take turns.
static FFMPEG_ENV: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A real HLS stream of `seconds`, made by ffmpeg into `site`: pictures by
/// `picture` (an ffmpeg encoder: `libx264`, which dm puts into an MP4
/// itself, or `mpeg2video`, which it leaves to an ffmpeg) and AAC sound.
/// False where this ffmpeg cannot make one.
#[cfg(unix)]
fn make_stream(
    ffmpeg: &std::path::Path,
    site: &std::path::Path,
    seconds: u32,
    picture: &str,
) -> bool {
    std::process::Command::new(ffmpeg)
        .args([
            "-y",
            "-nostdin",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc=duration={seconds}:size=128x72:rate=10"),
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency=440:duration={seconds}"),
            "-c:v",
            picture,
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-f",
            "hls",
            "-hls_time",
            "1",
            "-hls_list_size",
            "0",
            "-hls_segment_filename",
        ])
        .arg(site.join("seg%d.ts"))
        .arg(site.join("show.m3u8"))
        .status()
        .is_ok_and(|s| s.success())
}

// Its stand-in tools are a shell script and /usr/bin/false.
#[cfg(unix)]
#[tokio::test]
async fn a_remux_that_fails_keeps_the_stream_whole_as_ts() {
    let _turn = FFMPEG_ENV.lock().await;
    let Some(real) = ffmpeg() else {
        eprintln!("no ffmpeg installed — skipping");
        return;
    };
    let site = tempdir();
    if !make_stream(&real, site.path(), 3, "mpeg2video") {
        return;
    }
    let addr = serve_folder(site.path().to_path_buf()).await;
    // An "ffmpeg" that fails every time: the segments are already gathered
    // into one stream by then, and that stream is what is saved.
    std::env::set_var("DM_FFMPEG", "/usr/bin/false");
    let dir = tempdir();
    let asked = Options {
        room: Default::default(),
        remux: true,
        ..options(dir.path(), dm_core::stream::Quality::Best)
    };
    let done = download(&client(), &format!("http://{addr}/show.m3u8"), &asked).await;
    std::env::remove_var("DM_FFMPEG");
    let done = done.expect("a failed remux failed the download");
    assert_eq!(
        done.path.file_name().and_then(|n| n.to_str()),
        Some("show.ts")
    );
    let playlist = std::fs::read_to_string(site.path().join("show.m3u8")).unwrap();
    let served: Vec<u8> = playlist
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .flat_map(|name| std::fs::read(site.path().join(name.trim())).unwrap())
        .collect();
    assert_eq!(
        std::fs::read(&done.path).unwrap(),
        served,
        "not the stream, whole and in order"
    );
    let left = std::fs::read_dir(dir.path().join("parts"))
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(left, 0, "segments or a half-made MP4 were left behind");
}

#[tokio::test]
async fn a_transport_stream_is_saved_as_an_mp4_where_ffmpeg_can_and_as_ts_where_not_asked() {
    let _turn = FFMPEG_ENV.lock().await;
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("no ffmpeg installed — skipping the remux test");
        return;
    };
    let site = tempdir();
    // Three seconds of a test pattern with a tone, as one-second segments.
    let made = std::process::Command::new(&ffmpeg)
        .args([
            "-y",
            "-nostdin",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=3:size=128x72:rate=10",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=3",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-f",
            "hls",
            "-hls_time",
            "1",
            "-hls_list_size",
            "0",
            "-hls_segment_filename",
        ])
        .arg(site.path().join("seg%d.ts"))
        .arg(site.path().join("show.m3u8"))
        .status();
    if !made.is_ok_and(|s| s.success()) {
        eprintln!("this ffmpeg could not make an HLS stream — skipping the remux test");
        return;
    }
    let addr = serve_folder(site.path().to_path_buf()).await;
    let url = format!("http://{addr}/show.m3u8");

    let dir = tempdir();
    let as_mp4 = Options {
        room: Default::default(),
        remux: true,
        ..options(dir.path(), dm_core::stream::Quality::Best)
    };
    let done = download(&client(), &url, &as_mp4)
        .await
        .expect("the stream");
    assert_eq!(
        done.path.extension().and_then(|e| e.to_str()),
        Some("mp4"),
        "{:?}",
        done.path
    );
    assert_eq!(
        done.path.file_name().and_then(|n| n.to_str()),
        Some("show.mp4")
    );
    // Whole and playable: ffmpeg decodes it to the end without a word.
    let decoded = std::process::Command::new(&ffmpeg)
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(&done.path)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        decoded.status.success(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
    let parts_left = std::fs::read_dir(dir.path().join("parts"))
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(parts_left, 0, "parts or the joined .ts were left behind");

    // Not asked: the transport stream, as it came.
    let dir = tempdir();
    let as_ts = options(dir.path(), dm_core::stream::Quality::Best);
    let kept = download(&client(), &url, &as_ts).await.expect("the stream");
    assert_eq!(
        kept.path.file_name().and_then(|n| n.to_str()),
        Some("show.ts")
    );
    // The segments the playlist names, in its order: how many ffmpeg cut
    // is its business (a keyframe every second is a wish, not a promise).
    let playlist = std::fs::read_to_string(site.path().join("show.m3u8")).unwrap();
    let served: Vec<u8> = playlist
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .flat_map(|name| std::fs::read(site.path().join(name.trim())).unwrap())
        .collect();
    assert_eq!(
        std::fs::read(&kept.path).unwrap(),
        served,
        "not the segments, in order"
    );
}

// Its stand-in tools are a shell script and /usr/bin/false.
#[cfg(unix)]
#[tokio::test]
async fn a_pause_during_the_remux_keeps_the_stream_and_a_resume_fetches_nothing_again() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let _turn = FFMPEG_ENV.lock().await;
    let Some(real) = ffmpeg() else {
        eprintln!("no ffmpeg installed — skipping");
        return;
    };
    let site = tempdir();
    if !make_stream(&real, site.path(), 3, "mpeg2video") {
        return;
    }
    // Segments served and counted.
    let asked = Arc::new(AtomicUsize::new(0));
    let (dir_served, counted) = (Arc::new(site.path().to_path_buf()), Arc::clone(&asked));
    let app = axum::Router::new().fallback(move |uri: Uri| {
        let (dir, counted) = (Arc::clone(&dir_served), Arc::clone(&counted));
        async move {
            let name = uri.path().trim_start_matches('/').to_string();
            if name.ends_with(".ts") {
                counted.fetch_add(1, Ordering::SeqCst);
            }
            match std::fs::read(dir.join(&name)) {
                Ok(bytes) => Response::new(Body::from(bytes)),
                Err(_) => Response::builder().status(404).body(Body::empty()).unwrap(),
            }
        }
    });
    let addr = common::serve(app).await;
    let url = format!("http://{addr}/show.m3u8");

    // An ffmpeg slow enough to pause under.
    let tools = tempdir();
    let slow = tools.path().join("ffmpeg");
    std::fs::write(
        &slow,
        format!("#!/bin/sh\nsleep 2\nexec '{}' \"$@\"\n", real.display()),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::env::set_var("DM_FFMPEG", &slow);

    let dir = tempdir();
    let first = Options {
        room: Default::default(),
        remux: true,
        ..options(dir.path(), dm_core::stream::Quality::Best)
    };
    let control = first.control.clone();
    let running = {
        let (url, first) = (url.clone(), first.clone());
        tokio::spawn(async move { download(&client(), &url, &first).await })
    };
    // Every segment in, gathered, and the remux under way: then paused.
    let gathered = dir.path().join("parts").join("joined.ts");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !gathered.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "the stream was never gathered"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let asked_at = std::time::Instant::now();
    control.pause();
    let paused = running.await.unwrap();
    assert!(matches!(paused, Err(dm_core::Error::Paused)), "{paused:?}");
    // Stopped now, not when the slow ffmpeg would have finished.
    assert!(
        asked_at.elapsed() < std::time::Duration::from_millis(1500),
        "the pause waited for ffmpeg: {:?}",
        asked_at.elapsed()
    );
    let fetched = asked.load(Ordering::SeqCst);

    // Resumed: placed from what was kept, nothing asked for again.
    let again = Options {
        control: dm_core::Control::new(),
        ..first
    };
    let done = download(&client(), &url, &again).await;
    std::env::remove_var("DM_FFMPEG");
    let done = done.expect("the resume");
    assert_eq!(
        asked.load(Ordering::SeqCst),
        fetched,
        "segments were fetched again"
    );
    assert_eq!(
        done.path.file_name().and_then(|n| n.to_str()),
        Some("show.mp4")
    );
    let decoded = std::process::Command::new(&real)
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(&done.path)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        decoded.status.success(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
}

/// H.264 and AAC, as nearly every HLS stream is, put into an MP4 by dm with
/// no ffmpeg on the computer at all. (One is needed here only to make the
/// stream and judge the result.)
#[cfg(unix)]
#[tokio::test]
async fn an_hls_stream_becomes_an_mp4_with_no_ffmpeg() {
    let _turn = FFMPEG_ENV.lock().await;
    let Some(real) = ffmpeg() else {
        eprintln!("no ffmpeg installed — skipping");
        return;
    };
    let site = tempdir();
    if !make_stream(&real, site.path(), 3, "libx264") {
        return;
    }
    let addr = serve_folder(site.path().to_path_buf()).await;
    std::env::set_var("DM_FFMPEG", "");
    let dir = tempdir();
    let asked = Options {
        room: Default::default(),
        remux: true,
        ..options(dir.path(), dm_core::stream::Quality::Best)
    };
    let done = download(&client(), &format!("http://{addr}/show.m3u8"), &asked).await;
    std::env::remove_var("DM_FFMPEG");
    let done = done.expect("the stream");
    assert_eq!(
        done.path.file_name().and_then(|n| n.to_str()),
        Some("show.mp4")
    );
    let probed = std::process::Command::new(&real)
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(&done.path)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        probed.status.success() && probed.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&probed.stderr)
    );
    let left = std::fs::read_dir(dir.path().join("parts"))
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(left, 0, "parts were left behind");
}

#[cfg(unix)]
#[tokio::test]
async fn an_mp4_left_from_an_earlier_run_is_not_placed_when_the_remux_is_off() {
    let _turn = FFMPEG_ENV.lock().await;
    let Some(real) = ffmpeg() else {
        return;
    };
    let site = tempdir();
    if !make_stream(&real, site.path(), 3, "libx264") {
        return;
    }
    let addr = serve_folder(site.path().to_path_buf()).await;
    let dir = tempdir();
    let off = options(dir.path(), dm_core::stream::Quality::Best);
    // What a pause after an earlier, remuxing run left behind.
    std::fs::create_dir_all(&off.parts_dir).unwrap();
    std::fs::write(off.parts_dir.join("joined.mp4"), b"an earlier run's mp4").unwrap();
    let done = download(&client(), &format!("http://{addr}/show.m3u8"), &off)
        .await
        .unwrap();
    assert_eq!(
        done.path.file_name().and_then(|n| n.to_str()),
        Some("show.ts")
    );
    let playlist = std::fs::read_to_string(site.path().join("show.m3u8")).unwrap();
    let served: Vec<u8> = playlist
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .flat_map(|name| std::fs::read(site.path().join(name.trim())).unwrap())
        .collect();
    assert_eq!(std::fs::read(&done.path).unwrap(), served);
    assert!(
        !off.parts_dir.join("joined.mp4").exists(),
        "the stale MP4 was left"
    );
}

#[tokio::test]
async fn a_gather_cut_short_by_a_crash_is_cleared_when_the_stream_is_placed() {
    // A `.partial` left by a dm killed while gathering, on a run that does
    // not remux: placed from its segments, and the leftover goes.
    let (addr, _) = serve(false).await;
    let dir = tempdir();
    let opts = options(dir.path(), dm_core::stream::Quality::Best);
    std::fs::create_dir_all(&opts.parts_dir).unwrap();
    std::fs::write(opts.parts_dir.join("joined.ts.partial"), vec![0u8; 4096]).unwrap();
    let done = download(&client(), &format!("http://{addr}/v/master.m3u8"), &opts)
        .await
        .unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), whole());
    assert!(
        !opts.parts_dir.join("joined.ts.partial").exists(),
        "the leftover stayed"
    );
}

#[tokio::test]
async fn a_stream_cut_by_byte_ranges_is_refused_without_yt_dlp_not_saved_wrong() {
    // Saved as segments joined, it was the whole source repeated and called
    // done. Without yt-dlp to read it, it is refused with what fixes it.
    let _turn = FFMPEG_ENV.lock().await;
    let site = tempdir();
    std::fs::write(site.path().join("main.ts"), vec![7u8; 4096]).unwrap();
    std::fs::write(
        site.path().join("show.m3u8"),
        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\n#EXT-X-BYTERANGE:2048@0\nmain.ts\n\
         #EXTINF:4,\n#EXT-X-BYTERANGE:2048\nmain.ts\n#EXT-X-ENDLIST\n",
    )
    .unwrap();
    let addr = serve_folder(site.path().to_path_buf()).await;
    std::env::set_var("DM_YTDLP", "");
    let dir = tempdir();
    let outcome = download(
        &client(),
        &format!("http://{addr}/show.m3u8"),
        &options(dir.path(), dm_core::stream::Quality::Best),
    )
    .await;
    std::env::remove_var("DM_YTDLP");
    let said = outcome
        .expect_err("a byte-range stream was saved as joined segments")
        .to_string();
    assert!(said.contains("video helper"), "{said}");
    let saved = std::fs::read_dir(dir.path().join("out"))
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(saved, 0, "something was saved");
}

/// A short link that redirects to the CDN's master playlist, whose chosen
/// rendition redirects again to an edge. Relative entries belong to where
/// each playlist was served, and joined against the asked address every
/// rendition and segment was a 404.
#[tokio::test]
async fn relative_entries_are_joined_against_where_a_redirect_landed() {
    use axum::response::Redirect;
    use axum::routing::get;
    let mpegurl = [("content-type", "application/vnd.apple.mpegurl")];
    let app = axum::Router::new()
        .route(
            "/short/show.m3u8",
            get(|| async { Redirect::temporary("/cdn/v/master.m3u8") }),
        )
        .route(
            "/cdn/v/master.m3u8",
            get(move || async move {
                (
                    mpegurl,
                    "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1,RESOLUTION=10x10\nlow/index.m3u8\n",
                )
            }),
        )
        .route(
            "/cdn/v/low/index.m3u8",
            get(|| async { Redirect::temporary("/edge/x/index.m3u8") }),
        )
        .route(
            "/edge/x/index.m3u8",
            get(move || async move {
                (
                    mpegurl,
                    "#EXTM3U\n#EXTINF:4,\nseg0.ts\n#EXTINF:4,\nseg1.ts\n#EXT-X-ENDLIST\n",
                )
            }),
        )
        .route("/edge/x/seg0.ts", get(|| async { segment(0) }))
        .route("/edge/x/seg1.ts", get(|| async { segment(1) }));
    let addr = common::serve(app).await;
    let dir = tempdir();
    let done = download::download(
        &client(),
        &format!("http://{addr}/short/show.m3u8"),
        &options(dir.path(), dm_core::stream::Quality::Best),
    )
    .await
    .unwrap();
    let expected: Vec<u8> = (0..2).flat_map(segment).collect();
    assert_eq!(std::fs::read(&done.path).unwrap(), expected);
}

#[tokio::test]
async fn packed_audio_from_tokenised_addresses_is_saved_as_aac() {
    /* Packed audio was told only by every segment's path ending in `.aac`;
     * from addresses that are tokens it was called a transport stream and
     * handed to the remux. The first segment's first bytes say what it is:
     * here ADTS, behind the ID3 tag that carries its timestamp. */
    let _turn = FFMPEG_ENV.lock().await;
    std::env::set_var("DM_FFMPEG", "");
    fn frames(index: u8) -> Vec<u8> {
        let mut out = b"ID3\x04\0\0\0\0\0\x03".to_vec();
        out.extend([0, 0, index]);
        for n in 0..64u8 {
            out.extend([0xFF, 0xF1, 0x50, 0x80, 0x02, 0x1F, 0xFC, index, n]);
        }
        out
    }
    let playlist = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\nseg/0?token=a1\n\
                    #EXTINF:4,\nseg/1?token=b2\n#EXT-X-ENDLIST\n";
    let app = axum::Router::new()
        .route(
            "/radio/index.m3u8",
            axum::routing::get(move || async move {
                (
                    [("content-type", "application/vnd.apple.mpegurl")],
                    playlist,
                )
            }),
        )
        .route(
            "/radio/seg/:n",
            axum::routing::get(
                |axum::extract::Path(n): axum::extract::Path<u8>| async move {
                    ([("content-type", "application/octet-stream")], frames(n))
                },
            ),
        );
    let addr = common::serve(app).await;
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("http://{addr}/radio/index.m3u8"),
        &common::options(dir.path()),
    )
    .await
    .unwrap();
    assert_eq!(done.path.file_name().unwrap(), "radio.aac");
    let mut both = frames(0);
    both.extend(frames(1));
    assert_eq!(std::fs::read(&done.path).unwrap(), both);
}
