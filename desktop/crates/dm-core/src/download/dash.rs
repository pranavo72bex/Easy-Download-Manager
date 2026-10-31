use super::*;
use crate::dash::{self, Medium, Track};

/// Fetches a DASH stream as one file.
///
/// The manifest is read by dm (`dash`), one picture and one sound are chosen
/// by the quality asked for, each is fetched piece by piece into the parts
/// folder and gathered into a fragmented MP4 (or WebM) of its own, and the
/// two are joined by dm's own muxer, copying the frames as they are. Like an
/// HLS stream there is no length and no position inside a piece to resume
/// from: pieces that arrived whole are kept and not fetched again.
pub(super) async fn download_dash(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    let (text, landed) = hls::fetch_playlist(client, url, options).await?;
    let plan = dash::plan(
        &text,
        &landed,
        options.quality,
        options.container.as_deref(),
    )?;

    let tracks: Vec<&Track> = plan.video.iter().chain(plan.audio.iter()).collect();
    // WebM where it was asked for and every track is one; else an MP4,
    // which takes a WebM picture or sound too. Sound alone is an `.m4a`.
    let webm = options.container.as_deref() == Some("webm") && tracks.iter().all(|t| t.webm);
    let extension = match (&plan.video, webm) {
        (None, _) => "m4a",
        (Some(_), true) => "webm",
        (Some(_), false) => "mp4",
    };
    let filename = stream::name_for(url, extension);
    let signature = format!("{url}\n{}", plan.signature());
    join_tracks(
        client, &tracks, &filename, extension, webm, &signature, options, report,
    )
    .await
}

/// An HLS picture and sound kept in playlists of their own (plain
/// fragmented MP4 on both sides), fetched and joined as the two tracks of a
/// DASH stream are.
pub(super) async fn download_pair(
    client: &reqwest::Client,
    picture: &stream::Stream,
    sound: &stream::Stream,
    rendition: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    // A key is fetched once for the stream it opens, then each piece is
    // given it with the vector its own number makes (the initialisation
    // segment is never scrambled).
    let (picture_key, sound_key) = tokio::try_join!(
        key_of(client, picture, options),
        key_of(client, sound, options)
    )?;
    let track = |medium, s: &stream::Stream, key: &Option<(stream::Key, Vec<u8>)>| Track {
        medium,
        id: String::new(),
        bandwidth: 0,
        height: 0,
        webm: false,
        pieces: s
            .parts
            .iter()
            .map(|p| dash::Piece {
                url: p.url.clone(),
                range: p.range,
                lock: match key {
                    Some((declared, bytes)) if !p.init => Some(dash::Lock {
                        key: bytes.clone(),
                        iv: stream::iv_for(declared, p.sequence),
                    }),
                    _ => None,
                },
            })
            .collect(),
    };
    let tracks = [
        track(Medium::Video, picture, &picture_key),
        track(Medium::Audio, sound, &sound_key),
    ];
    let refs: Vec<&Track> = tracks.iter().collect();
    let signature = format!("pair {rendition}");
    // A transport-stream picture is joined into an MP4 all the same.
    let name = if picture.container == "ts" {
        format!("{}.mp4", hls::ts_stem(&picture.filename))
    } else {
        picture.filename.clone()
    };
    join_tracks(
        client, &refs, &name, "mp4", false, &signature, options, report,
    )
    .await
}

/// The key an HLS stream's segments are scrambled with, fetched once, with
/// what says which vector each takes.
async fn key_of(
    client: &reqwest::Client,
    s: &stream::Stream,
    options: &Options,
) -> Result<Option<(stream::Key, Vec<u8>)>, Error> {
    match &s.key {
        Some(declared) => Ok(Some((
            declared.clone(),
            fetch_capped(client, &declared.uri, 1024, options)
                .await?
                .body,
        ))),
        None => Ok(None),
    }
}

