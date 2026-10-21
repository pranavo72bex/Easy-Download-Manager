//! The muxer against real encoder output: H.264 with B-frames and AAC,
//! fragmented as DASH serves them and plain, made by an ffmpeg on this
//! computer and judged by it. Joined, every decoded picture and sound must
//! match the inputs'. Skipped where there is no ffmpeg; dm itself never
//! needs one to join.

use std::path::{Path, PathBuf};
use std::process::Command;

fn tool(name: &str) -> Option<PathBuf> {
    let found = Command::new(name).arg("-version").output().ok()?;
    found.status.success().then(|| PathBuf::from(name))
}

fn run(program: &Path, args: &[&str]) -> String {
    let out = Command::new(program).args(args).output().unwrap();
    assert!(
        out.status.success(),
        "{} {args:?}: {}",
        program.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The MD5 of a stream's decoded frames.
fn decoded(ffmpeg: &Path, file: &Path, stream: &str) -> String {
    run(
        ffmpeg,
        &[
            "-v",
            "error",
            "-i",
            file.to_str().unwrap(),
            "-map",
            stream,
            "-fps_mode",
            "passthrough",
            "-f",
            "md5",
            "-",
        ],
    )
}

/// Picture and sound decode as the inputs' do, or as ffmpeg's own
/// `-c copy` join of them does. The two differ in the sound's last frame
/// from ffmpeg 9 on: a DASH sound's edit list leaves its end open, and a
/// joined file says where it ends, so the encoder's padding after the end
/// is played from the input and not from either join.
fn same_as_ffmpeg(ffmpeg: &Path, out: &Path, inputs: &[&Path]) {
    let mut name = std::ffi::OsString::from("ffmpeg-joined.");
    name.push(out.extension().unwrap());
    let reference = out.with_file_name(name);
    let mut args: Vec<&str> = vec!["-v", "error", "-y"];
    for input in inputs {
        args.extend(["-i", input.to_str().unwrap()]);
    }
    for n in ["0", "1"].iter().take(inputs.len()) {
        args.extend(["-map", n]);
    }
    args.extend(["-c", "copy", reference.to_str().unwrap()]);
    run(ffmpeg, &args);
    for (stream, source) in [("0:v", inputs[0]), ("0:a", *inputs.last().unwrap())] {
        let ours = decoded(ffmpeg, out, stream);
        let theirs = decoded(ffmpeg, source, stream);
        let joined = decoded(ffmpeg, &reference, stream);
        assert!(
            ours == theirs || ours == joined,
            "{stream}: ours {ours} / the input's {theirs} / ffmpeg's join {joined}"
        );
    }
}

fn make(ffmpeg: &Path, dir: &Path, fragmented: bool) -> (PathBuf, PathBuf) {
    let flags = if fragmented {
        // As a DASH server's files are: an empty index, a segment index,
        // fragments starting at each key frame.
        "+dash+global_sidx"
    } else {
        "+faststart"
    };
    let video = dir.join(if fragmented { "v.f137.mp4" } else { "v.mp4" });
    let audio = dir.join(if fragmented { "a.f140.m4a" } else { "a.m4a" });
    run(
        ffmpeg,
        &[
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=duration=6:size=320x180:rate=30",
            "-c:v",
            "libx264",
            "-g",
            "30",
            "-bf",
            "2",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            flags,
            video.to_str().unwrap(),
        ],
    );
    run(
        ffmpeg,
        &[
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=6:sample_rate=44100",
            "-c:a",
            "aac",
            "-b:a",
            "96k",
            "-movflags",
            flags,
            audio.to_str().unwrap(),
        ],
    );
    (video, audio)
}

fn check(fragmented: bool) {
    let (Some(ffmpeg), Some(ffprobe)) = (tool("ffmpeg"), tool("ffprobe")) else {
        eprintln!("no ffmpeg here to make and judge real files; skipped");
        return;
    };
    let encoders = run(&ffmpeg, &["-v", "error", "-encoders"]);
    if !encoders.contains("libx264") {
        eprintln!("this ffmpeg has no H.264 encoder to make real files with; skipped");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (video, audio) = make(&ffmpeg, dir.path(), fragmented);
    let out = dir.path().join("joined.mp4");
    let started = std::time::Instant::now();
    dm_core::mp4mux::join(&[video.clone(), audio.clone()], &out).unwrap();
    eprintln!("joined in {:?}", started.elapsed());

    // Decodes from end to end with nothing said.
    let said = Command::new(&ffmpeg)
        .args([
            "-v",
            "error",
            "-i",
            out.to_str().unwrap(),
            "-f",
            "null",
            "-",
        ])
        .output()
        .unwrap();
    assert!(
        said.status.success() && said.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&said.stderr)
    );

    let streams = run(
        &ffprobe,
        &[
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type,duration",
            "-of",
            "csv=p=0",
            out.to_str().unwrap(),
        ],
    );
    // Picture and sound start together, as a join by ffmpeg itself has
    // them: a DASH picture's first frame is shown two frames in.
    let starts = run(
        &ffprobe,
        &[
            "-v",
            "error",
            "-show_entries",
            "stream=start_time",
            "-of",
            "csv=p=0",
            out.to_str().unwrap(),
        ],
    );
    for start in starts.lines() {
        assert!(start.parse::<f64>().unwrap().abs() < 0.001, "{starts}");
    }
    let kinds: Vec<&str> = streams
        .lines()
        .map(|l| l.split(',').next().unwrap())
        .collect();
    assert_eq!(kinds, ["video", "audio"], "{streams}");
    for line in streams.lines() {
        let seconds: f64 = line.split(',').nth(1).unwrap().parse().unwrap();
        assert!((seconds - 6.0).abs() < 0.2, "{streams}");
    }

    same_as_ffmpeg(&ffmpeg, &out, &[&video, &audio]);
}

#[test]
fn a_dash_picture_and_sound_join_into_one_mp4_that_decodes_the_same() {
    check(true);
}

#[test]
fn plain_files_join_the_same_way() {
    check(false);
}

/// An HLS stream as dm gathers it: its `.ts` segments one after another.
fn transport_stream(ffmpeg: &Path, dir: &Path, segmented: bool) -> PathBuf {
    let ts = dir.join("joined.ts");
    let source = [
        "-v",
        "error",
        "-y",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=duration=6:size=320x180:rate=30",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:duration=6:sample_rate=44100",
        "-c:v",
        "libx264",
        "-g",
        "30",
        "-bf",
        "2",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-b:a",
        "96k",
    ];
    if segmented {
        let playlist = dir.join("index.m3u8");
        let mut args = source.to_vec();
        args.extend(["-f", "hls", "-hls_time", "2", "-hls_list_size", "0"]);
        args.push(playlist.to_str().unwrap());
        run(ffmpeg, &args);
        let mut segments: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "ts"))
            .collect();
        segments.sort();
        assert!(segments.len() >= 3, "{segments:?}");
        let joined: Vec<u8> = segments
            .iter()
            .flat_map(|s| std::fs::read(s).unwrap())
            .collect();
        std::fs::write(&ts, joined).unwrap();
    } else {
        let mut args = source.to_vec();
        args.extend(["-f", "mpegts"]);
        args.push(ts.to_str().unwrap());
        run(ffmpeg, &args);
    }
    ts
}

