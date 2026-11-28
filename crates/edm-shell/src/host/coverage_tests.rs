// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// A world with no network and an EDM that is or is not there.
struct Stub {
    running: bool,
    launches: bool,
    rung: usize,
}

impl World for Stub {
    fn app_running(&mut self) -> bool {
        self.running
    }
    fn launch(&mut self) -> std::io::Result<()> {
        if self.launches {
            Ok(())
        } else {
            Err(std::io::Error::other("no such program"))
        }
    }
    fn ring(&mut self) {
        self.rung += 1;
    }
    fn reachable(&mut self, _: &str, _: &str, _: &str, _: u64) -> Result<String, String> {
        Ok("application/zip".into())
    }
    fn formats(&mut self, _: &str, _: &str, _: &str) -> Result<Vec<Choice>, String> {
        Ok(Vec::new())
    }
}

fn stub(running: bool, launches: bool) -> Stub {
    Stub {
        running,
        launches,
        rung: 0,
    }
}

fn reply(home: &Path, frame: serde_json::Value, world: &mut Stub) -> Reply {
    Reply::from_frame(&answer(home, &frame, world).expect("an answer")).unwrap()
}

fn failure(reply: Reply) -> String {
    match reply {
        Reply::Failed { failure, .. } => failure.unwrap().message,
        other => panic!("taken: {other:?}"),
    }
}

fn filed(home: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(inbox(home))
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "json"))
                .collect()
        })
        .unwrap_or_default()
}

fn batch(id: &str) -> serde_json::Value {
    Asked::Batch(Batch {
        id: id.into(),
        page: "https://e.com/list.html".into(),
        urls: vec!["https://e.com/a.zip".into()],
        ..Batch::default()
    })
    .to_frame()
}

#[test]
fn a_pages_links_start_edm_when_none_runs_and_are_taken_back_when_it_cannot_start() {
    let home = tempfile::tempdir().unwrap();
    let mut world = stub(false, true);
    assert_eq!(
        reply(home.path(), batch("links-1"), &mut world),
        Reply::Started {
            id: "links-1".into(),
            shown: true,
            notice: None,
        }
    );
    assert_eq!(filed(home.path()).len(), 1, "left for the EDM started");
    assert_eq!(world.rung, 0, "no EDM was there to ring for");

    // EDM cannot be started: the browser is told, and nothing is left
    // for an EDM started later to download a second time.
    let home = tempfile::tempdir().unwrap();
    let said = failure(reply(
        home.path(),
        batch("links-2"),
        &mut stub(false, false),
    ));
    assert!(said.contains("could not be started"), "{said}");
    assert!(filed(home.path()).is_empty());

    // The very same links already waiting were answered before: they
    // stay where they are.
    let home = tempfile::tempdir().unwrap();
    reply(home.path(), batch("links-3"), &mut stub(true, true));
    let said = failure(reply(
        home.path(),
        batch("links-3"),
        &mut stub(false, false),
    ));
    assert!(said.contains("could not be started"), "{said}");
    assert_eq!(filed(home.path()).len(), 1);
}

#[test]
fn a_hand_off_whose_name_cannot_be_a_file_is_refused_with_why() {
    let home = tempfile::tempdir().unwrap();
    let said = failure(reply(home.path(), batch("../.."), &mut stub(true, true)));
    assert!(said.starts_with("EDM could not take it"), "{said}");
    let start = Asked::Start(Start {
        id: "...".into(),
        url: "https://e.com/f.zip".into(),
        ..Start::default()
    })
    .to_frame();
    let said = failure(reply(home.path(), start, &mut stub(true, true)));
    assert!(said.starts_with("EDM could not take it"), "{said}");
    assert!(filed(home.path()).is_empty());
}

#[test]
fn a_caught_download_stays_with_the_browser_when_the_download_folder_cannot_be_made() {
    let home = tempfile::tempdir().unwrap();
    let file = home.path().join("a file");
    std::fs::write(&file, b"").unwrap();
    std::fs::write(
        home.path().join("settings.json"),
        serde_json::json!({ "download_dir": file.join("downloads") }).to_string(),
    )
    .unwrap();
    let start = Asked::Start(Start {
        id: "caught-1".into(),
        url: "https://e.com/f.zip".into(),
        caught: true,
        ..Start::default()
    })
    .to_frame();
    let said = failure(reply(home.path(), start, &mut stub(true, true)));
    assert!(
        said.contains("may not save into its download folder"),
        "{said}"
    );
    assert!(filed(home.path()).is_empty());
}

