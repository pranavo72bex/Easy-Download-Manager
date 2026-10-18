//! The server zoo: how real servers behave, one case each, and what dm
//! makes of them.
//!
//! A corpus of cases and a floor that is raised and never lowered: each
//! case is a server behaviour seen in the wild — a redirect chain, a range that lies, a body cut short, a name only in a header —
//! with what dm must do about it. The count that pass is held to a floor in
//! `tests/zoo-floor.txt`; a change that makes one fail fails the build.
//!
//! A second opinion: where `curl` is on the machine (it is on macOS and
//! most Linux), the same address is fetched with curl and the bytes must
//! agree with dm's.
//!
//!     cargo test -p dm-core --test zoo -- --nocapture       # the table
//!     DM_WRITE_ZOO=1 cargo test -p dm-core --test zoo       # docs/zoo.md
//!     DM_RAISE_FLOOR=1 cargo test -p dm-core --test zoo     # after a fix

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Uri};
use axum::response::Response;
use dm_core::{client, download};
use futures_util::StreamExt;

/// What dm has to do with a case.
#[derive(Clone)]
enum Expect {
    /// Save exactly these bytes.
    Bytes(Vec<u8>),
    /// Save exactly these bytes, under a name ending like this.
    Named(Vec<u8>, &'static str),
    /// Refuse: no file saved, an error said.
    Refused,
    /// Either save exactly these bytes or refuse — never anything else.
    Safe(Vec<u8>),
}

struct Case {
    name: &'static str,
    /// What the server does, in a sentence.
    what: &'static str,
    path: &'static str,
    expect: Expect,
}

const SMALL: usize = 300 * 1024;
const LARGE: usize = 3 * 1024 * 1024;

fn body(n: usize, seed: u8) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u8).wrapping_mul(13).wrapping_add(seed) ^ ((i >> 8) as u8))
        .collect()
}