fn check_ts(segmented: bool) {
    let (Some(ffmpeg), Some(ffprobe)) = (tool("ffmpeg"), tool("ffprobe")) else {
        eprintln!("no ffmpeg here to make and judge real files; skipped");
        return;
    };
    if !run(&ffmpeg, &["-v", "error", "-encoders"]).contains("libx264") {
        eprintln!("this ffmpeg has no H.264 encoder to make real files with; skipped");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let ts = transport_stream(&ffmpeg, dir.path(), segmented);
    let out = dir.path().join("joined.mp4");
    let started = std::time::Instant::now();
    dm_core::tsmux::to_mp4(&ts, &out).unwrap();
    eprintln!("remuxed in {:?}", started.elapsed());
    let left: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("joined.mp4."))
        .collect();
    assert!(left.is_empty(), "scratch files left: {left:?}");

    let said = Command::new(&ffmpeg)
        .args([
            "-v",
            "error",
            "-i",
            out.to_str().unwrap(),
            "-f",
            "null",
            "-",
        ])
        .output()
        .unwrap();
    assert!(
        said.status.success() && said.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&said.stderr)
    );
    let streams = run(
        &ffprobe,
        &[
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type,width,height,start_time,duration",
            "-of",
            "csv=p=0",
            out.to_str().unwrap(),
        ],
    );
    eprintln!("{streams}");
    assert!(streams.contains("video,320,180,"), "{streams}");
    assert_eq!(streams.lines().count(), 2, "{streams}");
    let source = run(
        &ffprobe,
        &[
            "-v",
            "error",
            "-show_entries",
            "stream=start_time",
            "-of",
            "csv=p=0",
            ts.to_str().unwrap(),
        ],
    );
    let starts: Vec<f64> = source
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim().parse().unwrap())
        .collect();
    eprintln!("source starts {starts:?}");
    let gap = starts[0] - starts[1];
    let ours: Vec<f64> = streams
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split(',').rev().nth(1).unwrap().parse().unwrap())
        .collect();
    // Picture and sound keep the distance apart they had in the stream.
    assert!(
        ((ours[0] - ours[1]) - gap).abs() < 0.002,
        "{source} / {streams}"
    );

    same_as_ffmpeg(&ffmpeg, &out, &[&ts]);
}

