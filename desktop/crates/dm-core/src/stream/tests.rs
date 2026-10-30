#[test]
fn sound_alone_is_named_for_what_its_segments_are() {
    /* Called `.aac` whatever it was: fMP4 sound is an MP4 file and a
     * transport stream is a transport stream, and neither opened as the
     * raw AAC its name promised. */
    let at = "http://h/a/index.m3u8";
    let from = "http://h/show/master.m3u8";
    let fmp4 = "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4,\na.m4s\n#EXT-X-ENDLIST\n";
    let read = media_playlist(fmp4, at, from, Quality::Audio).unwrap();
    assert_eq!(
        (read.container.as_str(), read.filename.as_str()),
        ("m4a", "show.m4a")
    );
    let ts = "#EXTM3U\n#EXTINF:4,\na.ts\n#EXT-X-ENDLIST\n";
    let read = media_playlist(ts, at, from, Quality::Audio).unwrap();
    assert_eq!(
        (read.container.as_str(), read.filename.as_str()),
        ("ts", "show.ts")
    );
    let packed = "#EXTM3U\n#EXTINF:4,\na.aac\n#EXTINF:4,\nb.aac\n#EXT-X-ENDLIST\n";
    let read = media_playlist(packed, at, from, Quality::Audio).unwrap();
    assert_eq!(read.container, "aac");
}

#[test]
fn segments_are_told_apart_by_their_first_bytes() {
    let mut ts = vec![0u8; 376];
    ts[0] = 0x47;
    ts[188] = 0x47;
    assert_eq!(container_of_head(&ts), Some("ts"));
    assert_eq!(container_of_head(b"\0\0\0\x18ftypiso5"), Some("mp4"));
    assert_eq!(container_of_head(b"\0\0\0\x18stypmsdh"), Some("mp4"));
    assert_eq!(container_of_head(&[0xFF, 0xF1, 0x50, 0x80]), Some("aac"));
    let mut tagged = b"ID3\x04\0\0\0\0\0\x05".to_vec();
    tagged.extend([1, 2, 3, 4, 5, 0xFF, 0xF9, 0x50]);
    assert_eq!(container_of_head(&tagged), Some("aac"));
    assert_eq!(container_of_head(b"<html>"), None);
    assert_eq!(container_of_head(&[]), None);
}

#[test]
fn a_second_initialisation_segment_is_refused_not_swapped_in() {
    let remapped = "#EXTM3U\n#EXT-X-MAP:URI=\"i1.mp4\"\na.m4s\n#EXT-X-MAP:URI=\"i2.mp4\"\nb.m4s\n#EXT-X-ENDLIST\n";
    assert!(beyond_dm(None, None, remapped, "http://h/v/i.m3u8").is_some());
    // The same segment written two ways is one: compared as addresses.
    let rewritten = "#EXTM3U\n#EXT-X-MAP:URI=\"i.mp4\"\na.m4s\n#EXT-X-MAP:URI=\"./i.mp4\"\nb.m4s\n#EXT-X-ENDLIST\n";
    assert_eq!(beyond_dm(None, None, rewritten, "http://h/v/i.m3u8"), None);
    let restated = "#EXTM3U\n#EXT-X-MAP:URI=\"i.mp4\"\na.m4s\n#EXT-X-MAP:URI=\"i.mp4\"\nb.m4s\n#EXT-X-ENDLIST\n";
    let read = media_playlist(restated, "http://h/v/i.m3u8", "x", Quality::Best).unwrap();
    assert_eq!(
        read.parts.len(),
        3,
        "one initialisation segment and two of media"
    );
}

