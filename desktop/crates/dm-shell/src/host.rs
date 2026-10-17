//! Taking a download from the browser.
//!
//! The browser launches this as a native messaging host: a short-lived
//! process it owns and kills when the port closes. So it holds nothing worth
//! losing — it writes the request into a directory the running app watches,
//! says it did, and exits.
//!
//! Deliberately one-way. The browser extension does not draw progress bars;
//! it catches the click and the application shows what happened. Relaying
//! progress back through a process the browser can kill at any moment buys a
//! second copy of a queue that is already on screen.

// Reads outside input: an index out of range here is a crash a server or
// a file could cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use dm_protocol::{
    Batch, CatchRules, Choice, FormatsAsked, Hello, Reply, Request as Asked, SettingsReply, Start,
};

/// A request the app will pick up.
#[derive(serde::Serialize, serde::Deserialize, Debug, Default, PartialEq)]
pub struct Request {
    pub url: String,
    /// The page it came from. Kept because a media URL alone is often not
    /// something anything can fetch twice.
    #[serde(default)]
    pub referrer: String,
    /// `--save-to`: the folder to save in, from the command line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_dir: Option<String>,
    /// `--name`: the name to save as.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_name: Option<String>,
    /// `--later`: into the main queue, not started.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub later: bool,
    /// The quality picked in the browser's video panel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    /// What the panel's list called that quality.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Open "Download details" for it rather than starting it, as with a
    /// video from the browser's panel.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ask: bool,
    /// A page's links, for the app's "Download all links" dialog.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
    /// The browser's cookies for the site, as a `Cookie` header, where the
    /// person switched that on for the site in the toolbar popup.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cookies: String,
}

/// Frames one message the way native messaging wants it.
pub fn frame(value: &serde_json::Value) -> Vec<u8> {
    let body = value.to_string().into_bytes();
    let mut out = (body.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(&body);
    out
}

/// Reads one message, or `None` when the browser has gone.
///
/// A length the browser would never send means the stream has lost its place;
/// reading it would allocate until the process died, so it stops instead.
///
/// A whole frame that is not JSON is `Null`, not the end: the frame was read
/// in full, so the stream has not lost its place, and ending there left the
/// browser's every later message on that port unanswered.
pub fn read_frame(input: &mut impl Read) -> Option<serde_json::Value> {
    let mut header = [0u8; 4];
    input.read_exact(&mut header).ok()?;
    let len = u32::from_le_bytes(header) as usize;
    if len > 64 * 1024 * 1024 {
        return None;
    }

    let mut body = vec![0u8; len];
    input.read_exact(&mut body).ok()?;
    Some(serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null))
}

/// Where the app looks for work handed over by a browser.
pub fn inbox(home: &Path) -> PathBuf {
    home.join("inbox")
}

/// Writes a request where the app will find it.
///
/// Whole then moved, so the app cannot read a half-written file — it polls a
/// directory, and a partial JSON there would be a download silently dropped.
///
/// The id comes from the browser and is used as a filename, so it is held to
/// being a plain name: `../../../.bashrc` is a perfectly well-formed id, and
/// anything that writes where it is told by a message is a program that
/// writes wherever the sender likes.
pub fn deposit(home: &Path, id: &str, request: &Request) -> std::io::Result<()> {
    let named: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .take(64)
        .collect();
    if named.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "that is not a name a request can be filed under",
        ));
    }

    let path = inbox(home).join(format!("{named}.json"));
    dm_core::write_atomic(&path, serde_json::to_string(request)?.as_bytes())
}