#[test]
fn a_transport_stream_becomes_an_mp4_that_decodes_the_same() {
    check_ts(false);
}

#[test]
fn an_hls_streams_segments_become_one_mp4_that_decodes_the_same() {
    check_ts(true);
}

/// WebM, as a video site serves its VP9 and Opus qualities: picture and
/// sound apart, joined by dm into one WebM that decodes the same and seeks.
#[test]
fn a_webm_picture_and_sound_join_into_one_webm_that_decodes_the_same() {
    let (Some(ffmpeg), Some(ffprobe)) = (tool("ffmpeg"), tool("ffprobe")) else {
        eprintln!("no ffmpeg here to make and judge real files; skipped");
        return;
    };
    let encoders = run(&ffmpeg, &["-v", "error", "-encoders"]);
    if !encoders.contains("libvpx-vp9") || !encoders.contains("libopus") {
        eprintln!("this ffmpeg has no VP9 or Opus encoder; skipped");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("v.f248.webm");
    let audio = dir.path().join("a.f251.webm");
    run(
        &ffmpeg,
        &[
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=duration=6:size=320x180:rate=30",
            "-c:v",
            "libvpx-vp9",
            "-deadline",
            "realtime",
            "-g",
            "30",
            "-dash",
            "1",
            "-f",
            "webm",
            video.to_str().unwrap(),
        ],
    );
    run(
        &ffmpeg,
        &[
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=6:sample_rate=48000",
            "-c:a",
            "libopus",
            "-dash",
            "1",
            "-f",
            "webm",
            audio.to_str().unwrap(),
        ],
    );
    let out = dir.path().join("joined.webm");
    let started = std::time::Instant::now();
    dm_core::mkvmux::join(&[video.clone(), audio.clone()], &out).unwrap();
    eprintln!("joined in {:?}", started.elapsed());
    let said = Command::new(&ffmpeg)
        .args([
            "-v",
            "error",
            "-i",
            out.to_str().unwrap(),
            "-f",
            "null",
            "-",
        ])
        .output()
        .unwrap();
    assert!(
        said.status.success() && said.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&said.stderr)
    );
    let streams = run(
        &ffprobe,
        &[
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name,start_time:format=format_name,duration",
            "-of",
            "csv=p=0",
            out.to_str().unwrap(),
        ],
    );
    eprintln!("{streams}");
    assert!(
        streams.contains("vp9") && streams.contains("opus"),
        "{streams}"
    );
    assert!(streams.contains("matroska,webm"), "{streams}");
    same_as_ffmpeg(&ffmpeg, &out, &[&video, &audio]);
    // It seeks: a decode from four seconds in starts there, by its index.
    let seeked = run(
        &ffprobe,
        &[
            "-v",
            "error",
            "-read_intervals",
            "4%+0.1",
            "-select_streams",
            "v",
            "-show_entries",
            "frame=pts_time",
            "-of",
            "csv=p=0",
            out.to_str().unwrap(),
        ],
    );
    let first: f64 = seeked.lines().next().unwrap().trim().parse().unwrap();
    assert!((3.0..=4.1).contains(&first), "{seeked}");
}