fn cases() -> Vec<Case> {
    let small = body(SMALL, 1);
    let large = body(LARGE, 2);
    vec![
        Case {
            name: "plain",
            what: "200 with a Content-Length",
            path: "/plain/file.bin",
            expect: Expect::Bytes(small.clone()),
        },
        Case {
            name: "chunked",
            what: "200, chunked, no length",
            path: "/chunked/file.bin",
            expect: Expect::Bytes(small.clone()),
        },
        Case {
            name: "ranges",
            what: "honest 206 ranges, several connections",
            path: "/ranges/file.bin",
            expect: Expect::Bytes(large.clone()),
        },
        Case {
            name: "ranges-ignored",
            what: "answers every range with the whole file",
            path: "/ranges-ignored/file.bin",
            expect: Expect::Bytes(large.clone()),
        },
        Case {
            name: "ranges-too-long",
            what: "206 with more bytes than asked",
            path: "/ranges-too-long/file.bin",
            expect: Expect::Safe(large.clone()),
        },
        Case {
            name: "ranges-other-range",
            what: "206 for a different range than asked, and says so",
            path: "/ranges-other-range/file.bin",
            expect: Expect::Safe(large.clone()),
        },
        Case {
            name: "ranges-lying-body",
            what: "206 whose header is right and whose bytes are from elsewhere",
            path: "/ranges-lying-body/file.bin",
            expect: Expect::Safe(large.clone()),
        },
        Case {
            name: "zero-length",
            what: "an empty file",
            path: "/zero/empty.txt",
            expect: Expect::Bytes(Vec::new()),
        },
        Case {
            name: "gzip",
            what: "gzip-encoded without being asked for ranges",
            path: "/gzip/file.txt",
            expect: Expect::Bytes(small.clone()),
        },
        Case {
            name: "gzip-ranged",
            what: "ranges over the gzip length where gzip is accepted, as GitHub's raw files",
            path: "/gzip-ranged/README",
            expect: Expect::Bytes(large.clone()),
        },
        Case {
            name: "disposition-quoted",
            what: "filename=\"…\" with a space",
            path: "/disposition-quoted/get",
            expect: Expect::Named(small.clone(), "report final.pdf"),
        },
        Case {
            name: "disposition-extended",
            what: "filename*=UTF-8''… percent-encoded",
            path: "/disposition-extended/get",
            expect: Expect::Named(small.clone(), "résumé.pdf"),
        },
        Case {
            name: "disposition-both",
            what: "filename and filename* both, filename* wins",
            path: "/disposition-both/get",
            expect: Expect::Named(small.clone(), "real.bin"),
        },
        Case {
            name: "disposition-traversal",
            what: "filename=\"../../etc/passwd\"",
            path: "/disposition-traversal/get",
            expect: Expect::Named(small.clone(), "passwd"),
        },
        Case {
            name: "disposition-long",
            what: "a 600-character name",
            path: "/disposition-long/get",
            expect: Expect::Named(small.clone(), ".zip"),
        },
        Case {
            name: "type-only",
            what: "no name anywhere but a Content-Type",
            path: "/type-only/get",
            expect: Expect::Named(small.clone(), ".pdf"),
        },
        Case {
            name: "unicode-path",
            what: "a percent-encoded non-Latin path",
            path: "/unicode/%E3%83%95%E3%82%A1%E3%82%A4%E3%83%AB.txt",
            expect: Expect::Named(small.clone(), "ファイル.txt"),
        },
        Case {
            name: "query-string",
            what: "the name before a query string",
            path: "/query/setup.exe?token=abc&x=1",
            expect: Expect::Named(small.clone(), "setup.exe"),
        },
        Case {
            name: "redirect-chain",
            what: "302 then 301 then 307",
            path: "/redirect-chain/0",
            expect: Expect::Named(small.clone(), "landed.bin"),
        },
        Case {
            name: "redirect-relative",
            what: "a relative Location",
            path: "/redirect-relative/start",
            expect: Expect::Named(small.clone(), "target.bin"),
        },
        Case {
            name: "redirect-loop",
            what: "redirects to itself",
            path: "/redirect-loop/x",
            expect: Expect::Refused,
        },
        Case {
            name: "not-found",
            what: "404",
            path: "/status/404",
            expect: Expect::Refused,
        },
        Case {
            name: "gone",
            what: "410",
            path: "/status/410",
            expect: Expect::Refused,
        },
        Case {
            name: "unauthorized",
            what: "401",
            path: "/status/401",
            expect: Expect::Refused,
        },
        Case {
            name: "busy-then-ok",
            what: "503 once, then the file",
            path: "/busy-then-ok/file.bin",
            expect: Expect::Bytes(small.clone()),
        },
        Case {
            name: "rate-limited",
            what: "429 with Retry-After: 1, then the file",
            path: "/rate-limited/file.bin",
            expect: Expect::Bytes(small.clone()),
        },
        Case {
            name: "short-body",
            what: "promises more than it sends, then closes",
            path: "/short-body/file.bin",
            expect: Expect::Refused,
        },
        Case {
            name: "dropped-once",
            what: "drops one range mid-body",
            path: "/dropped-once/file.bin",
            expect: Expect::Bytes(large.clone()),
        },
        Case {
            name: "changes-midway",
            what: "the file is republished during the download",
            path: "/changes-midway/file.bin",
            expect: Expect::Safe(body(LARGE, 9)),
        },
        Case {
            name: "slow-trickle",
            what: "a few kilobytes at a time, slowly",
            path: "/slow/file.bin",
            expect: Expect::Bytes(body(64 * 1024, 3)),
        },
        Case {
            name: "html-error-page",
            what: "a 500 with an HTML page",
            path: "/status/500",
            expect: Expect::Refused,
        },
        Case {
            name: "accept-ranges-none",
            what: "says Accept-Ranges: none but answers ranges",
            path: "/accept-none/file.bin",
            expect: Expect::Bytes(large.clone()),
        },
    ]
}