/// Starts the normal desktop process so an app that is closed can consume
/// the inbox. Native messaging launches this binary in host mode, which must
/// not open a window or write anything except framed replies to stdout.
///
/// A failure comes back: the browser is about to be told the download has
/// started, and with no app to consume the inbox it has not.
fn launch_desktop() -> std::io::Result<()> {
    let executable = std::env::current_exe()?;
    let start = |breakaway: bool| {
        let mut command = Command::new(&executable);
        command
            .arg("--desktop")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        /* Firefox on Windows runs its native hosts in a job it ends, with
         * everything in it, when the host exits: the dm started here would
         * go with it, and the download just taken from the browser would
         * wait for someone to open dm. Started outside the job where the job
         * allows it; where it does not, as before. */
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
            command.creation_flags(if breakaway {
                DETACHED_PROCESS | CREATE_BREAKAWAY_FROM_JOB
            } else {
                DETACHED_PROCESS
            });
        }
        let _ = breakaway;
        command.spawn().map(|_| ())
    };
    start(true).or_else(|_| start(false))
}

/// The catch rules in the app's settings file, read without touching it.
///
/// Not `Settings::load`, which moves an unreadable file aside: this process
/// is the browser's, starts on every download, and has no window to say it
/// did so. An unreadable or missing file means the defaults, which is what
/// the app itself would be running with.
pub fn catch_rules(home: &Path) -> CatchRules {
    saved(home).catch
}

pub(crate) fn saved(home: &Path) -> crate::Settings {
    std::fs::read_to_string(home.join("settings.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<crate::Settings>(&text).ok())
        .unwrap_or_default()
}

/// What answering needs from outside this process. A trait so a test can
/// answer without starting a second copy of whatever runs it, or a network.
pub trait World {
    /// Whether a dm is already there to take a download.
    fn app_running(&mut self) -> bool;
    /// Starts one.
    fn launch(&mut self) -> std::io::Result<()>;
    /// Whether dm, from outside the browser, can fetch `url` at all.
    fn reachable(&mut self, url: &str, referrer: &str, cookies: &str) -> Result<(), String>;
    /// The qualities a page's video comes in, as yt-dlp reads them.
    fn formats(&mut self, url: &str, referrer: &str) -> Result<Vec<Choice>, String>;
}

/// The real one: the instance lock, this binary, and dm's own client.
struct Here<'a> {
    home: &'a Path,
    started: bool,
}

impl World for Here<'_> {
    fn app_running(&mut self) -> bool {
        self.started || crate::instance::running(self.home)
    }

    fn launch(&mut self) -> std::io::Result<()> {
        launch_desktop()?;
        self.started = true;
        Ok(())
    }

    /// One ranged request, made the way the queue will make it: through the
    /// saved proxy, with the page as referrer. A magnet has no server to ask.
    fn reachable(&mut self, url: &str, referrer: &str, cookies: &str) -> Result<(), String> {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Ok(());
        }
        let settings = saved(self.home);
        // The same cookies the app sends, so a download they make possible
        // is taken rather than left with the browser.
        let _ = dm_core::cookies::use_file(std::path::Path::new(settings.cookies_file.trim()));
        let client =
            dm_core::proxy::client_with(&settings.proxy).unwrap_or_else(|_| dm_core::client());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let referrer = (!referrer.is_empty()).then_some(referrer);
        runtime.block_on(async {
            match tokio::time::timeout(
                std::time::Duration::from_secs(15),
                dm_core::probe::probe(
                    &client,
                    // A site's saved sign-in (Options › Sign-ins), as the
                    // app will use it: asked without it, such a link was
                    // refused and always left with the browser.
                    &settings.with_login(url),
                    referrer,
                    dm_core::cookies::SignIn::new(url, cookies).as_ref(),
                ),
            )
            .await
            {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(e)) => Err(e.to_string()),
                Err(_) => Err("the server did not answer dm within 15 seconds".into()),
            }
        })
    }

    /// yt-dlp's reading of the page, through the saved proxy, bounded.
    fn formats(&mut self, url: &str, referrer: &str) -> Result<Vec<Choice>, String> {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err("only a web page has qualities to list".into());
        }
        let proxy = saved(self.home).proxy.url_for_tools();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let referrer = (!referrer.is_empty()).then_some(referrer);
        let listed = runtime
            .block_on(dm_core::ytdlp::list_choices(
                url,
                referrer,
                proxy.as_deref(),
            ))
            .map_err(|e| e.to_string())?;
        Ok(listed
            .into_iter()
            .map(|c| Choice {
                label: c.label,
                quality: c.quality,
                size: c.size,
            })
            .collect())
    }
}