const VTT_EN: &str = "WEBVTT\n\n00:00:01.000 --> 00:00:02.500\nHello <i>there</i>\n\n00:00:03.000 --> 00:00:04.000\nA &amp; B\n";
const VTT_DE: &str = "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nHallo\n";

/// The cues ffmpeg reads back from a file's first and second subtitle
/// tracks, as SRT, and the tracks' languages.
fn subtitles_read_back(ffmpeg: &Path, ffprobe: &Path, file: &Path) -> (String, String, String) {
    // Without the font tags ffmpeg writes around an MP4's text.
    let srt = |n: &str| {
        let text = run(
            ffmpeg,
            &[
                "-v",
                "error",
                // The file's own times, not shifted to start at zero.
                "-copyts",
                "-i",
                file.to_str().unwrap(),
                "-map",
                n,
                "-f",
                "srt",
                "-",
            ],
        );
        let mut plain = String::new();
        let mut in_tag = false;
        for c in text.chars() {
            match c {
                '<' => in_tag = true,
                '>' if in_tag => in_tag = false,
                _ if !in_tag => plain.push(c),
                _ => {}
            }
        }
        plain
    };
    let languages = run(
        ffprobe,
        &[
            "-v",
            "error",
            "-select_streams",
            "s",
            "-show_entries",
            "stream=codec_name:stream_tags=language",
            "-of",
            "csv=p=0",
            file.to_str().unwrap(),
        ],
    );
    (srt("0:s:0"), srt("0:s:1"), languages)
}