#[test]
fn streams_dm_cannot_assemble_itself_are_named() {
    let plain = "#EXTM3U\n#EXTINF:4,\na.ts\n#EXT-X-ENDLIST\n";
    assert_eq!(beyond_dm(None, None, plain, "http://h/v/i.m3u8"), None);
    let one_key =
        "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\n#EXTINF:4,\na.ts\n#EXT-X-ENDLIST\n";
    assert_eq!(beyond_dm(None, None, one_key, "http://h/v/i.m3u8"), None);
    let restated = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\na.ts\n#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\nb.ts\n#EXT-X-ENDLIST\n";
    assert_eq!(
        beyond_dm(None, None, restated, "http://h/v/i.m3u8"),
        None,
        "the same key said twice"
    );
    let fmp4 = "#EXTM3U\n#EXT-X-MAP:URI=\"i.mp4\"\n#EXTINF:4,\na.m4s\n#EXT-X-ENDLIST\n";
    assert_eq!(beyond_dm(None, None, fmp4, "http://h/v/i.m3u8"), None);
    let remapped = "#EXTM3U\n#EXT-X-MAP:URI=\"i1.mp4\"\na.m4s\n#EXT-X-MAP:URI=\"i2.mp4\"\nb.m4s\n#EXT-X-ENDLIST\n";
    assert!(beyond_dm(None, None, remapped, "http://h/v/i.m3u8")
        .unwrap()
        .contains("initialisation"));
    let restated_map = "#EXTM3U\n#EXT-X-MAP:URI=\"i.mp4\"\na.m4s\n#EXT-X-MAP:URI=\"i.mp4\"\nb.m4s\n#EXT-X-ENDLIST\n";
    assert_eq!(
        beyond_dm(None, None, restated_map, "http://h/v/i.m3u8"),
        None
    );
    let none_then_map =
        "#EXTM3U\n#EXT-X-KEY:METHOD=NONE\n#EXT-X-MAP:URI=\"i.mp4\"\na.m4s\n#EXT-X-ENDLIST\n";
    assert_eq!(
        beyond_dm(None, None, none_then_map, "http://h/v/i.m3u8"),
        None
    );
    let vod = "#EXTM3U\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXTINF:4,\na.ts\n";
    assert_eq!(
        beyond_dm(None, None, vod, "http://h/v/i.m3u8"),
        None,
        "VOD without an end tag"
    );
    // Cut by byte ranges, which dm fetches as ranges.
    let ranged = "#EXTM3U\n#EXT-X-BYTERANGE:100@0\nmain.ts\n#EXT-X-ENDLIST\n";
    assert_eq!(beyond_dm(None, None, ranged, "http://h/v/i.m3u8"), None);
    let ranged_map =
        "#EXTM3U\n#EXT-X-MAP:URI=\"m.mp4\",BYTERANGE=\"720@0\"\nm.mp4\n#EXT-X-ENDLIST\n";
    assert_eq!(beyond_dm(None, None, ranged_map, "http://h/v/i.m3u8"), None);
    // Live is recorded as it goes, not refused.
    let live = "#EXTM3U\n#EXTINF:4,\na.ts\n";
    assert_eq!(beyond_dm(None, None, live, "http://h/v/i.m3u8"), None);
    assert!(is_live(live));
    assert!(!is_live(vod) && !is_live(plain));
    let rotated = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"k1\"\na.ts\n#EXT-X-KEY:METHOD=NONE\nb.ts\n#EXT-X-ENDLIST\n";
    assert!(beyond_dm(None, None, rotated, "http://h/v/i.m3u8")
        .unwrap()
        .contains("encryption"));
    let sealed_init = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\n#EXT-X-MAP:URI=\"i.mp4\"\na.m4s\n#EXT-X-ENDLIST\n";
    assert!(beyond_dm(None, None, sealed_init, "http://h/v/i.m3u8").is_some());
    let separate = "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",DEFAULT=YES,URI=\"audio.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=1,AUDIO=\"a\"\nv.m3u8\n";
    assert!(
        beyond_dm(Some(separate), Some("a"), plain, "http://h/v/i.m3u8")
            .unwrap()
            .contains("sound")
    );
    // Sound in the picture, with a commentary track listed apart.
    let commentary =
        "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"English\",DEFAULT=YES\n\
            #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"Commentary\",URI=\"c.m3u8\"\n\
            #EXT-X-STREAM-INF:BANDWIDTH=1,AUDIO=\"a\"\nv.m3u8\n";
    assert_eq!(
        beyond_dm(Some(commentary), Some("a"), plain, "http://h/v/i.m3u8"),
        None
    );
    // A rendition with no audio group plays its own sound.
    assert_eq!(
        beyond_dm(Some(separate), None, plain, "http://h/v/i.m3u8"),
        None
    );
}

use super::*;

const MASTER: &str = "#EXTM3U\n\
        #EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,CODECS=\"avc1.4d401f,mp4a.40.2\"\n\
        low/index.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080\n\
        high/index.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=64000,CODECS=\"mp4a.40.2\"\n\
        audio/index.m3u8\n";

const MEDIA: &str = "#EXTM3U\n\
        #EXT-X-TARGETDURATION:10\n\
        #EXT-X-MEDIA-SEQUENCE:7\n\
        #EXTINF:9.9,\n\
        seg0.ts\n\
        #EXTINF:9.9,\n\
        seg1.ts\n\
        #EXT-X-ENDLIST\n";

