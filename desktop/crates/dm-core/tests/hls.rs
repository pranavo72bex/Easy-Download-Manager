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

/// A real HLS stream kept beside the tests, one folder per stream: three
/// seconds of a 128x72 test pattern at ten pictures a second with a 440 Hz
/// tone, cut into one-second transport stream segments, made once and kept
/// here so no tool is needed to run the tests. `h264` is H.264 (a keyframe
/// each second) and AAC, which dm puts into an MP4 itself; `mpeg2` is MPEG-2
/// pictures, which it does not.
fn stream(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/media/hls")
        .join(name)
}

/// A folder served as a site serves its HLS, down to the bytes.
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

/// The segments a folder's `show.m3u8` names, joined in its order.
fn served(site: &std::path::Path) -> Vec<u8> {
    let playlist = std::fs::read_to_string(site.join("show.m3u8")).unwrap();
    playlist
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .flat_map(|name| std::fs::read(site.join(name.trim())).unwrap())
        .collect()
}

/// `DM_YTDLP` is process-wide: the tests that set it, or could be routed by
/// it, take turns.
static ENV: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// An MP4 read box by box, apart from dm's own reader: the tracks a player
/// finds, and where each sample's bytes are.
mod mp4 {
    pub struct Track {
        pub codec: [u8; 4],
        pub timescale: u32,
        /// Each sample's length, offset in the file and duration.
        pub sizes: Vec<u32>,
        pub offsets: Vec<u64>,
        pub durations: Vec<u32>,
        /// The first sample's composition offset, from `ctts`.
        pub first_offset: i64,
        /// The edit list: (duration in the movie's timescale, media time).
        pub edits: Vec<(u64, i64)>,
    }

    pub struct Movie {
        pub timescale: u32,
        pub tracks: Vec<Track>,
    }

    fn u32_at(b: &[u8], at: usize) -> u32 {
        u32::from_be_bytes(b[at..at + 4].try_into().unwrap())
    }

    fn u64_at(b: &[u8], at: usize) -> u64 {
        u64::from_be_bytes(b[at..at + 8].try_into().unwrap())
    }

    /// The boxes directly inside `b`, as (kind, body).
    fn boxes(b: &[u8]) -> Vec<([u8; 4], &[u8])> {
        let mut out = Vec::new();
        let mut at = 0;
        while at + 8 <= b.len() {
            let mut size = u32_at(b, at) as usize;
            let kind: [u8; 4] = b[at + 4..at + 8].try_into().unwrap();
            let mut head = 8;
            if size == 1 {
                size = u64_at(b, at + 8) as usize;
                head = 16;
            } else if size == 0 {
                size = b.len() - at;
            }
            assert!(size >= head && at + size <= b.len(), "a box overruns");
            out.push((kind, &b[at + head..at + size]));
            at += size;
        }
        assert_eq!(at, b.len(), "bytes after the last box");
        out
    }