#[test]
fn subtitles_are_carried_in_the_mp4_and_the_webm_dm_joins() {
    let (Some(ffmpeg), Some(ffprobe)) = (tool("ffmpeg"), tool("ffprobe")) else {
        eprintln!("no ffmpeg here to judge real files; skipped");
        return;
    };
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let dir = tempfile::tempdir().unwrap();
    let texts = vec![
        dm_core::subtitles::Subtitle {
            language: "de".into(),
            cues: dm_core::subtitles::parse_vtt(VTT_DE),
        },
        dm_core::subtitles::Subtitle {
            language: "en".into(),
            cues: dm_core::subtitles::parse_vtt(VTT_EN),
        },
    ];
    let mp4 = dir.path().join("joined.mp4");
    dm_core::mp4mux::join_with_text(
        &[
            fixtures.join("mp4/picture.f137.mp4"),
            fixtures.join("mp4/sound.f140.m4a"),
        ],
        &texts,
        &mp4,
    )
    .unwrap();
    let (de, en, languages) = subtitles_read_back(&ffmpeg, &ffprobe, &mp4);
    eprintln!("{languages}{de}{en}");
    assert_eq!(languages.trim(), "mov_text,deu\nmov_text,eng");
    assert!(de.contains("00:00:01,000 --> 00:00:02,000\nHallo"), "{de}");
    assert!(
        en.contains("00:00:01,000 --> 00:00:02,500\nHello there"),
        "{en}"
    );
    assert!(en.contains("00:00:03,000 --> 00:00:04,000\nA & B"), "{en}");

    let webm = dir.path().join("joined.webm");
    dm_core::mkvmux::join_with_text(
        &[
            fixtures.join("webm/picture.f247.webm"),
            fixtures.join("webm/sound.f251.webm"),
        ],
        &texts,
        &webm,
    )
    .unwrap();
    let (de, en, languages) = subtitles_read_back(&ffmpeg, &ffprobe, &webm);
    eprintln!("{languages}{de}{en}");
    assert_eq!(languages.trim(), "webvtt,deu\nwebvtt,eng");
    assert!(de.contains("00:00:01,000 --> 00:00:02,000\nHallo"), "{de}");
    assert!(en.contains("00:00:01,000 --> 00:00:02,500"), "{en}");
    assert!(en.contains("00:00:03,000 --> 00:00:04,000\nA & B"), "{en}");
    // Both still decode whole.
    for file in [&mp4, &webm] {
        let said = Command::new(&ffmpeg)
            .args([
                "-v",
                "error",
                "-i",
                file.to_str().unwrap(),
                "-f",
                "null",
                "-",
            ])
            .output()
            .unwrap();
        assert!(
            said.status.success() && said.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&said.stderr)
        );
    }
}

/// A transport stream of `video` (an ffmpeg encoder) and `audio`, put into
/// an MP4 by dm: it decodes as the stream does, picture and sound.
fn check_ts_kind(video: &str, audio: &str) {
    let Some(ffmpeg) = tool("ffmpeg") else {
        eprintln!("no ffmpeg here to make and judge real files; skipped");
        return;
    };
    let encoders = run(&ffmpeg, &["-v", "error", "-encoders"]);
    if !encoders.contains(video) || !encoders.contains(audio) {
        eprintln!("this ffmpeg has no {video} or {audio} encoder; skipped");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let ts = dir.path().join("joined.ts");
    run(
        &ffmpeg,
        &[
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=duration=4:size=320x180:rate=25",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=4:sample_rate=48000",
            "-c:v",
            video,
            "-g",
            "25",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            audio,
            "-ac",
            "2",
            "-f",
            "mpegts",
            ts.to_str().unwrap(),
        ],
    );
    let out = dir.path().join("joined.mp4");
    dm_core::tsmux::to_mp4(&ts, &out).unwrap_or_else(|e| panic!("{video}/{audio}: {e}"));
    let said = Command::new(&ffmpeg)
        .args([
            "-v",
            "error",
            "-i",
            out.to_str().unwrap(),
            "-f",
            "null",
            "-",
        ])
        .output()
        .unwrap();
    assert!(
        said.status.success() && said.stderr.is_empty(),
        "{video}/{audio}: {}",
        String::from_utf8_lossy(&said.stderr)
    );
    same_as_ffmpeg(&ffmpeg, &out, &[&ts]);
}

#[test]
fn a_transport_stream_with_mp3_sound_becomes_an_mp4() {
    check_ts_kind("libx264", "libmp3lame");
}

#[test]
fn a_transport_stream_with_mp2_sound_becomes_an_mp4() {
    check_ts_kind("libx264", "mp2");
}

#[test]
fn a_transport_stream_with_ac3_sound_becomes_an_mp4() {
    check_ts_kind("libx264", "ac3");
}

#[test]
fn a_transport_stream_with_hevc_pictures_becomes_an_mp4() {
    check_ts_kind("libx265", "aac");
}

#[test]
fn a_transport_stream_with_eac3_sound_becomes_an_mp4() {
    check_ts_kind("libx264", "eac3");
}