#[test]
fn a_thousand_requests_under_one_name_are_not_written_over() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("same.json"), b"first").unwrap();
    for n in 2..=1000 {
        std::fs::write(dir.path().join(format!("same-{n}.json")), b"other").unwrap();
    }
    let refused = file_new(dir.path(), "same", b"new").unwrap_err();
    assert_eq!(refused.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(
        std::fs::read(dir.path().join("same.json")).unwrap(),
        b"first"
    );
    let left = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .count();
    assert_eq!(left, 0, "the staged copy was left behind");
}

#[test]
fn the_real_world_knows_whether_edm_runs_and_says_why_it_cannot_tell() {
    let home = tempfile::tempdir().unwrap();
    let mut here = Here {
        home: home.path(),
        started: false,
    };
    assert!(!here.app_running(), "no EDM holds this home's lock");
    here.started = true;
    assert!(here.app_running(), "the one this host started counts");

    // A home that is a file has no lock to read: no EDM is started, and
    // the browser is told why.
    let file = home.path().join("not a folder");
    std::fs::write(&file, b"").unwrap();
    let mut here = Here {
        home: &file,
        started: false,
    };
    assert!(!here.app_running());
    assert!(here.launch().is_err());
    assert!(!here.started);
}

#[test]
fn a_caught_download_edm_can_fetch_is_filed_and_a_quality_list_is_passed_on() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join("settings.json"),
        serde_json::json!({ "download_dir": home.path().join("out") }).to_string(),
    )
    .unwrap();
    let mut world = stub(true, true);
    let start = Asked::Start(Start {
        id: "caught-2".into(),
        url: "https://e.com/f.zip".into(),
        caught: true,
        mime: "application/zip".into(),
        ..Start::default()
    })
    .to_frame();
    assert_eq!(
        reply(home.path(), start, &mut world),
        Reply::Started {
            id: "caught-2".into(),
            shown: true,
            notice: None,
        }
    );
    assert_eq!(filed(home.path()).len(), 1);
    assert_eq!(world.rung, 1, "the running EDM was told");

    let asked = Asked::Formats(FormatsAsked {
        url: " https://e.com/watch ".into(),
        ..FormatsAsked::default()
    })
    .to_frame();
    assert_eq!(
        reply(home.path(), asked, &mut world),
        Reply::Formats {
            choices: Vec::new(),
            reason: None
        }
    );
}

/// The program started is this test's own binary with `--desktop`, which
/// the test harness refuses and leaves at once: nothing is opened.
#[test]
fn the_real_world_starts_edm_detached_from_the_browser_and_remembers_it_did() {
    let home = tempfile::tempdir().unwrap();
    let mut here = Here {
        home: home.path(),
        started: false,
    };
    here.launch().unwrap();
    assert!(here.started);
    assert!(here.app_running());
}

#[test]
fn the_real_world_rings_a_running_dms_bell_and_reads_the_clock() {
    let home = tempfile::tempdir().unwrap();
    let bell = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    bell.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    std::fs::create_dir_all(inbox(home.path())).unwrap();
    std::fs::write(
        inbox(home.path()).join(BELL),
        bell.local_addr().unwrap().port().to_string(),
    )
    .unwrap();
    let mut here = Here {
        home: home.path(),
        started: false,
    };
    here.ring();
    let mut buf = [0u8; 8];
    let (got, _) = bell.recv_from(&mut buf).expect("the bell rang");
    assert_eq!(got, 0, "a ring carries nothing");

    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let now = here.now_ms();
    assert!(now >= before && now < before + 60_000, "{now} {before}");
}