#[test]
fn a_url_or_a_content_type_says_which_kind_of_manifest() {
    assert_eq!(kind_of("https://e.com/a.m3u8", None), Some(Kind::Hls));
    assert_eq!(kind_of("https://e.com/a.m3u8?t=1", None), Some(Kind::Hls));
    assert_eq!(kind_of("https://e.com/a.mpd", None), Some(Kind::Dash));
    assert_eq!(
        kind_of("https://e.com/x", Some("application/vnd.apple.mpegurl")),
        Some(Kind::Hls)
    );
    assert_eq!(kind_of("https://e.com/a.mp4", None), None);
}

#[test]
fn attributes_survive_a_comma_inside_quotes() {
    /* CODECS="avc1.4d401f,mp4a.40.2" is the ordinary case, and splitting
     * the line on commas first reads the codec list as another
     * attribute. */
    let read = attributes(
        "#EXT-X-STREAM-INF:BANDWIDTH=800000,CODECS=\"avc1.4d401f,mp4a.40.2\",RESOLUTION=640x360",
    );
    assert_eq!(
        read,
        vec![
            ("BANDWIDTH".into(), "800000".to_string()),
            ("CODECS".into(), "avc1.4d401f,mp4a.40.2".to_string()),
            ("RESOLUTION".into(), "640x360".to_string()),
        ]
    );
}

#[test]
fn a_master_playlist_lists_what_it_offers() {
    let found = variants(MASTER, "https://e.com/v/master.m3u8");
    assert_eq!(found.len(), 3);
    assert_eq!(found[0].url, "https://e.com/v/low/index.m3u8");
    assert_eq!(found[0].height, 360);
    assert_eq!(found[1].height, 1080);
    assert_eq!(found[2].height, 0, "an audio rendition has no picture");
}

#[test]
fn best_takes_the_tallest() {
    let chosen = pick(variants(MASTER, "https://e.com/v/m.m3u8"), Quality::Best).unwrap();
    assert_eq!(chosen.height, 1080);
}

#[test]
fn a_cap_takes_the_tallest_at_or_below_it() {
    let chosen = pick(
        variants(MASTER, "https://e.com/v/m.m3u8"),
        Quality::UpTo(720),
    )
    .unwrap();
    assert_eq!(chosen.height, 360, "1080 is over the cap");
}

#[test]
fn equal_renditions_are_taken_in_the_order_the_playlist_lists_them() {
    /* A playlist offering one height at two bitrates lists the cheaper
     * one first, and under a cap that is the one to take. `max_by_key`
     * keeps the last of equal candidates and would take the other. */
    let text = "#EXTM3U\n\
            #EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=1280x720\n\
            cheap/index.m3u8\n\
            #EXT-X-STREAM-INF:BANDWIDTH=4000000,RESOLUTION=1280x720\n\
            dear/index.m3u8\n";
    let offered = variants(text, "https://e.com/v/m.m3u8");

    for quality in [Quality::UpTo(1080), Quality::UpTo(720)] {
        assert!(
            pick(offered.clone(), quality)
                .unwrap()
                .url
                .contains("cheap"),
            "{quality:?} took the dearer of two equal renditions"
        );
    }
    // Best does rank by bandwidth, so there it is the dearer one.
    assert!(pick(offered, Quality::Best).unwrap().url.contains("dear"));
}

#[test]
fn a_cap_below_everything_takes_the_shortest_rather_than_the_tallest() {
    // A cap must never quietly fetch something larger than the file the
    // person setting it was trying to avoid.
    let chosen = pick(
        variants(MASTER, "https://e.com/v/m.m3u8"),
        Quality::UpTo(240),
    )
    .unwrap();
    assert_eq!(chosen.height, 360);
}

#[test]
fn audio_takes_the_smallest_stream_with_no_picture() {
    let chosen = pick(variants(MASTER, "https://e.com/v/m.m3u8"), Quality::Audio).unwrap();
    assert_eq!(chosen.bandwidth, 64000);
    assert_eq!(chosen.height, 0);
}