/// A picture and a sound a player fetched apart (two plain files, as a video
/// site serves them), fetched together and joined as the two tracks of a DASH
/// stream are. Each is read in pieces by byte range, so a server that does not
/// honour ranges is refused here rather than written in as the whole of a
/// piece.
pub(super) async fn download_media_pair(
    client: &reqwest::Client,
    picture: &str,
    sound: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    /// The size of one request. Large enough that setting a request up is
    /// small beside carrying it, small enough that a stop is heard promptly.
    const PIECE: u64 = 8 * 1024 * 1024;

    let track = |medium, url: &str, total: u64| Track {
        medium,
        id: String::new(),
        bandwidth: 0,
        height: 0,
        webm: query_of(url, "mime").is_some_and(|m| m.contains("webm")),
        pieces: pieces_of(url, total, PIECE),
    };
    let (picture_size, sound_size) = tokio::try_join!(
        total_of(client, picture, options),
        total_of(client, sound, options)
    )?;
    let tracks = [
        track(Medium::Video, picture, picture_size),
        track(Medium::Audio, sound, sound_size),
    ];
    let refs: Vec<&Track> = tracks.iter().collect();
    // The name a person gave, made an `.mp4` as what is joined is one.
    let filename = match options
        .save_name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
    {
        Some(name) if name.to_ascii_lowercase().ends_with(".mp4") => name.to_string(),
        Some(name) => format!("{name}.mp4"),
        None => "video.mp4".to_string(),
    };
    let signature = format!("media {picture}\n{sound}");
    // `finish` places the file under the chosen name as it stands.
    let options = &Options {
        save_name: Some(filename.clone()),
        ..options.clone()
    };
    join_tracks(
        client, &refs, &filename, "mp4", false, &signature, options, report,
    )
    .await
}

/// The value of one query parameter of a URL, where it has it.
pub(crate) fn query_of(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1.split('#').next()?;
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then(|| value.replace("%2F", "/").replace("%2f", "/"))
    })
}

/// `total` bytes cut into requests of at most `piece` bytes each. A file of
/// no known length (0) is one request with no range.
pub(crate) fn pieces_of(url: &str, total: u64, piece: u64) -> Vec<dash::Piece> {
    if total == 0 {
        return vec![dash::Piece {
            url: url.to_string(),
            range: None,
            lock: None,
        }];
    }
    let mut pieces = Vec::new();
    let mut first = 0;
    while first < total {
        let last = (first + piece).min(total) - 1;
        pieces.push(dash::Piece {
            url: url.to_string(),
            range: Some((first, last)),
            lock: None,
        });
        first = last + 1;
    }
    pieces
}

/// How long a file is, asked of the server by a one-byte range: the length
/// after the slash of `Content-Range`. A server that answers the whole file
/// does not do ranges, and is refused rather than fetched twice as often.
async fn total_of(client: &reqwest::Client, url: &str, options: &Options) -> Result<u64, Error> {
    let request = get(
        client,
        url,
        options.referrer.as_deref(),
        options.sign_in.as_ref(),
    )
    .header(reqwest::header::RANGE, "bytes=0-0");
    let res = or_stop(&options.control, crate::auth::send(client, request, url)).await?;
    let res = Error::refused_or(res).await?;
    if res.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(Error::Internal(format!(
            "{url} does not serve byte ranges, which a picture and sound fetched apart need"
        )));
    }
    res.headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit('/').next())
        .and_then(|n| n.trim().parse::<u64>().ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| Error::Internal(format!("{url} did not say how long it is")))
}