/// A server on this computer that answers every request with `answer`,
/// or holds every connection open and says nothing.
fn server(answer: Option<&'static str>) -> std::net::SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut asked = Vec::new();
            let mut buf = [0u8; 1024];
            while !asked.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => asked.extend_from_slice(&buf[..n]),
                }
            }
            match answer {
                Some(text) => {
                    let _ = stream.write_all(text.as_bytes());
                }
                None => held.push(stream),
            }
        }
    });
    addr
}

#[test]
fn the_real_world_asks_the_server_what_it_sends() {
    let home = tempfile::tempdir().unwrap();
    let mut here = Here {
        home: home.path(),
        started: false,
    };
    let addr = server(Some(
        "HTTP/1.1 206 Partial Content\r\ncontent-type: video/mp4\r\n\
         content-range: bytes 0-0/100\r\ncontent-length: 1\r\nconnection: close\r\n\r\nx",
    ));
    let served = here.reachable_with(
        &format!("http://{addr}/film.mp4"),
        "http://page.example/watch",
        "sid=1",
        &[("Accept-Language".into(), "en".into())],
        5000,
    );
    assert_eq!(served, Ok("video/mp4".into()));

    // A server that never answers is given up on in the time allowed.
    let silent = server(None);
    let said = here
        .reachable(&format!("http://{silent}/f.zip"), "", "", 300)
        .unwrap_err();
    assert_eq!(said, "the server did not answer EDM within 1 seconds");

    // Nothing listening: the probe's own error.
    let gone = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = gone.local_addr().unwrap().port();
    drop(gone);
    assert!(here
        .reachable(&format!("http://127.0.0.1:{port}/f.zip"), "", "", 5000)
        .is_err());
}

#[test]
fn the_real_world_lists_qualities_from_a_reading_kept_or_none_for_a_site_without_them() {
    let home = tempfile::tempdir().unwrap();
    let mut here = Here {
        home: home.path(),
        started: false,
    };
    assert_eq!(
        here.formats("magnet:?xt=urn:btih:abc", "", ""),
        Err("only a web page has qualities to list".into())
    );
    // Read a moment ago: given again without asking anyone.
    let page = "https://video.example/watch/1";
    let kept = Choice {
        label: "MP4 1280×720".into(),
        quality: "720".into(),
        size: Some(10),
    };
    remember_formats(
        home.path(),
        &formats_key(page, "sid=1"),
        std::slice::from_ref(&kept),
        now_secs(),
    );
    assert_eq!(here.formats(page, "", "sid=1"), Ok(vec![kept]));
    // A page of a site EDM reads no qualities from: none, and nothing is
    // asked of the network (nothing listens on port 9 here).
    assert_eq!(
        here.formats("http://127.0.0.1:9/page", "http://127.0.0.1:9/", ""),
        Ok(Vec::new())
    );
    assert!(
        recalled_formats(
            home.path(),
            &formats_key("http://127.0.0.1:9/page", ""),
            now_secs()
        )
        .is_none(),
        "an empty reading is not kept"
    );
}

/// The formats cache stays a bare list, read in either shape, and the extension's file carries
/// its format number (gaps row A8).
#[test]
fn the_formats_cache_stays_a_list_and_the_extension_file_carries_its_format() {
    let home = tempfile::tempdir().unwrap();
    let key = formats_key("https://video.example/watch/2", "");
    let kept = Choice {
        label: "MP4 640×360".into(),
        quality: "360".into(),
        size: None,
    };
    remember_formats(home.path(), &key, std::slice::from_ref(&kept), 1000);
    let written = std::fs::read(formats_file(home.path())).unwrap();
    assert_eq!(
        written.first(),
        Some(&b'['),
        "a bare list, as an older EDM reads it"
    );
    assert_eq!(
        recalled_formats(home.path(), &key, 1001),
        Some(vec![kept.clone()])
    );
    // The object shape an early build of this change wrote.
    let rows = serde_json::to_value([Remembered {
        key: key.clone(),
        at: 1000,
        choices: vec![kept.clone()],
    }])
    .unwrap();
    let object = serde_json::json!({ "format": 1, "rows": rows }).to_string();
    std::fs::write(formats_file(home.path()), object).unwrap();
    assert_eq!(recalled_formats(home.path(), &key, 1001), Some(vec![kept]));

    let greeting = edm_protocol::Greeting::default();
    note_extension(home.path(), &greeting);
    let written = std::fs::read(extension_file(home.path())).unwrap();
    assert_eq!(edm_core::saved::format_of(&written), EXTENSION_FORMAT);
    assert_eq!(extension_heard(home.path()), Some(greeting));
}