/// The answer to one frame, or `None` for a frame that gets none.
pub fn answer(
    home: &Path,
    frame: &serde_json::Value,
    world: &mut dyn World,
) -> Option<serde_json::Value> {
    let reply = match Asked::from_frame(frame) {
        // What this app can do, said up front: a player page handed to an
        // app with no yt-dlp fails in the app, where the browser never hears
        // of it. Told now, the extension can say so.
        Ok(Asked::Hello) => Reply::Hello(Hello {
            engine_version: env!("CARGO_PKG_VERSION").into(),
            ytdlp: Some(dm_core::ytdlp::available()),
        }),

        // Which downloads to catch: the app's choice, read on every ask so a
        // change in the window is in force by the next download.
        Ok(Asked::Settings) => Reply::Settings(SettingsReply {
            rules: catch_rules(home),
        }),

        Ok(Asked::Batch(Batch { id, page, urls })) => {
            let links: Vec<String> = urls
                .into_iter()
                .map(|u| u.trim().to_string())
                .filter(|u| dm_core::queue::accepted(u))
                .take(2000)
                .collect();
            if links.is_empty() {
                Reply::failed(&id, "there are no links here dm can download")
            } else {
                let filed = deposit(
                    home,
                    &id,
                    &Request {
                        url: page.clone(),
                        referrer: page,
                        ask: true,
                        links,
                        ..Request::default()
                    },
                );
                match filed {
                    Ok(()) if world.app_running() => Reply::Started { id },
                    Ok(()) => match world.launch() {
                        Ok(()) => Reply::Started { id },
                        Err(e) => {
                            Reply::failed(&id, format!("dm could not be started to take it: {e}"))
                        }
                    },
                    Err(e) => Reply::failed(&id, format!("dm could not take it: {e}")),
                }
            }
        }

        // The toolbar button: the window to the front, or dm started, which
        // opens it. Nothing is downloaded.
        Ok(Asked::Show) => {
            let shown = if world.app_running() {
                std::fs::create_dir_all(inbox(home))
                    .and_then(|()| std::fs::write(inbox(home).join("show"), b""))
            } else {
                world.launch()
            };
            Reply::Shown {
                reason: shown
                    .err()
                    .map(|e| format!("dm could not be brought up: {e}")),
            }
        }

        Ok(Asked::Formats(FormatsAsked { url, referrer })) => {
            match world.formats(url.trim(), referrer.trim()) {
                Ok(choices) => Reply::Formats {
                    choices,
                    reason: None,
                },
                Err(why) => Reply::Formats {
                    choices: Vec::new(),
                    reason: Some(why),
                },
            }
        }

        Ok(Asked::Start(Start {
            id,
            url,
            referrer,
            caught,
            quality,
            label,
            ask,
            cookies,
        })) => {
            let url = url.trim().to_string();
            if url.is_empty() {
                Reply::failed(&id, "no URL was given")
            } else if !dm_core::queue::accepted(&url) {
                // Refused here by the queue's own rule, rather than accepted
                // and then dropped by the queue unseen.
                Reply::failed(
                    &id,
                    "dm downloads http(s) and ftp links, magnets and .torrent files only",
                )
            } else if let Some(why) = caught
                .then(|| world.reachable(&url, &referrer, &cookies).err())
                .flatten()
            {
                // Before anything is filed: the browser fetches this with its
                // cookies and its session, and dm has neither. A link only
                // the signed-in browser can fetch is refused here, while the
                // browser still holds it, rather than taken from the browser
                // and failed in dm — a download caught and then lost.
                Reply::failed(
                    &id,
                    format!("dm cannot fetch this without the browser: {why}"),
                )
            } else {
                match deposit(
                    home,
                    &id,
                    &Request {
                        url,
                        referrer,
                        quality: quality.filter(|q| !q.trim().is_empty()),
                        label,
                        ask,
                        cookies: cookies.trim().to_string(),
                        ..Request::default()
                    },
                ) {
                    Ok(()) if world.app_running() => Reply::Started { id },
                    Ok(()) => match world.launch() {
                        Ok(()) => Reply::Started { id },
                        // Filed, but nothing will read it: said now, not
                        // discovered when the file never appears.
                        Err(e) => {
                            Reply::failed(&id, format!("dm could not be started to take it: {e}"))
                        }
                    },
                    Err(e) => Reply::failed(&id, format!("dm could not take it: {e}")),
                }
            }
        }

        // Another version, or something this host does not do: pausing and
        // cancelling belong to the window, which is where the download is.
        Err(_) => return None,
    };
    Some(reply.to_frame())
}

