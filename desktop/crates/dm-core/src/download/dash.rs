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
    let track = |medium, s: &stream::Stream| Track {
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
                range: None,
            })
            .collect(),
    };
    let tracks = [track(Medium::Video, picture), track(Medium::Audio, sound)];
    let refs: Vec<&Track> = tracks.iter().collect();
    let signature = format!("pair {rendition}");
    let name = picture.filename.clone();
    join_tracks(
        client, &refs, &name, "mp4", false, &signature, options, report,
    )
    .await
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

/// Every piece of each track, in order, then each track gathered into a
/// file of its own. A piece is written under a temporary name and moved
/// when whole, so a part file that is present is a finished one.
async fn fetch_tracks(
    client: &reqwest::Client,
    tracks: &[&Track],
    options: &Options,
    downloaded: &AtomicU64,
) -> Result<Vec<PathBuf>, Error> {
    let mut index = 0usize;
    let mut files = Vec::new();
    for (n, track) in tracks.iter().enumerate() {
        let first = index;
        let kind = match (track.medium, track.webm) {
            (_, true) => "webm",
            (Medium::Video, false) => "mp4",
            (Medium::Audio, false) => "m4a",
        };
        let gathered = options.parts_dir.join(format!("track{n}.{kind}"));
        // Gathered before a stop: its pieces were freed as they were
        // consumed, and the file is named only when whole.
        if fs::metadata(&gathered).await.is_ok_and(|m| m.len() > 0) {
            index += track.pieces.len();
            files.push(gathered);
            continue;
        }
        for piece in &track.pieces {
            let done = part_path(&options.parts_dir, index);
            index += 1;
            if let Ok(meta) = fs::metadata(&done).await {
                downloaded.fetch_add(meta.len(), Ordering::Relaxed);
                continue;
            }
            options.control.check()?;
            let staging = done.with_extension("partial");
            let body = retry::run_up_to(&options.control, options.tries(), || {
                hls::fetch_segment_range(client, &piece.url, piece.range, options)
            })
            .await?;
            downloaded.fetch_add(body.len() as u64, Ordering::Relaxed);
            fs::write(&staging, &body).await?;
            fs::rename(&staging, &done).await?;
        }
        let parts: Vec<PathBuf> = (first..index)
            .map(|i| part_path(&options.parts_dir, i))
            .collect();
        let partial = options.parts_dir.join(format!("track{n}.partial"));
        join_parts(&parts, &partial).await?;
        fs::rename(&partial, &gathered).await?;
        files.push(gathered);
    }
    Ok(files)
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
}