#[derive(Clone, Default)]
struct Server {
    hits: Arc<Mutex<HashMap<String, usize>>>,
}

impl Server {
    fn hit(&self, key: &str) -> usize {
        let mut hits = self.hits.lock().unwrap();
        let n = hits.entry(key.to_string()).or_default();
        *n += 1;
        *n - 1
    }
}

fn range_of(headers: &HeaderMap, len: usize) -> Option<(usize, usize)> {
    let value = headers
        .get("range")?
        .to_str()
        .ok()?
        .strip_prefix("bytes=")?;
    let (a, b) = value.split_once('-')?;
    let a: usize = a.parse().ok()?;
    let b = b
        .parse::<usize>()
        .unwrap_or(len.saturating_sub(1))
        .min(len.saturating_sub(1));
    (a <= b).then_some((a, b))
}

fn ranged(data: &[u8], headers: &HeaderMap, etag: &str) -> Response {
    let if_range = headers.get("if-range").and_then(|v| v.to_str().ok());
    match range_of(headers, data.len()) {
        Some((a, b)) if if_range.is_none_or(|v| v == etag) => Response::builder()
            .status(206)
            .header("etag", etag)
            .header("content-range", format!("bytes {a}-{b}/{}", data.len()))
            .body(Body::from(data[a..=b].to_vec()))
            .unwrap(),
        _ => Response::builder()
            .header("etag", etag)
            .header("content-length", data.len())
            .body(Body::from(data.to_vec()))
            .unwrap(),
    }
}

fn plain(data: Vec<u8>) -> Response {
    Response::builder()
        .header("content-length", data.len())
        .body(Body::from(data))
        .unwrap()
}

fn with_header(data: Vec<u8>, name: &str, value: &str) -> Response {
    Response::builder()
        .header("content-length", data.len())
        .header(name, value)
        .body(Body::from(data))
        .unwrap()
}