#[test]
fn a_media_playlist_becomes_an_ordered_list_of_segments() {
    let stream = media_playlist(
        MEDIA,
        "https://e.com/v/index.m3u8",
        "https://e.com/v/index.m3u8",
        Quality::Best,
    )
    .unwrap();
    assert_eq!(stream.container, "ts");
    assert_eq!(stream.parts.len(), 2);
    assert_eq!(stream.parts[0].url, "https://e.com/v/seg0.ts");
    assert_eq!(stream.parts[0].sequence, 7, "the playlist starts at 7");
    assert_eq!(stream.parts[1].sequence, 8);
    assert_eq!(stream.filename, "v.ts", "index.m3u8 names nothing");
}

#[test]
fn an_initialisation_segment_leads_the_file() {
    // fMP4 will not decode without it, wherever the playlist mentions it.
    let text = MEDIA.replace(
        "#EXTINF:9.9,\nseg0.ts",
        "#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:9.9,\nseg0.m4s",
    );
    let stream = media_playlist(
        &text,
        "https://e.com/v/i.m3u8",
        "https://e.com/v/i.m3u8",
        Quality::Best,
    )
    .unwrap();
    assert_eq!(stream.container, "mp4");
    assert!(stream.parts[0].init);
    assert_eq!(stream.parts[0].url, "https://e.com/v/init.mp4");
    assert_eq!(stream.parts.len(), 3);
}

#[test]
fn the_initialisation_segment_does_not_shift_the_sequence_numbers() {
    /* The sequence number is the decryption vector for a playlist that
     * declares none, so counting the init segment as a media segment
     * decrypts every one of them to noise. */
    let text = MEDIA.replace(
        "#EXT-X-MEDIA-SEQUENCE:7",
        "#EXT-X-MEDIA-SEQUENCE:7\n#EXT-X-MAP:URI=\"init.mp4\"",
    );
    let stream = media_playlist(
        &text,
        "https://e.com/v/i.m3u8",
        "https://e.com/v/i.m3u8",
        Quality::Best,
    )
    .unwrap();
    let media: Vec<u64> = stream
        .parts
        .iter()
        .filter(|p| !p.init)
        .map(|p| p.sequence)
        .collect();
    assert_eq!(media, [7, 8]);
}

#[test]
fn an_encryption_dm_cannot_undo_fails_loudly() {
    // Saving a file of undecryptable bytes under a .ts name would be
    // worse than refusing.
    let text = MEDIA.replace(
        "#EXT-X-TARGETDURATION:10",
        "#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"k\"",
    );
    let err = media_playlist(
        &text,
        "https://e.com/i.m3u8",
        "https://e.com/i.m3u8",
        Quality::Best,
    )
    .unwrap_err();
    assert!(err.to_string().contains("SAMPLE-AES"), "{err}");
}

#[test]
fn a_key_is_resolved_against_the_playlist() {
    let text = MEDIA.replace(
        "#EXT-X-TARGETDURATION:10",
        "#EXT-X-KEY:METHOD=AES-128,URI=\"../k.bin\",IV=0x0123456789ABCDEF0123456789ABCDEF",
    );
    let stream = media_playlist(
        &text,
        "https://e.com/v/i.m3u8",
        "https://e.com/v/i.m3u8",
        Quality::Best,
    )
    .unwrap();
    let key = stream.key.unwrap();
    assert_eq!(key.uri, "https://e.com/k.bin");
    assert_eq!(
        iv_for(&key, 99),
        [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45, 0x67, 0x89, 0xAB,
            0xCD, 0xEF
        ],
        "a declared IV wins over the sequence number"
    );
}

#[test]
fn a_playlist_with_no_iv_uses_the_sequence_number() {
    let key = Key {
        uri: "https://e.com/k".into(),
        iv: None,
    };
    let mut expected = [0u8; 16];
    expected[8..].copy_from_slice(&7u64.to_be_bytes());
    assert_eq!(iv_for(&key, 7), expected);
}

#[test]
fn a_playlist_listing_nothing_is_refused() {
    assert!(media_playlist(
        "#EXTM3U\n#EXT-X-ENDLIST",
        "https://e.com/i.m3u8",
        "https://e.com/i.m3u8",
        Quality::Best
    )
    .is_err());
}

#[test]
fn the_name_comes_from_the_directory_when_the_playlist_is_called_index() {
    for (url, expected) in [
        ("https://e.com/holiday/index.m3u8", "holiday.ts"),
        ("https://e.com/a/b/master.m3u8", "b.ts"),
        ("https://e.com/the-film.m3u8", "the-film.ts"),
        ("https://e.com/my%20film/index.m3u8", "my film.ts"),
    ] {
        assert_eq!(name_for(url, "ts"), expected, "for {url}");
    }
}