    fn one<'a>(b: &'a [u8], kind: &[u8; 4]) -> &'a [u8] {
        let found: Vec<_> = boxes(b).into_iter().filter(|(k, _)| k == kind).collect();
        assert_eq!(found.len(), 1, "{} {kind:?} boxes", found.len());
        found[0].1
    }

    fn maybe<'a>(b: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
        boxes(b)
            .into_iter()
            .find(|(k, _)| k == kind)
            .map(|(_, v)| v)
    }

    /// The count after a full box's version and flags, then the entries.
    fn entries(b: &[u8], width: usize) -> impl Iterator<Item = &[u8]> {
        let count = u32_at(b, 4) as usize;
        assert!(8 + count * width <= b.len(), "a table overruns its box");
        (0..count).map(move |i| &b[8 + i * width..8 + (i + 1) * width])
    }

    pub fn read(file: &[u8]) -> Movie {
        let moov = one(file, b"moov");
        let mvhd = one(moov, b"mvhd");
        let timescale = if mvhd[0] == 1 {
            u32_at(mvhd, 20)
        } else {
            u32_at(mvhd, 12)
        };
        let tracks = boxes(moov)
            .into_iter()
            .filter(|(k, _)| k == b"trak")
            .map(|(_, trak)| track(trak))
            .collect();
        Movie { timescale, tracks }
    }

    fn track(trak: &[u8]) -> Track {
        let edits = maybe(trak, b"edts")
            .map(|edts| {
                let elst = one(edts, b"elst");
                let wide = elst[0] == 1;
                entries(elst, if wide { 20 } else { 12 })
                    .map(|e| {
                        if wide {
                            (u64_at(e, 0), u64_at(e, 8) as i64)
                        } else {
                            (u64::from(u32_at(e, 0)), i64::from(u32_at(e, 4) as i32))
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mdia = one(trak, b"mdia");
        let mdhd = one(mdia, b"mdhd");
        let timescale = if mdhd[0] == 1 {
            u32_at(mdhd, 20)
        } else {
            u32_at(mdhd, 12)
        };
        let stbl = one(one(mdia, b"minf"), b"stbl");
        let stsd = one(stbl, b"stsd");
        assert_eq!(u32_at(stsd, 4), 1, "one sample description");
        let codec: [u8; 4] = stsd[12..16].try_into().unwrap();

        let stsz = one(stbl, b"stsz");
        let fixed = u32_at(stsz, 4);
        let count = u32_at(stsz, 8) as usize;
        let sizes: Vec<u32> = if fixed != 0 {
            vec![fixed; count]
        } else {
            (0..count).map(|i| u32_at(stsz, 12 + i * 4)).collect()
        };

        let chunks: Vec<u64> = match maybe(stbl, b"co64") {
            Some(co64) => entries(co64, 8).map(|e| u64_at(e, 0)).collect(),
            None => entries(one(stbl, b"stco"), 4)
                .map(|e| u64::from(u32_at(e, 0)))
                .collect(),
        };
        // Samples per chunk, as runs from a first chunk (counted from one).
        let runs: Vec<(usize, usize)> = entries(one(stbl, b"stsc"), 12)
            .map(|e| (u32_at(e, 0) as usize, u32_at(e, 4) as usize))
            .collect();
        let mut offsets = Vec::with_capacity(count);
        for (n, &chunk) in chunks.iter().enumerate() {
            let per = runs
                .iter()
                .rev()
                .find(|(first, _)| *first <= n + 1)
                .map(|(_, per)| *per)
                .unwrap();
            let mut at = chunk;
            for _ in 0..per {
                let size = sizes[offsets.len()];
                offsets.push(at);
                at += u64::from(size);
            }
        }
        assert_eq!(offsets.len(), count, "the chunks do not hold every sample");

        let durations: Vec<u32> = entries(one(stbl, b"stts"), 8)
            .flat_map(|e| std::iter::repeat_n(u32_at(e, 4), u32_at(e, 0) as usize))
            .collect();
        assert_eq!(durations.len(), count, "stts does not time every sample");
        let first_offset = maybe(stbl, b"ctts")
            .map(|ctts| {
                let e = entries(ctts, 8).next().unwrap();
                i64::from(u32_at(e, 4) as i32)
            })
            .unwrap_or(0);

        Track {
            codec,
            timescale,
            sizes,
            offsets,
            durations,
            first_offset,
            edits,
        }
    }

    impl Track {
        /// Every sample's bytes, in order.
        pub fn samples(&self, file: &[u8]) -> Vec<u8> {
            self.offsets
                .iter()
                .zip(&self.sizes)
                .flat_map(|(&at, &size)| file[at as usize..at as usize + size as usize].to_vec())
                .collect()
        }

        /// How long its samples last, in seconds.
        pub fn seconds(&self) -> f64 {
            self.durations.iter().map(|&d| f64::from(d)).sum::<f64>() / f64::from(self.timescale)
        }

        /// When its first picture or sound is shown, in seconds from the
        /// movie's start: after any empty edit, from the media time the
        /// edit starts at.
        pub fn starts(&self, movie_timescale: u32) -> f64 {
            let mut lead = 0.0;
            for &(duration, media_time) in &self.edits {
                if media_time == -1 {
                    lead += duration as f64 / f64::from(movie_timescale);
                } else {
                    return lead
                        + (self.first_offset - media_time) as f64 / f64::from(self.timescale);
                }
            }
            lead + self.first_offset as f64 / f64::from(self.timescale)
        }
    }
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(bytes))
}

/// The MP4 dm makes of the `h264` stream, judged as a player would find it:
/// a picture track and a sound track, every picture and sound frame of the
/// stream in it, three seconds of each starting together, and the frames'
/// bytes exactly those of an MP4 a decoder played through to the end.
fn assert_the_h264_stream_as_mp4(path: &std::path::Path) {
    let file = std::fs::read(path).unwrap();
    assert_eq!(file.get(4..8), Some(&b"ftyp"[..]), "not an MP4");
    let movie = mp4::read(&file);
    assert_eq!(movie.tracks.len(), 2, "a picture and a sound track");
    let picture = movie.tracks.iter().find(|t| &t.codec == b"avc1");
    let sound = movie.tracks.iter().find(|t| &t.codec == b"mp4a");
    let (Some(picture), Some(sound)) = (picture, sound) else {
        panic!("not H.264 and AAC");
    };
    // Ten pictures a second for three seconds; AAC frames of 1024 samples
    // at 44.1 kHz, the encoder's first one included.
    assert_eq!(picture.sizes.len(), 30, "pictures");
    assert_eq!(sound.sizes.len(), GOLDEN_SOUND_FRAMES, "sound frames");
    for (what, track) in [("picture", picture), ("sound", sound)] {
        let seconds = track.seconds();
        assert!(
            (seconds - 3.0).abs() < 0.1,
            "the {what} lasts {seconds} s, not three"
        );
    }
    let (p, s) = (
        picture.starts(movie.timescale),
        sound.starts(movie.timescale),
    );
    // Together as the stream had them: its first picture is shown 23 ms
    // after its first sound.
    assert!(
        (p - s - 0.023).abs() < 0.002,
        "the picture starts at {p} s and the sound at {s} s"
    );
    assert_eq!(sha256(&picture.samples(&file)), GOLDEN_PICTURE, "pictures");
    assert_eq!(sha256(&sound.samples(&file)), GOLDEN_SOUND, "sound");
}

/// Read off dm's MP4 of the `h264` stream once a separate decoder had
/// played it through without a word; that decoder's own copy of each
/// track's frames hashed the same.
const GOLDEN_SOUND_FRAMES: usize = 131;
const GOLDEN_PICTURE: &str = "822e4024f0dc32da59eee0eec30f3244861cd78afc1f50ebff5664303cd847c8";
const GOLDEN_SOUND: &str = "e097c5875f8540a974afc0d30c0e14270269722717bca6a06d279cbfb0354087";

#[tokio::test]
async fn a_remux_that_fails_keeps_the_stream_whole_as_ts() {
    let _turn = ENV.lock().await;
    let site = stream("mpeg2");
    let addr = serve_folder(site.clone()).await;
    // MPEG-2 pictures, which dm does not put into an MP4: the segments,
    // already gathered into one stream, are saved as that stream.
    let dir = tempdir();
    let asked = Options {
        room: Default::default(),
        remux: true,
        ..options(dir.path(), dm_core::stream::Quality::Best)
    };
    let done = download(&client(), &format!("http://{addr}/show.m3u8"), &asked).await;
    let done = done.expect("a failed remux failed the download");
    assert_eq!(
        done.path.file_name().and_then(|n| n.to_str()),
        Some("show.ts")
    );
    assert_eq!(
        std::fs::read(&done.path).unwrap(),
        served(&site),
        "not the stream, whole and in order"
    );
    let left = std::fs::read_dir(dir.path().join("parts"))
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(left, 0, "segments or a half-made MP4 were left behind");
}

#[tokio::test]
async fn a_transport_stream_is_saved_as_an_mp4_when_asked_and_as_ts_when_not() {
    let _turn = ENV.lock().await;
    let site = stream("h264");
    let addr = serve_folder(site.clone()).await;
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
        done.path.file_name().and_then(|n| n.to_str()),
        Some("show.mp4")
    );
    assert_the_h264_stream_as_mp4(&done.path);
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
    // The segments the playlist names, in its order.
    assert_eq!(
        std::fs::read(&kept.path).unwrap(),
        served(&site),
        "not the segments, in order"
    );
}

/// H.264 and AAC, as nearly every HLS stream is, put into an MP4 by dm,
/// with no tool of the computer's: offered as a master playlist's only
/// rendition, the way a site serves it.
#[tokio::test]
async fn an_hls_stream_becomes_an_mp4_dm_makes_itself() {
    let _turn = ENV.lock().await;
    let site = stream("h264");
    let addr = serve_folder(site).await;
    let master = format!(
        "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=200000,RESOLUTION=128x72\n\
         http://{addr}/show.m3u8\n"
    );
    let app = axum::Router::new().route(
        "/clip/master.m3u8",
        axum::routing::get(move || async move {
            ([("content-type", "application/vnd.apple.mpegurl")], master)
        }),
    );
    let front = common::serve(app).await;
    let dir = tempdir();
    let asked = Options {
        room: Default::default(),
        remux: true,
        ..options(dir.path(), dm_core::stream::Quality::Best)
    };
    let done = download(
        &client(),
        &format!("http://{front}/clip/master.m3u8"),
        &asked,
    )
    .await;
    let done = done.expect("the stream");
    assert_eq!(
        done.path.file_name().and_then(|n| n.to_str()),
        Some("clip.mp4")
    );
    assert_the_h264_stream_as_mp4(&done.path);
    let left = std::fs::read_dir(dir.path().join("parts"))
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(left, 0, "parts were left behind");
}

#[tokio::test]
async fn an_mp4_left_from_an_earlier_run_is_not_placed_when_the_remux_is_off() {
    let _turn = ENV.lock().await;
    let site = stream("h264");
    let addr = serve_folder(site.clone()).await;
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
    assert_eq!(std::fs::read(&done.path).unwrap(), served(&site));
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
    let _turn = ENV.lock().await;
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
    let _turn = ENV.lock().await;
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

/// A live stream: its playlist keeps a window of the latest three segments
/// and adds one each time it is asked for, until it says the stream ended
/// after six. Recorded as it goes, each segment once, in order.
async fn serve_live(segments: usize) -> SocketAddr {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let asked = Arc::new(AtomicUsize::new(0));
    let app = axum::Router::new().fallback(move |uri: Uri| {
        let asked = Arc::clone(&asked);
        async move {
            let path = uri.path().to_string();
            if path.ends_with(".m3u8") {
                // One more segment out with each request, up to `segments`.
                let out = (asked.fetch_add(1, Ordering::SeqCst) + 1).min(segments);
                let first = out.saturating_sub(3);
                let mut text = format!(
                    "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:{first}\n"
                );
                for n in first..out {
                    text.push_str(&format!("#EXTINF:1.0,\nseg{n}.ts\n"));
                }
                if out == segments {
                    text.push_str("#EXT-X-ENDLIST\n");
                }
                return Response::new(Body::from(text));
            }
            let n: usize = path
                .trim_start_matches("/seg")
                .trim_end_matches(".ts")
                .parse()
                .unwrap_or(999);
            Response::new(Body::from(segment(n)))
        }
    });
    common::serve(app).await
}

#[tokio::test]
async fn a_live_stream_is_recorded_as_it_goes_each_segment_once() {
    let addr = serve_live(6).await;
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("http://{addr}/live.m3u8"),
        &options(dir.path(), dm_core::stream::Quality::Best),
    )
    .await
    .unwrap();
    let got = std::fs::read(&done.path).unwrap();
    let wanted: Vec<u8> = (0..6).flat_map(segment).collect();
    assert_eq!(got, wanted, "{:?}", done.path);
}

#[tokio::test]
async fn a_live_recording_stopped_keeps_what_it_recorded() {
    // A stream that would run for a long time.
    let addr = serve_live(10_000).await;
    let dir = tempdir();
    let opts = options(dir.path(), dm_core::stream::Quality::Best);
    let control = opts.control.clone();
    let stop = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(1800)).await;
        control.pause();
    });
    let done = download(&client(), &format!("http://{addr}/live.m3u8"), &opts)
        .await
        .unwrap();
    stop.await.unwrap();
    let got = std::fs::read(&done.path).unwrap();
    // Some segments, whole and in order from the first.
    let first = segment(0).len();
    assert!(got.len() >= first * 2, "{} bytes", got.len());
    assert_eq!(&got[..first], segment(0).as_slice());
}