async fn handler(State(s): State<Server>, uri: Uri, headers: HeaderMap) -> Response {
    let small = body(SMALL, 1);
    let large = body(LARGE, 2);
    let path = uri.path().to_string();
    let first = path.trim_start_matches('/').split('/').next().unwrap_or("");
    match first {
        "plain" => plain(small),
        "chunked" => {
            let chunks: Vec<_> = small
                .chunks(16 * 1024)
                .map(|c| Ok::<_, std::io::Error>(bytes::Bytes::copy_from_slice(c)))
                .collect();
            Response::new(Body::from_stream(futures_util::stream::iter(chunks)))
        }
        "ranges" => ranged(&large, &headers, "\"v1\""),
        "ranges-ignored" => plain(large),
        "ranges-too-long" => match range_of(&headers, large.len()) {
            Some((a, b)) if b > a => {
                let end = (b + 100).min(large.len() - 1);
                Response::builder()
                    .status(206)
                    .header("content-range", format!("bytes {a}-{b}/{}", large.len()))
                    .body(Body::from(large[a..=end].to_vec()))
                    .unwrap()
            }
            _ => ranged(&large, &headers, "\"v1\""),
        },
        "ranges-other-range" => match range_of(&headers, large.len()) {
            Some((a, b)) if a > 0 => {
                // Rounds the start down to a 64 KB block, and says so.
                let from = a - (a % 65536).min(a).max(1);
                Response::builder()
                    .status(206)
                    .header("content-range", format!("bytes {from}-{b}/{}", large.len()))
                    .body(Body::from(large[from..=b].to_vec()))
                    .unwrap()
            }
            _ => ranged(&large, &headers, "\"v1\""),
        },
        "ranges-lying-body" => match range_of(&headers, large.len()) {
            Some((a, b)) if a > 0 => {
                // Says it is the asked range, sends bytes from 7 earlier.
                let from = a - 7;
                let len = b - a + 1;
                Response::builder()
                    .status(206)
                    .header("content-range", format!("bytes {a}-{b}/{}", large.len()))
                    .body(Body::from(large[from..from + len].to_vec()))
                    .unwrap()
            }
            _ => ranged(&large, &headers, "\"v1\""),
        },
        "zero" => plain(Vec::new()),
        "gzip" => {
            use std::io::Write;
            let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(&small).unwrap();
            let gz = e.finish().unwrap();
            Response::builder()
                .header("content-encoding", "gzip")
                .header("content-length", gz.len())
                .body(Body::from(gz))
                .unwrap()
        }
        // Compressed when compression is accepted, and ranged over the
        // compressed bytes: a client that asks for gzip and decodes it gets
        // each range longer than it asked for.
        "gzip-ranged" => {
            let accepts_gzip = headers
                .get("accept-encoding")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.contains("gzip"));
            if !accepts_gzip {
                return ranged(&large, &headers, "\"g1\"");
            }
            use std::io::Write;
            let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(&large).unwrap();
            let gz = e.finish().unwrap();
            let mut res = ranged(&gz, &headers, "\"g1-gzip\"");
            res.headers_mut()
                .insert("content-encoding", "gzip".parse().unwrap());
            res
        }
        "disposition-quoted" => with_header(
            small,
            "content-disposition",
            "attachment; filename=\"report final.pdf\"",
        ),
        "disposition-extended" => with_header(
            small,
            "content-disposition",
            "attachment; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf",
        ),
        "disposition-both" => with_header(
            small,
            "content-disposition",
            "attachment; filename=\"fallback.bin\"; filename*=UTF-8''real.bin",
        ),
        "disposition-traversal" => with_header(
            small,
            "content-disposition",
            "attachment; filename=\"../../etc/passwd\"",
        ),
        "disposition-long" => with_header(
            small,
            "content-disposition",
            &format!("attachment; filename=\"{}.zip\"", "a".repeat(600)),
        ),
        "type-only" => with_header(small, "content-type", "application/pdf"),
        "unicode" | "query" => plain(small),
        "redirect-chain" => {
            let step: usize = path
                .rsplit('/')
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(9);
            let (status, next) = match step {
                0 => (302, "/redirect-chain/1"),
                1 => (301, "/redirect-chain/2"),
                2 => (307, "/redirect-chain/landed.bin"),
                _ => return plain(small),
            };
            Response::builder()
                .status(status)
                .header("location", next)
                .body(Body::empty())
                .unwrap()
        }
        "redirect-relative" if path.ends_with("/start") => Response::builder()
            .status(302)
            .header("location", "target.bin")
            .body(Body::empty())
            .unwrap(),
        "redirect-relative" => plain(small),
        "redirect-loop" => Response::builder()
            .status(302)
            .header("location", path.clone())
            .body(Body::empty())
            .unwrap(),
        "status" => {
            let code: u16 = path
                .rsplit('/')
                .next()
                .and_then(|c| c.parse().ok())
                .unwrap_or(500);
            Response::builder()
                .status(code)
                .header("content-type", "text/html")
                .body(Body::from("<html><body>No.</body></html>"))
                .unwrap()
        }
        "busy-then-ok" => {
            if s.hit("busy") == 0 {
                Response::builder()
                    .status(503)
                    .body(Body::from("busy"))
                    .unwrap()
            } else {
                plain(small)
            }
        }
        "rate-limited" => {
            if s.hit("rate") == 0 {
                Response::builder()
                    .status(429)
                    .header("retry-after", "1")
                    .body(Body::from("slow down"))
                    .unwrap()
            } else {
                plain(small)
            }
        }
        "short-body" => {
            let half = bytes::Bytes::copy_from_slice(&small[..SMALL / 2]);
            let stream = futures_util::stream::iter(vec![
                Ok::<_, std::io::Error>(half),
                Err(std::io::Error::other("closed")),
            ]);
            Response::builder()
                .header("content-length", SMALL)
                .body(Body::from_stream(stream))
                .unwrap()
        }
        "dropped-once" => {
            let response = ranged(&large, &headers, "\"v1\"");
            // The second request (the first range) is cut off part-way.
            if s.hit("dropped") != 1 || response.status() != 206 {
                return response;
            }
            let (parts, b) = response.into_parts();
            let bytes = axum::body::to_bytes(b, usize::MAX).await.unwrap();
            let stream = futures_util::stream::iter(vec![
                Ok::<_, std::io::Error>(bytes.slice(..1000.min(bytes.len()))),
                Err(std::io::Error::other("dropped")),
            ])
            .then(|x| async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
                x
            });
            Response::from_parts(parts, Body::from_stream(stream))
        }
        "changes-midway" => {
            // The first few requests see version 1; then it is republished.
            let n = s.hit("changes");
            if n < 3 {
                ranged(&large, &headers, "\"v1\"")
            } else {
                ranged(&body(LARGE, 9), &headers, "\"v2\"")
            }
        }
        "slow" => {
            let data = body(64 * 1024, 3);
            let chunks: Vec<_> = data
                .chunks(4 * 1024)
                .map(bytes::Bytes::copy_from_slice)
                .collect();
            let stream = futures_util::stream::iter(chunks).then(|c| async move {
                tokio::time::sleep(Duration::from_millis(20)).await;
                Ok::<_, std::io::Error>(c)
            });
            Response::builder()
                .header("content-length", 64 * 1024)
                .body(Body::from_stream(stream))
                .unwrap()
        }
        "accept-none" => {
            let mut response = ranged(&large, &headers, "\"v1\"");
            response
                .headers_mut()
                .insert("accept-ranges", "none".parse().unwrap());
            response
        }
        _ => Response::builder().status(404).body(Body::empty()).unwrap(),
    }
}