#[test]
fn a_rights_system_key_is_refused_as_copy_protection() {
    for (format, uri) in [
        ("com.apple.streamingkeydelivery", "skd://key"),
        ("", "skd://key"),
        (
            "urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed",
            "data:text/plain;base64,AAAA",
        ),
        ("com.microsoft.playready", "https://lic.example/k"),
    ] {
        assert!(protected_key(format, uri), "{format} {uri}");
    }
    assert!(!protected_key("", "https://e.com/k.key"));
    assert!(!protected_key("identity", "key.bin"));
    let playlist = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"skd://x\",KEYFORMAT=\"com.apple.streamingkeydelivery\"\na.ts\n";
    let refused = media_playlist(
        playlist,
        "https://e.com/v.m3u8",
        "https://e.com/v.m3u8",
        Quality::Best,
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_default();
    assert!(refused.contains("copy-protected"), "{refused}");
}

#[test]
fn a_segment_survives_being_encrypted_and_decrypted() {
    use cbc::cipher::{block_padding::Pkcs7, BlockModeEncrypt, InnerIvInit, KeyInit};

    let key = [7u8; 16];
    let iv = [3u8; 16];
    let plain = b"the quick brown fox jumps over the lazy dog, twice over".to_vec();

    let cipher = aes::Aes128::new_from_slice(&key).unwrap();
    let sealed = cbc::Encryptor::<aes::Aes128>::inner_iv_slice_init(cipher, &iv)
        .unwrap()
        .encrypt_padded_vec::<Pkcs7>(&plain);

    assert_eq!(decrypt(&key, &iv, &sealed).unwrap(), plain);
    // The wrong key must fail rather than produce plausible rubbish.
    assert!(decrypt(&[9u8; 16], &iv, &sealed).is_err());
}

#[test]
fn byte_ranges_name_the_bytes_of_each_segment() {
    let text = "#EXTM3U\n#EXT-X-MAP:URI=\"f.mp4\",BYTERANGE=\"720@0\"\n\
                #EXT-X-BYTERANGE:1000@720\nf.mp4\n#EXT-X-BYTERANGE:500\nf.mp4\n\
                #EXT-X-BYTERANGE:300@5000\nf.mp4\nplain.m4s\n#EXT-X-ENDLIST\n";
    let s = media_playlist(
        text,
        "http://h/v/i.m3u8",
        "http://h/v/i.m3u8",
        Quality::Best,
    )
    .unwrap();
    let ranges: Vec<Option<(u64, u64)>> = s.parts.iter().map(|p| p.range).collect();
    assert_eq!(
        ranges,
        [
            Some((0, 719)),
            Some((720, 1719)),
            // Goes on from where the last piece of the same file stopped.
            Some((1720, 2219)),
            Some((5000, 5299)),
            None
        ]
    );
}

#[test]
fn a_byte_range_dm_cannot_read_is_refused_not_ignored() {
    // Ignored, the segments would be the whole file repeated.
    for bad in ["x", "0", "10@x", "@5", ""] {
        let text = format!("#EXTM3U\n#EXT-X-BYTERANGE:{bad}\nmain.ts\n#EXT-X-ENDLIST\n");
        assert!(
            media_playlist(&text, "http://h/i.m3u8", "http://h/i.m3u8", Quality::Best).is_err(),
            "{bad}"
        );
    }
    // A range with no start, and no piece of that file before it.
    let first = "#EXTM3U\n#EXT-X-BYTERANGE:100\nmain.ts\n#EXT-X-ENDLIST\n";
    assert!(media_playlist(first, "http://h/i.m3u8", "http://h/i.m3u8", Quality::Best).is_err());
}

#[test]
fn a_portrait_playlist_is_picked_by_the_shorter_side() {
    // A Short's renditions: 720p is 720 wide and 1280 tall.
    let master = "#EXTM3U\n\
#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1080x1920\nhi.m3u8\n\
#EXT-X-STREAM-INF:BANDWIDTH=2500000,RESOLUTION=720x1280\nmid.m3u8\n\
#EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=405x720\nlo.m3u8\n";
    let found = variants(master, "https://e.example/master.m3u8");
    assert_eq!(found.iter().map(|v| v.height).collect::<Vec<_>>(), [1080, 720, 405]);
    let chosen = pick(found, Quality::UpTo(720)).unwrap();
    assert!(chosen.url.ends_with("mid.m3u8"), "{}", chosen.url);
}

