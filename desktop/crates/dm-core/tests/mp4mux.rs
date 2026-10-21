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

    assert_eq!(
        decoded(&ffmpeg, &out, "0:v"),
        decoded(&ffmpeg, &video, "0:v")
    );
    assert_eq!(
        decoded(&ffmpeg, &out, "0:a"),
        decoded(&ffmpeg, &audio, "0:a")
    );
}

#[test]
fn a_dash_picture_and_sound_join_into_one_mp4_that_decodes_the_same() {
    check(true);
}

#[test]
fn plain_files_join_the_same_way() {
    check(false);
}