/// Runs as the browser's native messaging host until the browser closes it.
pub fn run(home: &Path) {
    let mut input = std::io::stdin();
    let mut output = std::io::stdout();
    let mut here = Here {
        home,
        started: false,
    };

    while let Some(message) = read_frame(&mut input) {
        let Some(reply) = answer(home, &message, &mut here) else {
            continue;
        };
        if output.write_all(&frame(&reply)).is_err() || output.flush().is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_survives_being_framed_and_read_back() {
        let value = serde_json::json!({ "protocolVersion": 1, "type": "engine.hello" });
        let bytes = frame(&value);

        // Four bytes of little-endian length, then the body.
        assert_eq!(&bytes[..4], &(bytes.len() as u32 - 4).to_le_bytes());
        assert_eq!(read_frame(&mut &bytes[..]), Some(value));
    }

    #[test]
    fn a_truncated_stream_ends_rather_than_blocking() {
        // The browser going away mid-message is how this normally ends.
        let bytes = frame(&serde_json::json!({ "type": "engine.hello" }));
        assert_eq!(read_frame(&mut &bytes[..6]), None);
    }

    #[test]
    fn a_frame_that_is_not_json_does_not_end_the_conversation() {
        let mut bytes = 5u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(b"{oops");
        let next = serde_json::json!({ "protocolVersion": 1, "type": "engine.hello" });
        bytes.extend_from_slice(&frame(&next));

        let mut input = &bytes[..];
        assert_eq!(read_frame(&mut input), Some(serde_json::Value::Null));
        assert_eq!(read_frame(&mut input), Some(next));
    }

    #[test]
    fn the_host_refuses_what_the_queue_would_drop() {
        for refused in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "ftps://e.com/f",
        ] {
            assert!(!dm_core::queue::accepted(refused), "{refused}");
        }
        assert!(dm_core::queue::accepted("https://e.com/f.bin"));
    }

    #[test]
    fn an_implausible_length_is_refused_rather_than_allocated() {
        // A desynchronised stream otherwise asks for gigabytes.
        let mut bytes = u32::MAX.to_le_bytes().to_vec();
        bytes.extend_from_slice(b"{}");
        assert_eq!(read_frame(&mut &bytes[..]), None);
    }

    #[test]
    fn a_request_lands_where_the_app_looks() {
        let dir = tempfile::tempdir().unwrap();
        let request = Request {
            url: "https://example.com/f.bin".into(),
            referrer: "https://example.com/page".into(),
            ..Request::default()
        };
        deposit(dir.path(), "abc", &request).unwrap();

        let file = inbox(dir.path()).join("abc.json");
        let read: Request = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
        assert_eq!(read, request);
    }

    #[test]
    fn a_browser_does_not_get_to_choose_where_a_request_is_written() {
        /* The id arrives in a message and becomes a filename. Anything that
         * writes where a message tells it to is a program that writes
         * wherever the sender likes — and `write_atomic` creates the parent,
         * so a traversal would make the directory as well as the file. */
        let dir = tempfile::tempdir().unwrap();
        let request = Request {
            url: "https://e.com/f.bin".into(),
            referrer: String::new(),
            ..Request::default()
        };

        for hostile in ["../../escaped", "..", "/etc/passwd", "a/b"] {
            let _ = deposit(dir.path(), hostile, &request);
            let escaped = dir.path().join("escaped.json");
            assert!(!escaped.exists(), "{hostile} wrote outside the inbox");
        }

        // Everything written stayed in the inbox, under a plain name.
        for entry in std::fs::read_dir(inbox(dir.path())).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)),
                "{name} is not a plain name"
            );
        }

        // An id with nothing usable in it is refused rather than guessed at.
        assert!(deposit(dir.path(), "../..", &request).is_err());
    }

    #[test]
    fn no_half_written_request_is_left_for_the_app_to_read() {
        // The app polls a directory; a partial file there is a download
        // dropped without a word.
        let dir = tempfile::tempdir().unwrap();
        deposit(
            dir.path(),
            "abc",
            &Request {
                url: "https://e.com/f".into(),
                referrer: String::new(),
                ..Request::default()
            },
        )
        .unwrap();

        let leftovers: Vec<_> = std::fs::read_dir(inbox(dir.path()))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "a temp file was left behind");
    }

    /// A world with an app that is (or is not) running, a launcher that
    /// records whether it was asked, and servers that answer dm or do not.
    struct Pretend {
        running: bool,
        launches: Result<(), ()>,
        launched: bool,
        refuses: Option<&'static str>,
        asked: Vec<(String, String)>,
        /// The cookies each probe was asked with.
        cookies_seen: Vec<String>,
    }

    impl World for Pretend {
        fn app_running(&mut self) -> bool {
            self.running
        }
        fn launch(&mut self) -> std::io::Result<()> {
            self.launched = true;
            self.launches
                .map_err(|()| std::io::Error::other("no such program"))
        }
        fn reachable(&mut self, url: &str, referrer: &str, cookies: &str) -> Result<(), String> {
            self.asked.push((url.into(), referrer.into()));
            self.cookies_seen.push(cookies.into());
            self.refuses.map_or(Ok(()), |why| Err(why.into()))
        }
        fn formats(&mut self, url: &str, _referrer: &str) -> Result<Vec<Choice>, String> {
            if url.contains("unreadable") {
                return Err("yt-dlp could not read that page".into());
            }
            Ok(vec![
                Choice {
                    label: "MP4 1280×720".into(),
                    quality: "720".into(),
                    size: Some(10),
                },
                Choice {
                    label: "M4A audio".into(),
                    quality: "audio".into(),
                    size: None,
                },
            ])
        }
    }

    #[test]
    fn the_toolbar_button_shows_the_app_and_downloads_nothing() {
        // Running: asked to show itself through the inbox, nothing filed.
        let dir = tempfile::tempdir().unwrap();
        let (reply, launched) = ask(dir.path(), Asked::Show.to_frame(), true, Ok(()));
        assert_eq!(reply, Some(Reply::Shown { reason: None }));
        assert!(inbox(dir.path()).join("show").exists());
        assert!(!launched);
        let filed = std::fs::read_dir(inbox(dir.path()))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
            .count();
        assert_eq!(filed, 0, "the toolbar button handed something over");

        // Not running: started, which opens the window.
        let dir = tempfile::tempdir().unwrap();
        let (reply, launched) = ask(dir.path(), Asked::Show.to_frame(), false, Ok(()));
        assert_eq!(reply, Some(Reply::Shown { reason: None }));
        assert!(launched);

        // Not running and cannot be started: said, for the browser to show.
        let dir = tempfile::tempdir().unwrap();
        let (reply, _) = ask(dir.path(), Asked::Show.to_frame(), false, Err(()));
        assert!(
            matches!(reply, Some(Reply::Shown { reason: Some(_) })),
            "{reply:?}"
        );
    }

    #[test]
    fn a_pages_links_reach_the_app_together_to_be_picked_from() {
        let dir = tempfile::tempdir().unwrap();
        let frame = Asked::Batch(Batch {
            id: "links-1".into(),
            page: "https://e.com/list.html".into(),
            urls: vec![
                "https://e.com/a.zip".into(),
                "mailto:a@e.com".into(),
                "https://e.com/b.iso".into(),
            ],
        })
        .to_frame();
        let (reply, _) = ask(dir.path(), frame, true, Ok(()));
        assert_eq!(
            reply,
            Some(Reply::Started {
                id: "links-1".into()
            })
        );
        let filed: Request = serde_json::from_str(
            &std::fs::read_to_string(inbox(dir.path()).join("links-1.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(filed.links, ["https://e.com/a.zip", "https://e.com/b.iso"]);
        assert_eq!(filed.referrer, "https://e.com/list.html");
        assert!(filed.ask);

        let none = Asked::Batch(Batch {
            id: "links-2".into(),
            page: "https://e.com/".into(),
            urls: vec!["mailto:a@e.com".into()],
        })
        .to_frame();
        let (reply, _) = ask(dir.path(), none, true, Ok(()));
        assert!(matches!(reply, Some(Reply::Failed { .. })));
    }

    #[test]
    fn the_panel_is_told_the_qualities_a_page_comes_in_or_why_not() {
        let dir = tempfile::tempdir().unwrap();
        let frame = |url: &str| {
            Asked::Formats(FormatsAsked {
                url: url.into(),
                referrer: String::new(),
            })
            .to_frame()
        };
        let (reply, _) = ask(
            dir.path(),
            frame("https://video.example/watch?v=1"),
            true,
            Ok(()),
        );
        let Some(Reply::Formats { choices, reason }) = reply else {
            panic!("no formats answer");
        };
        assert_eq!(reason, None);
        assert_eq!(choices[0].quality, "720");
        assert_eq!(choices.len(), 2);

        let (reply, _) = ask(
            dir.path(),
            frame("https://unreadable.example/"),
            true,
            Ok(()),
        );
        let Some(Reply::Formats { choices, reason }) = reply else {
            panic!("no formats answer");
        };
        assert!(choices.is_empty());
        assert_eq!(reason.as_deref(), Some("yt-dlp could not read that page"));
    }

    #[test]
    fn a_sites_cookies_handed_over_reach_the_app_and_the_probe() {
        let dir = tempfile::tempdir().unwrap();
        let frame = Asked::Start(Start {
            id: "c1".into(),
            url: "https://files.example/private.zip".into(),
            referrer: "https://files.example/".into(),
            caught: true,
            cookies: "session=abc; theme=dark".into(),
            ..Start::default()
        })
        .to_frame();
        let mut world = Pretend {
            running: true,
            launches: Ok(()),
            launched: false,
            refuses: None,
            asked: Vec::new(),
            cookies_seen: Vec::new(),
        };
        let reply = answer(dir.path(), &frame, &mut world).map(|r| Reply::from_frame(&r).unwrap());
        assert_eq!(reply, Some(Reply::Started { id: "c1".into() }));
        // The probe before taking it asked as the signed-in browser would.
        assert_eq!(world.cookies_seen, ["session=abc; theme=dark"]);
        let filed: Request = serde_json::from_str(
            &std::fs::read_to_string(inbox(dir.path()).join("c1.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(filed.cookies, "session=abc; theme=dark");
        // A start without any hands none over.
        let (_, _) = ask(
            dir.path(),
            start("c2", "https://plain.example/f.zip"),
            true,
            Ok(()),
        );
        let plain: Request = serde_json::from_str(
            &std::fs::read_to_string(inbox(dir.path()).join("c2.json")).unwrap(),
        )
        .unwrap();
        assert!(plain.cookies.is_empty());
    }

    #[test]
    fn a_video_from_the_panel_reaches_the_app_with_its_quality_and_asks_first() {
        let dir = tempfile::tempdir().unwrap();
        let frame = Asked::Start(Start {
            id: "panel-1".into(),
            url: "https://video.example/watch?v=1".into(),
            quality: Some("720".into()),
            label: Some("MP4 1280×720".into()),
            ask: true,
            ..Start::default()
        })
        .to_frame();
        let (reply, _) = ask(dir.path(), frame, true, Ok(()));
        assert_eq!(
            reply,
            Some(Reply::Started {
                id: "panel-1".into()
            })
        );
        let filed: Request = serde_json::from_str(
            &std::fs::read_to_string(inbox(dir.path()).join("panel-1.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(filed.quality.as_deref(), Some("720"));
        assert_eq!(filed.label.as_deref(), Some("MP4 1280×720"));
        assert!(filed.ask);
    }

    fn ask(
        home: &Path,
        frame: serde_json::Value,
        running: bool,
        launches: Result<(), ()>,
    ) -> (Option<Reply>, bool) {
        let mut world = Pretend {
            running,
            launches,
            launched: false,
            refuses: None,
            asked: Vec::new(),
            cookies_seen: Vec::new(),
        };
        let reply = answer(home, &frame, &mut world);
        (
            reply.map(|r| Reply::from_frame(&r).unwrap()),
            world.launched,
        )
    }

    fn start(id: &str, url: &str) -> serde_json::Value {
        dm_protocol::Request::Start(Start {
            id: id.into(),
            url: url.into(),
            referrer: "https://e.com/page".into(),
            caught: true,
            ..Start::default()
        })
        .to_frame()
    }

    #[test]
    fn a_hello_says_what_this_app_is() {
        let dir = tempfile::tempdir().unwrap();
        let (reply, _) = ask(
            dir.path(),
            dm_protocol::Request::Hello.to_frame(),
            true,
            Ok(()),
        );
        let Some(Reply::Hello(hello)) = reply else {
            panic!("no hello")
        };
        assert_eq!(hello.engine_version, env!("CARGO_PKG_VERSION"));
        assert!(hello.ytdlp.is_some(), "the extension is told either way");
    }

    #[test]
    fn the_extension_is_sent_the_catch_rules_in_the_apps_settings() {
        let dir = tempfile::tempdir().unwrap();
        let asked = dm_protocol::Request::Settings.to_frame();

        // No settings file: the defaults the app would run with.
        let (reply, _) = ask(dir.path(), asked.clone(), true, Ok(()));
        assert_eq!(
            reply,
            Some(Reply::Settings(SettingsReply {
                rules: CatchRules::default()
            }))
        );

        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"connections":4,"catch":{"enabled":true,"extensions":["bin"],"skipHosts":["e.com"]}}"#,
        )
        .unwrap();
        let (reply, _) = ask(dir.path(), asked.clone(), true, Ok(()));
        let Some(Reply::Settings(SettingsReply { rules })) = reply else {
            panic!("no settings")
        };
        assert_eq!(rules.extensions, vec!["bin".to_string()]);
        assert_eq!(rules.skip_hosts, vec!["e.com".to_string()]);

        // A file the app could not read is left where it is: this process
        // has no window to say it moved anything.
        std::fs::write(dir.path().join("settings.json"), "{ oops").unwrap();
        let (reply, _) = ask(dir.path(), asked, true, Ok(()));
        assert_eq!(
            reply,
            Some(Reply::Settings(SettingsReply {
                rules: CatchRules::default()
            }))
        );
        assert!(dir.path().join("settings.json").exists());
    }

    #[test]
    fn a_hand_off_is_filed_and_taken_by_the_running_app() {
        let dir = tempfile::tempdir().unwrap();
        let (reply, launched) = ask(
            dir.path(),
            start("b1-2", "https://e.com/f.zip"),
            true,
            Ok(()),
        );
        assert_eq!(reply, Some(Reply::Started { id: "b1-2".into() }));
        assert!(!launched, "a running app is not started twice");

        let filed: Request = serde_json::from_str(
            &std::fs::read_to_string(inbox(dir.path()).join("b1-2.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(filed.url, "https://e.com/f.zip");
        assert_eq!(filed.referrer, "https://e.com/page");
    }

    #[test]
    fn with_no_app_running_one_is_started_and_a_failure_to_start_is_said() {
        let dir = tempfile::tempdir().unwrap();
        let (reply, launched) = ask(dir.path(), start("a", "https://e.com/f.zip"), false, Ok(()));
        assert!(launched);
        assert_eq!(reply, Some(Reply::Started { id: "a".into() }));

        let (reply, _) = ask(
            dir.path(),
            start("b", "https://e.com/f.zip"),
            false,
            Err(()),
        );
        let Some(Reply::Failed { id, failure }) = reply else {
            panic!("a launch that failed was reported as taken")
        };
        assert_eq!(id, "b");
        assert!(failure.unwrap().message.contains("could not be started"));
    }

    #[test]
    fn what_the_queue_would_drop_is_refused_by_name() {
        let dir = tempfile::tempdir().unwrap();
        for url in ["", "javascript:alert(1)", "file:///etc/passwd"] {
            let (reply, launched) = ask(dir.path(), start("x", url), true, Ok(()));
            assert!(matches!(reply, Some(Reply::Failed { .. })), "{url}");
            assert!(!launched);
        }
        assert!(!inbox(dir.path()).join("x.json").exists());
    }

    #[test]
    fn a_link_dm_cannot_fetch_itself_is_left_with_the_browser() {
        /* The browser has the site's cookies and dm does not. Taken anyway,
         * the browser's copy is cancelled and dm's fails: the download is
         * lost. Refused, the extension gives it back to the browser. */
        let dir = tempfile::tempdir().unwrap();
        let mut world = Pretend {
            running: true,
            launches: Ok(()),
            launched: false,
            refuses: Some("server returned 403 Forbidden"),
            asked: Vec::new(),
            cookies_seen: Vec::new(),
        };
        let reply = answer(
            dir.path(),
            &start("p", "https://e.com/private.zip"),
            &mut world,
        )
        .map(|r| Reply::from_frame(&r).unwrap());
        let Some(Reply::Failed { failure, .. }) = reply else {
            panic!("a link dm cannot reach was taken")
        };
        let said = failure.unwrap().message;
        assert!(
            said.contains("without the browser") && said.contains("403"),
            "{said}"
        );
        assert_eq!(
            world.asked,
            vec![(
                "https://e.com/private.zip".into(),
                "https://e.com/page".into()
            )],
            "asked once, the way the queue will ask, referrer and all"
        );
        assert!(!inbox(dir.path()).join("p.json").exists(), "nothing filed");
    }

    #[test]
    fn a_link_handed_over_on_request_is_taken_without_asking_the_server() {
        // The browser holds nothing to give back, and asking the server now
        // would contact it outside a schedule the app would have kept to.
        let dir = tempfile::tempdir().unwrap();
        let mut world = Pretend {
            running: true,
            launches: Ok(()),
            launched: false,
            refuses: Some("unreachable"),
            asked: Vec::new(),
            cookies_seen: Vec::new(),
        };
        let asked = dm_protocol::Request::Start(Start {
            id: "m1".into(),
            url: "https://e.com/f.zip".into(),
            ..Default::default()
        })
        .to_frame();
        let reply = answer(dir.path(), &asked, &mut world).map(|r| Reply::from_frame(&r).unwrap());
        assert_eq!(reply, Some(Reply::Started { id: "m1".into() }));
        assert!(world.asked.is_empty(), "the server was asked");
        assert!(inbox(dir.path()).join("m1.json").exists());
    }

    #[test]
    fn a_magnet_is_not_probed_as_though_it_had_a_server() {
        let dir = tempfile::tempdir().unwrap();
        let mut here = Here {
            home: dir.path(),
            started: false,
        };
        assert_eq!(here.reachable("magnet:?xt=urn:btih:abc", "", ""), Ok(()));
    }

    #[test]
    fn a_frame_this_host_does_not_answer_gets_nothing() {
        let dir = tempfile::tempdir().unwrap();
        for frame in [
            serde_json::json!({ "protocolVersion": 1, "type": "download.pause", "id": "a" }),
            serde_json::json!({ "protocolVersion": 2, "type": "engine.hello" }),
            serde_json::Value::Null,
        ] {
            assert_eq!(ask(dir.path(), frame, true, Ok(())).0, None);
        }
    }
}