/// The tracks fetched, gathered one file each, and joined into the file
/// named `filename`, placed where downloads go. `signature` says what is
/// being fetched: parts staged for something else are not adopted.
#[allow(clippy::too_many_arguments)]
async fn join_tracks(
    client: &reqwest::Client,
    tracks: &[&Track],
    filename: &str,
    extension: &str,
    webm: bool,
    signature: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    let filename = filename.to_string();
    fs::create_dir_all(&options.parts_dir).await?;
    hls::keep_only_parts_of(&options.parts_dir, signature).await?;

    if let Some(tx) = &report {
        let _ = tx
            .send(Update::Named {
                name: filename.clone(),
                category: category::classify(&filename, None),
            })
            .await;
    }
    fs::create_dir_all(&options.dest_dir).await?;

    // Joined before a stop: that file is whole (it is named only when it is),
    // so it is placed and nothing is fetched again.
    let joined = options.parts_dir.join(format!("joined.{extension}"));
    if !fs::metadata(&joined).await.is_ok_and(|m| m.len() > 0) {
        let downloaded = Arc::new(AtomicU64::new(0));
        let meter = report.map(|tx| watch(Arc::clone(&downloaded), None, tx));
        let fetched = fetch_tracks(client, tracks, options, &downloaded).await;
        if let Some((handle, finished)) = meter {
            let _ = finished.send(());
            let _ = handle.await;
        }
        let files = fetched?;
        options.control.check()?;

        let partial = options
            .parts_dir
            .join(format!("joined.partial.{extension}"));
        let _ = fs::remove_file(&partial).await;
        // Run to the end like the remux of a transport stream: cut loose
        // from a pause it would go on writing a file a resume makes afresh.
        let made = if webm {
            crate::mkvmux::join_async(files.clone(), partial.clone()).await
        } else if let [picture, sound] = files.as_slice().iter().collect::<Vec<_>>()[..]
            .iter()
            .map(|p| (*p).clone())
            .collect::<Vec<_>>()
            .as_slice()
        {
            if crate::tsmux::is_transport_stream(picture) && crate::tsmux::is_packed_aac(sound) {
                crate::tsmux::join_packed_async(picture.clone(), sound.clone(), partial.clone())
                    .await
            } else {
                crate::mp4mux::join_async(files.clone(), partial.clone()).await
            }
        } else {
            crate::mp4mux::join_async(files.clone(), partial.clone()).await
        };
        if let Err(why) = made {
            let _ = fs::remove_file(&partial).await;
            return Err(Error::Internal(format!(
                "the picture and sound could not be joined: {why}"
            )));
        }
        fs::rename(&partial, &joined).await?;
        for file in files {
            let _ = fs::remove_file(file).await;
        }
    }

    let (path, bytes, category) = finish(&filename, None, &[joined], options).await?;
    Ok(Finished {
        left_behind: None,
        path,
        bytes,
        connections: 1,
        category,
        verified: options.checksum.is_some(),
        mirrors: 1,
    })
}

/// Every piece of each track, then each track gathered into a file of its
/// own. The tracks are fetched at the same time (a picture and a sound do not
/// wait for one another), and the pieces of a track on as many connections as
/// are set. A piece is written under a temporary name and moved when whole,
/// so a part file that is present is a finished one.
async fn fetch_tracks(
    client: &reqwest::Client,
    tracks: &[&Track],
    options: &Options,
    downloaded: &AtomicU64,
) -> Result<Vec<PathBuf>, Error> {
    let mut first = 0usize;
    let mut work = Vec::new();
    for (n, track) in tracks.iter().enumerate() {
        work.push(fetch_track(client, track, n, first, options, downloaded));
        first += track.pieces.len();
    }
    futures_util::future::try_join_all(work).await
}

/// One track: its pieces (numbered from `first`), gathered into `track{n}`.
async fn fetch_track(
    client: &reqwest::Client,
    track: &Track,
    n: usize,
    first: usize,
    options: &Options,
    downloaded: &AtomicU64,
) -> Result<PathBuf, Error> {
    let kind = match (track.medium, track.webm) {
        (_, true) => "webm",
        (Medium::Video, false) => "mp4",
        (Medium::Audio, false) => "m4a",
    };
    let gathered = options.parts_dir.join(format!("track{n}.{kind}"));
    // Gathered before a stop: its pieces were freed as they were
    // consumed, and the file is named only when whole.
    if fs::metadata(&gathered).await.is_ok_and(|m| m.len() > 0) {
        return Ok(gathered);
    }
    // As many at once as there are connections, each taking the next piece as
    // it finishes its own: a slow piece holds only the connection it is on.
    let at_once = options.connections.max(1);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let worker = || async {
        loop {
            let i = next.fetch_add(1, Ordering::Relaxed);
            let Some(piece) = track.pieces.get(i) else {
                return Ok::<(), Error>(());
            };
            fetch_piece(client, piece, first + i, options, downloaded).await?;
        }
    };
    futures_util::future::try_join_all((0..at_once).map(|_| worker())).await?;
    let parts: Vec<PathBuf> = (first..first + track.pieces.len())
        .map(|i| part_path(&options.parts_dir, i))
        .collect();
    let partial = options.parts_dir.join(format!("track{n}.partial"));
    join_parts(&parts, &partial).await?;
    fs::rename(&partial, &gathered).await?;
    Ok(gathered)
}