#[test]
fn the_warm_reading_does_nothing() {
    let home = tempfile::tempdir().unwrap();
    warm_main(home.path());
    assert!(std::fs::read_dir(home.path()).unwrap().next().is_none());
}

/// The host as the browser starts it: its frames answered in the real world, in turn, until
/// the browser's end closes.
#[test]
fn the_host_in_the_real_world_answers_the_browser_s_hello() {
    let home = tempfile::tempdir().unwrap();
    let mut input = frame(&serde_json::json!({
        "protocolVersion": 1, "type": "engine.hello", "extensionVersion": "3.5.0"
    }));
    input.extend(frame(
        &serde_json::json!({ "protocolVersion": 99, "type": "engine.hello" }),
    ));
    let mut output = Vec::new();
    serve_here(home.path(), &mut input.as_slice(), &mut output);
    let mut answers = output.as_slice();
    assert_eq!(read_frame(&mut answers).unwrap()["type"], "engine.hello");
    assert!(
        read_frame(&mut answers).is_none(),
        "a version EDM does not speak got an answer"
    );
}

/// Started in Safari's sandbox, EDM's window is opened through the system: `open` that
/// succeeds is a start, one that fails or is missing is the browser's to hear.
#[cfg(unix)]
#[test]
fn the_window_opened_through_the_system_says_when_it_could_not_be() {
    let app = Path::new("/Applications/EDM.app");
    assert!(crate::platform::programs::open_app_through(Path::new("true"), app).is_ok());
    let refused = crate::platform::programs::open_app_through(Path::new("false"), app).unwrap_err();
    assert!(
        refused
            .to_string()
            .starts_with("macOS could not open /Applications/EDM.app"),
        "{refused}"
    );
    assert!(crate::platform::programs::open_app_through(Path::new("/nowhere/open"), app).is_err());
}

/// A stand-in for YouTube's watch page (`EDM_YOUTUBE_HOST`): its player data lists a 720p
/// picture and the sound, whatever is asked.
fn watch_page_stand_in() -> String {
    use std::io::{BufRead, BufReader};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    // Each stream with its own address, as YouTube's are: one with neither an address nor a
    // cipher is a framed stream's (gaps row Y2), dropped where the answer names none.
    let video = format!("{origin}/video.mp4");
    let audio = format!("{origin}/audio.m4a");
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(&stream);
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let player = serde_json::json!({
                "playabilityStatus": {"status": "OK"},
                "videoDetails": {"videoId": "standin0000", "isLive": false},
                "streamingData": {"adaptiveFormats": [
                    {"itag": 136, "mimeType": "video/mp4; codecs=\"avc1.4d401f\"", "width": 1280,
                     "height": 720, "fps": 30, "contentLength": "2000", "url": video},
                    {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"",
                     "averageBitrate": 130000, "contentLength": "500", "url": audio}
                ]}
            });
            let body = format!(
                "<html><script>var ytInitialPlayerResponse = {player};var meta = 1;</script></html>"
            );
            let _ = (&stream).write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            );
        }
    });
    origin
}

#[test]
fn the_real_world_lists_a_youtube_video_s_qualities_from_its_page_and_keeps_them() {
    std::env::set_var("EDM_YOUTUBE_HOST", watch_page_stand_in());
    let home = tempfile::tempdir().unwrap();
    let mut here = Here {
        home: home.path(),
        started: false,
    };
    let page = "https://www.youtube.com/watch?v=standin0000";
    let listed = here.formats(page, "", "").unwrap();
    assert!(
        listed.iter().any(|c| c.quality.starts_with("720")),
        "{listed:?}"
    );
    assert!(
        listed.iter().any(|c| c.quality == "audio/m4a"),
        "{listed:?}"
    );
    assert_eq!(
        recalled_formats(home.path(), &formats_key(page, ""), now_secs()),
        Some(listed),
        "the reading is kept for the next ask"
    );
}