/// What happened to one case.
struct Outcome {
    passed: bool,
    said: String,
    curl: Option<bool>,
}

async fn run_case(addr: std::net::SocketAddr, case: &Case) -> Outcome {
    let dir = common::tempdir();
    let options = dm_core::Options {
        connections: 4,
        ..common::options(dir.path())
    };
    let url = format!("http://{addr}{}", case.path);
    let result =
        tokio::time::timeout(Duration::from_secs(60), download(&client(), &url, &options)).await;
    let saved = match &result {
        Ok(Ok(finished)) => Some((
            std::fs::read(&finished.path).unwrap_or_default(),
            finished.path.clone(),
        )),
        _ => None,
    };
    let left_files = std::fs::read_dir(dir.path().join("out"))
        .map(|d| d.flatten().filter(|e| e.path().is_file()).count())
        .unwrap_or(0);
    let said = match &result {
        Ok(Ok(f)) => format!(
            "saved {} bytes as {}",
            f.bytes,
            f.path.file_name().unwrap_or_default().to_string_lossy()
        ),
        Ok(Err(e)) => format!("refused: {e}"),
        Err(_) => "timed out".into(),
    };
    let passed = match (&case.expect, &saved) {
        (Expect::Bytes(want), Some((got, _))) => got == want,
        (Expect::Named(want, ending), Some((got, path))) => {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            got == want && name.ends_with(ending) && name.len() <= 240
        }
        (Expect::Refused, None) => result.is_ok() && left_files == 0,
        (Expect::Safe(want), Some((got, _))) => got == want,
        (Expect::Safe(_), None) => result.is_ok() && left_files == 0,
        _ => false,
    };

    // The second opinion: curl's bytes, where curl is here and the case
    // expects bytes at all.
    let curl = match (&case.expect, &saved) {
        (Expect::Bytes(_) | Expect::Named(..), Some((got, _))) => {
            let out = dir.path().join("curl.out");
            let url = url.clone();
            let out2 = out.clone();
            let ran = tokio::task::spawn_blocking(move || {
                std::process::Command::new("curl")
                    .args([
                        "-sSL",
                        "--compressed",
                        "--retry",
                        "3",
                        "--retry-all-errors",
                        "-o",
                    ])
                    .arg(&out2)
                    .arg(&url)
                    .status()
            })
            .await
            .ok()
            .and_then(Result::ok);
            match ran {
                Some(status) if status.success() => {
                    Some(std::fs::read(&out).unwrap_or_default() == *got)
                }
                _ => None,
            }
        }
        _ => None,
    };
    Outcome { passed, said, curl }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_zoo_holds_its_floor() {
    let addr = common::serve(
        axum::Router::new()
            .fallback(handler)
            .with_state(Server::default()),
    )
    .await;
    let cases = cases();
    let mut rows = Vec::new();
    let mut passed = 0;
    let (mut agreed, mut compared) = (0, 0);
    // At once: each case has its own path on the server and its own
    // directory, and the slow ones are slow by waiting, not by working.
    let outcomes =
        futures_util::future::join_all(cases.iter().map(|case| run_case(addr, case))).await;
    for (case, outcome) in cases.iter().zip(outcomes) {
        if outcome.passed {
            passed += 1;
        }
        if let Some(same) = outcome.curl {
            compared += 1;
            if same {
                agreed += 1;
            }
        }
        println!(
            "{} {:<22} {}  [{}]",
            if outcome.passed { "PASS" } else { "FAIL" },
            case.name,
            case.what,
            outcome.said
        );
        rows.push((case, outcome));
    }
    println!(
        "{passed} of {} pass; curl agreed on {agreed} of {compared}",
        cases.len()
    );

    let floor_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/zoo-floor.txt");
    let floor: usize = std::fs::read_to_string(floor_path)
        .ok()
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or(0);
    if std::env::var_os("DM_RAISE_FLOOR").is_some() && passed > floor {
        std::fs::write(floor_path, format!("{passed}\n")).unwrap();
    }
    if std::env::var_os("DM_WRITE_ZOO").is_some() {
        let mut doc = format!(
            "# The server zoo\n\nGenerated by `DM_WRITE_ZOO=1 cargo test -p dm-core --test zoo` — do not edit by hand.\n\n\
             How servers behave in the wild, one case each, against what dm must do. \
             The count that pass is held to a floor that is only ever raised \
             (`desktop/crates/dm-core/tests/zoo-floor.txt`). Where curl is installed it fetches \
             the same address as a second opinion.\n\n\
             **{passed} of {}** pass; curl agreed with dm's bytes on **{agreed} of {compared}** it fetched.\n\n\
             | Case | The server | dm must | Result | dm | curl |\n|---|---|---|---|---|---|\n",
            cases.len()
        );
        for (case, outcome) in &rows {
            let must = match case.expect {
                Expect::Bytes(_) => "save the bytes exactly",
                Expect::Named(_, _) => "save the bytes, named right",
                Expect::Refused => "refuse, saving nothing",
                Expect::Safe(_) => "save the right bytes or refuse",
            };
            doc.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} |\n",
                case.name,
                case.what.replace('|', "\\|"),
                must,
                if outcome.passed { "PASS" } else { "**FAIL**" },
                outcome
                    .said
                    .replace('|', "\\|")
                    .replace(&addr.to_string(), "…"),
                match outcome.curl {
                    Some(true) => "same",
                    Some(false) => "**differs**",
                    None => "—",
                }
            ));
        }
        std::fs::write(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../../docs/zoo.md"),
            doc,
        )
        .unwrap();
    }

    assert!(
        passed >= floor,
        "the zoo fell below its floor: {passed} pass, the floor is {floor}"
    );
    assert_eq!(
        agreed, compared,
        "curl and dm disagree about bytes dm saved"
    );
}