/// One piece, written to its part file when it is whole.
async fn fetch_piece(
    client: &reqwest::Client,
    piece: &dash::Piece,
    index: usize,
    options: &Options,
    downloaded: &AtomicU64,
) -> Result<(), Error> {
    let done = part_path(&options.parts_dir, index);
    if let Ok(meta) = fs::metadata(&done).await {
        downloaded.fetch_add(meta.len(), Ordering::Relaxed);
        return Ok(());
    }
    options.control.check()?;
    let staging = done.with_extension("partial");
    let body = retry::run_up_to(&options.control, options.tries(), || {
        hls::fetch_segment_range(client, &piece.url, piece.range, options)
    })
    .await?;
    let body = match &piece.lock {
        Some(lock) => stream::decrypt(&lock.key, &lock.iv, &body)?,
        None => body,
    };
    downloaded.fetch_add(body.len() as u64, Ordering::Relaxed);
    fs::write(&staging, &body).await?;
    fs::rename(&staging, &done).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mp4mux::tests::{fragmented, plain};
    use axum::body::Body;
    use axum::http::{header, HeaderMap, StatusCode};
    use axum::response::Response;
    use axum::routing::get;

    fn options(dir: &Path) -> Options {
        Options {
            connections: 4,
            control: Control::new(),
            dest_dir: dir.join("out"),
            parts_dir: dir.join("parts"),
            limiter: Limiter::unlimited(),
            categories: Categories::default(),
            checksum: None,
            quality: Quality::Best,
            sound: None,
            span: None,
            shape: None,
            mp3: false,
            media: false,
            referrer: None,
            proxy: None,
            container: None,
            attempts: 2,
            host_limits: Vec::new(),
            proxy_bypass: Vec::new(),
            subtitles: false,
            room: Default::default(),
            remux: false,
            server_dates: false,
            retry_wait: 0,
            record_limit: None,
            sign_in: None,
            save_name: None,
            system_proxy: false,
        }
    }

    /// Offsets where each top-level box starts, and the end.
    fn box_starts(data: &[u8]) -> Vec<usize> {
        let mut at = 0;
        let mut starts = vec![];
        while at + 8 <= data.len() {
            starts.push(at);
            at += u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
        }
        starts.push(data.len());
        starts
    }

    /// Serves `body`, answering a byte range the way a CDN does.
    fn ranged(body: Vec<u8>, honours: bool) -> axum::routing::MethodRouter {
        get(move |headers: HeaderMap| {
            let body = body.clone();
            async move {
                let asked = headers
                    .get(header::RANGE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.strip_prefix("bytes="))
                    .and_then(|v| v.split_once('-'))
                    .and_then(|(a, b)| Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?)));
                match asked {
                    Some((a, b)) if honours => Response::builder()
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header(
                            header::CONTENT_RANGE,
                            format!("bytes {a}-{}/{}", b.min(body.len() - 1), body.len()),
                        )
                        .body(Body::from(body[a..=b.min(body.len() - 1)].to_vec()))
                        .unwrap(),
                    _ => Response::new(Body::from(body)),
                }
            }
        })
    }

    async fn serve(honours: bool) -> (String, tempfile::TempDir, usize) {
        let dir = tempfile::tempdir().unwrap();
        let (video, _) = fragmented(dir.path(), "v.mp4", 4);
        let (audio, _) = plain(dir.path(), "a.m4a", 7, 200);
        let video = std::fs::read(video).unwrap();
        let audio = std::fs::read(audio).unwrap();
        let starts = box_starts(&video);
        // ftyp, moov, then a moof and an mdat for each of four fragments.
        let init_end = starts[2] - 1;
        let mut list = format!(r#"<Initialization range="0-{init_end}"/>"#);
        for f in 0..4 {
            let (a, b) = (starts[2 + f * 2], starts[4 + f * 2] - 1);
            list.push_str(&format!(r#"<SegmentURL mediaRange="{a}-{b}"/>"#));
        }
        let mpd = format!(
            r#"<MPD type="static" mediaPresentationDuration="PT4S"><Period>
            <AdaptationSet mimeType="video/mp4"><Representation id="v" bandwidth="9" height="360">
              <BaseURL>v.mp4</BaseURL><SegmentList>{list}</SegmentList></Representation></AdaptationSet>
            <AdaptationSet mimeType="audio/mp4"><Representation id="a" bandwidth="1">
              <BaseURL>a.m4a</BaseURL><SegmentBase indexRange="0-1"/></Representation></AdaptationSet>
            </Period></MPD>"#
        );
        let app = axum::Router::new()
            .route(
                "/show/video.mpd",
                get(move || {
                    let mpd = mpd.clone();
                    async move {
                        Response::builder()
                            .header(header::CONTENT_TYPE, "application/dash+xml")
                            .body(Body::from(mpd))
                            .unwrap()
                    }
                }),
            )
            .route("/show/v.mp4", ranged(video, honours))
            .route("/show/a.m4a", ranged(audio, honours));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}/show/video.mpd"), dir, 2)
    }

    #[tokio::test]
    async fn a_picture_by_byte_ranges_and_a_sound_are_joined_into_one_mp4() {
        let (url, dir, tracks) = serve(true).await;
        let options = options(dir.path());
        let client = reqwest::Client::new();
        let done = crate::download::download(&client, &url, &options)
            .await
            .unwrap();
        assert_eq!(done.path.file_name().unwrap(), "show.mp4");
        let data = std::fs::read(&done.path).unwrap();
        assert_eq!(&data[4..8], b"ftyp");
        let traks = data.windows(4).filter(|w| *w == b"trak").count();
        assert_eq!(traks, tracks, "a picture and a sound");
    }

    #[tokio::test]
    async fn a_server_that_ignores_the_range_is_not_written_in_as_the_piece() {
        let (url, dir, _) = serve(false).await;
        let options = options(dir.path());
        let client = reqwest::Client::new();
        let err = crate::download::download(&client, &url, &options)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("byte range"), "{err}");
        assert!(!options.dest_dir.join("show.mp4").exists());
    }

    #[tokio::test]
    async fn an_hls_picture_and_its_separate_sound_are_joined_into_one_mp4() {
        let dir = tempfile::tempdir().unwrap();
        let (video, _) = fragmented(dir.path(), "v.mp4", 2);
        let (audio, _) = plain(dir.path(), "a.m4a", 4, 50);
        let video = std::fs::read(video).unwrap();
        let audio = std::fs::read(audio).unwrap();
        let starts = box_starts(&video);
        let (init, first, second) = (
            video[..starts[2]].to_vec(),
            video[starts[2]..starts[4]].to_vec(),
            video[starts[4]..].to_vec(),
        );
        let text = |body: &'static str| {
            get(move || async move {
                Response::builder()
                    .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
                    .body(Body::from(body))
                    .unwrap()
            })
        };
        let bytes = |body: Vec<u8>| get(move || async move { Response::new(Body::from(body)) });
        let app = axum::Router::new()
            .route(
                "/hls/master.m3u8",
                text(
                    "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"en\",DEFAULT=YES,URI=\"sound.m3u8\"\n\
                     #EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=640x360,AUDIO=\"a\"\npicture.m3u8\n",
                ),
            )
            .route(
                "/hls/picture.m3u8",
                text("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4,\none.m4s\n#EXTINF:4,\ntwo.m4s\n#EXT-X-ENDLIST\n"),
            )
            .route(
                "/hls/sound.m3u8",
                text("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\nsound.m4s\n#EXT-X-ENDLIST\n"),
            )
            .route("/hls/init.mp4", bytes(init))
            .route("/hls/one.m4s", bytes(first))
            .route("/hls/two.m4s", bytes(second))
            .route("/hls/sound.m4s", bytes(audio));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let options = options(dir.path());
        let url = format!("http://{addr}/hls/master.m3u8");
        let done = crate::download::download(&reqwest::Client::new(), &url, &options)
            .await
            .unwrap();
        assert_eq!(done.path.extension().unwrap(), "mp4");
        let data = std::fs::read(&done.path).unwrap();
        assert_eq!(data.windows(4).filter(|w| *w == b"trak").count(), 2);
    }

    /// A transport-stream picture with its sound in packed AAC, which a
    /// master playlist keeps in a rendition of its own, comes out one MP4
    /// with both tracks — with no video helper to fall back on.
    #[tokio::test]
    async fn an_hls_transport_stream_picture_and_packed_aac_sound_are_joined_into_one_mp4() {
        let picture = include_bytes!("../../tests/fixtures/media/hls/h264/seg0.ts").to_vec();
        // 44.1 kHz stereo AAC-LC frames of 300 bytes behind an ID3 tag that
        // starts the sound with the picture.
        let mut sound = b"ID3\x04\x00\x00\x00\x00\x00\x4f".to_vec();
        sound.extend_from_slice(b"PRIV\x00\x00\x00\x35\x00\x00");
        sound.extend_from_slice(b"com.apple.streaming.transportStreamTimestamp\0");
        sound.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
        for _ in 0..80 {
            let mut frame = vec![0xff, 0xf1, 0x50, 0x80, 0x25, 0x80 | 0x1f, 0xfc];
            frame.resize(300, 7);
            sound.extend(frame);
        }
        let text = |body: &'static str| {
            get(move || async move {
                Response::builder()
                    .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
                    .body(Body::from(body))
                    .unwrap()
            })
        };
        let bytes = |body: Vec<u8>| get(move || async move { Response::new(Body::from(body)) });
        let app = axum::Router::new()
            .route(
                "/hls/master.m3u8",
                text(
                    "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"en\",DEFAULT=YES,URI=\"sound.m3u8\"\n\
                     #EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=640x360,AUDIO=\"a\"\npicture.m3u8\n",
                ),
            )
            .route(
                "/hls/picture.m3u8",
                text("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\none.ts\n#EXT-X-ENDLIST\n"),
            )
            .route(
                "/hls/sound.m3u8",
                text("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\nsound.aac\n#EXT-X-ENDLIST\n"),
            )
            .route("/hls/one.ts", bytes(picture))
            .route("/hls/sound.aac", bytes(sound));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let dir = tempfile::tempdir().unwrap();
        let options = options(dir.path());
        let url = format!("http://{addr}/hls/master.m3u8");
        let done = crate::download::download(&reqwest::Client::new(), &url, &options)
            .await
            .unwrap();
        assert_eq!(done.path.extension().unwrap(), "mp4");
        let data = std::fs::read(&done.path).unwrap();
        assert_eq!(data.windows(4).filter(|w| *w == b"trak").count(), 2);
        assert!(data.windows(4).any(|w| w == b"avc1"));
        assert!(data.windows(4).any(|w| w == b"mp4a"));
    }

    #[tokio::test]
    async fn an_encrypted_hls_pair_cut_by_byte_ranges_is_fetched_decrypted_and_joined() {
        use aes::cipher::{block_padding::Pkcs7, BlockModeEncrypt, InnerIvInit, KeyInit};
        let dir = tempfile::tempdir().unwrap();
        let (video, _) = fragmented(dir.path(), "v.mp4", 2);
        let (audio, _) = plain(dir.path(), "a.m4a", 4, 50);
        let video = std::fs::read(video).unwrap();
        let audio = std::fs::read(audio).unwrap();
        let starts = box_starts(&video);
        let (init, first, second) = (
            video[..starts[2]].to_vec(),
            video[starts[2]..starts[4]].to_vec(),
            video[starts[4]..].to_vec(),
        );
        let key = [9u8; 16];
        // Sealed as a player would: CBC under the vector the sequence makes.
        let seal = |data: &[u8], sequence: u64| {
            let mut iv = [0u8; 16];
            iv[8..].copy_from_slice(&sequence.to_be_bytes());
            cbc::Encryptor::<aes::Aes128>::inner_iv_slice_init(
                aes::Aes128::new_from_slice(&key).unwrap(),
                &iv,
            )
            .unwrap()
            .encrypt_padded_vec::<Pkcs7>(data)
        };
        // One file for the picture's two segments, back to back; the sound
        // is one segment in a file of its own.
        let (a, b) = (seal(&first, 0), seal(&second, 1));
        let mut picture_file = init.clone();
        picture_file.extend_from_slice(&a);
        picture_file.extend_from_slice(&b);
        let sealed_sound = seal(&audio, 0);
        let list = format!(
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n\
             #EXT-X-MAP:URI=\"p.mp4\",BYTERANGE=\"{}@0\"\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n\
             #EXTINF:4,\n#EXT-X-BYTERANGE:{}@{}\np.mp4\n#EXTINF:4,\n#EXT-X-BYTERANGE:{}\np.mp4\n#EXT-X-ENDLIST\n",
            init.len(),
            a.len(),
            init.len(),
            b.len()
        );
        let sound_list =
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n\
                          #EXTINF:4,\nsound.m4s\n#EXT-X-ENDLIST\n";
        let text = |body: String| {
            get(move || {
                let body = body.clone();
                async move {
                    Response::builder()
                        .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
                        .body(Body::from(body))
                        .unwrap()
                }
            })
        };
        let app = axum::Router::new()
            .route(
                "/hls/master.m3u8",
                text(
                    "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"en\",DEFAULT=YES,URI=\"sound.m3u8\"\n\
                     #EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=640x360,AUDIO=\"a\"\npicture.m3u8\n"
                        .into(),
                ),
            )
            .route("/hls/picture.m3u8", text(list))
            .route("/hls/sound.m3u8", text(sound_list.into()))
            .route("/hls/key.bin", get(move || async move { Response::new(Body::from(key.to_vec())) }))
            .route("/hls/p.mp4", ranged(picture_file, true))
            .route("/hls/sound.m4s", ranged(sealed_sound, true));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let options = options(dir.path());
        let url = format!("http://{addr}/hls/master.m3u8");
        let done = crate::download::download(&reqwest::Client::new(), &url, &options)
            .await
            .unwrap();
        assert_eq!(done.path.extension().unwrap(), "mp4");
        let data = std::fs::read(&done.path).unwrap();
        assert_eq!(data.windows(4).filter(|w| *w == b"trak").count(), 2);
    }

    #[test]
    fn a_file_is_cut_into_whole_ranges_with_no_gap_or_overlap() {
        let pieces = pieces_of("http://x/f", 25, 10);
        let ranges: Vec<_> = pieces.iter().map(|p| p.range).collect();
        assert_eq!(ranges, [Some((0, 9)), Some((10, 19)), Some((20, 24))]);
        assert_eq!(pieces_of("http://x/f", 10, 10)[0].range, Some((0, 9)));
        assert_eq!(pieces_of("http://x/f", 0, 10)[0].range, None);
    }

    #[test]
    fn a_query_parameter_is_read_by_name() {
        let url = "https://r1.example/videoplayback?itag=137&mime=video%2Fmp4&clen=99#x";
        assert_eq!(query_of(url, "mime").as_deref(), Some("video/mp4"));
        assert_eq!(query_of(url, "itag").as_deref(), Some("137"));
        assert_eq!(query_of(url, "nope"), None);
    }

    #[tokio::test]
    async fn a_picture_and_a_sound_fetched_apart_are_joined_into_one_mp4() {
        let dir = tempfile::tempdir().unwrap();
        let (video, _) = fragmented(dir.path(), "v.mp4", 3);
        let (audio, _) = plain(dir.path(), "a.m4a", 5, 120);
        let app = axum::Router::new()
            .route("/p/picture", ranged(std::fs::read(video).unwrap(), true))
            .route("/p/sound", ranged(std::fs::read(audio).unwrap(), true));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut options = options(dir.path());
        options.sound = Some(format!("http://{addr}/p/sound"));
        options.save_name = Some("clip".into());
        let url = format!("http://{addr}/p/picture");
        let done = crate::download::download(&reqwest::Client::new(), &url, &options)
            .await
            .unwrap();
        assert_eq!(done.path.file_name().unwrap(), "clip.mp4");
        let data = std::fs::read(&done.path).unwrap();
        assert_eq!(data.windows(4).filter(|w| *w == b"trak").count(), 2);
    }

    #[tokio::test]
    async fn a_sound_from_a_server_that_ignores_ranges_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (video, _) = fragmented(dir.path(), "v.mp4", 2);
        let (audio, _) = plain(dir.path(), "a.m4a", 3, 50);
        let app = axum::Router::new()
            .route("/p/picture", ranged(std::fs::read(video).unwrap(), true))
            .route("/p/sound", ranged(std::fs::read(audio).unwrap(), false));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut options = options(dir.path());
        options.sound = Some(format!("http://{addr}/p/sound"));
        let url = format!("http://{addr}/p/picture");
        let err = crate::download::download(&reqwest::Client::new(), &url, &options)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("byte ranges"), "{err}");
    }
}
