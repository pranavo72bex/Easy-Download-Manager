// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! HLS and live pairs, plain, transport stream and encrypted, joined into one MP4.

use super::*;

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
    let done = crate::download::download(&crate::reqwest_defaults(), &url, &options)
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
    let done = crate::download::download(&crate::reqwest_defaults(), &url, &options)
        .await
        .unwrap();
    assert_eq!(done.path.extension().unwrap(), "mp4");
    let data = std::fs::read(&done.path).unwrap();
    assert_eq!(data.windows(4).filter(|w| *w == b"trak").count(), 2);
    assert!(data.windows(4).any(|w| w == b"avc1"));
    assert!(data.windows(4).any(|w| w == b"mp4a"));
}

/// A live stream in fragmented MP4 whose sound is a playlist of its own:
/// both playlists grow a fragment at each ask and then end, and the
/// recording is one MP4 with a picture track and a sound track.
#[tokio::test]
async fn a_live_fragmented_picture_and_its_separate_sound_are_recorded_into_one_mp4() {
    use std::sync::atomic::AtomicUsize;
    let dir = tempfile::tempdir().unwrap();
    // The stand-in's tracks: the same shape of fragments, one of them
    // handled as sound.
    let split = |path: PathBuf, sound: bool| {
        let mut data = std::fs::read(path).unwrap();
        if sound {
            let at = data.windows(4).position(|w| w == b"vide").unwrap();
            data[at..at + 4].copy_from_slice(b"soun");
        }
        let starts = box_starts(&data);
        let mut pieces = vec![data[..starts[2]].to_vec()];
        for f in 0..3 {
            pieces.push(data[starts[2 + f * 2]..starts[4 + f * 2]].to_vec());
        }
        pieces
    };
    let picture = split(fragmented(dir.path(), "v.mp4", 3).0, false);
    let sound = split(fragmented(dir.path(), "a.mp4", 3).0, true);
    // At ask n, the fragments out so far; the end tag once all three are.
    let live = |name: &'static str, asks: Arc<AtomicUsize>| {
        get(move || {
            let asks = Arc::clone(&asks);
            async move {
                let out = (asks.fetch_add(1, Ordering::SeqCst) + 1).min(3);
                let mut text = format!(
                    "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n\
                         #EXT-X-MAP:URI=\"{name}-init.mp4\"\n"
                );
                for n in 0..out {
                    text.push_str(&format!("#EXTINF:1,\n{name}{n}.m4s\n"));
                }
                if out == 3 {
                    text.push_str("#EXT-X-ENDLIST\n");
                }
                Response::builder()
                    .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
                    .body(Body::from(text))
                    .unwrap()
            }
        })
    };
    let bytes = |body: Vec<u8>| get(move || async move { Response::new(Body::from(body)) });
    let mut app = axum::Router::new()
        .route(
            "/live/master.m3u8",
            get(|| async {
                Response::builder()
                    .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
                    .body(Body::from(
                        "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"en\",DEFAULT=YES,URI=\"sound.m3u8\"\n\
                             #EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=640x360,AUDIO=\"a\"\npicture.m3u8\n",
                    ))
                    .unwrap()
            }),
        )
        .route("/live/picture.m3u8", live("v", Arc::default()))
        .route("/live/sound.m3u8", live("a", Arc::default()))
        .route("/live/v-init.mp4", bytes(picture[0].clone()))
        .route("/live/a-init.mp4", bytes(sound[0].clone()));
    for n in 0..3 {
        app = app
            .route(&format!("/live/v{n}.m4s"), bytes(picture[n + 1].clone()))
            .route(&format!("/live/a{n}.m4s"), bytes(sound[n + 1].clone()));
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let options = options(dir.path());
    let url = format!("http://{addr}/live/master.m3u8");
    let done = crate::download::download(&crate::reqwest_defaults(), &url, &options)
        .await
        .unwrap();
    assert_eq!(done.path.extension().unwrap(), "mp4");
    let data = std::fs::read(&done.path).unwrap();
    assert_eq!(data.windows(4).filter(|w| *w == b"trak").count(), 2);
    assert!(data.windows(4).any(|w| w == b"vide"), "no picture track");
    assert!(data.windows(4).any(|w| w == b"soun"), "no sound track");
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
    let done = crate::download::download(&crate::reqwest_defaults(), &url, &options)
        .await
        .unwrap();
    assert_eq!(done.path.extension().unwrap(), "mp4");
    let data = std::fs::read(&done.path).unwrap();
    assert_eq!(data.windows(4).filter(|w| *w == b"trak").count(), 2);
}
