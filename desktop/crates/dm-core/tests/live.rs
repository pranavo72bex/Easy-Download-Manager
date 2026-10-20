//! Streams from a media server: an RTMP address fetched through ffmpeg and
//! placed as a file, and the refusals where it cannot be — no ffmpeg, or a
//! proxy it would go around. The server is ffmpeg itself, which can serve
//! one client over RTMP.

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
async fn an_rtmp_stream_is_fetched_by_ffmpeg_and_saved_as_a_file() {
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

    let dir = tempdir();
    let done = download(&client(), &address, &options(dir.path()))
        .await
        .expect("the RTMP stream");
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
        assert!(seconds > 1.5, "{seconds} s of stream saved");
    }
}

#[tokio::test]
async fn without_ffmpeg_a_stream_is_refused_plainly() {
    let _turn = FFMPEG_ENV.lock().await;
    std::env::set_var("DM_FFMPEG", "");
    let dir = tempdir();
    let outcome = download(&client(), "rtmp://127.0.0.1:1/live/x", &options(dir.path())).await;
    std::env::remove_var("DM_FFMPEG");
    let said = outcome.unwrap_err().to_string();
    assert!(said.contains("install ffmpeg"), "{said}");
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
