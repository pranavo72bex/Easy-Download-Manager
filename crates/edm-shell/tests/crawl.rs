// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! "Site files" past the first page: how deep it reads, what it keeps
//! and skips, and where it stops.

use axum::response::Html;
use axum::routing::get;
use edm_shell::lists::{crawl, Search, MAX_PAGES};

async fn site() -> String {
    let app = axum::Router::new()
        .route(
            "/",
            get(|| async {
                Html(
                    r#"<a href="a.zip">a</a> <a href="sub/">sub</a> <a href="skip/index.html">skip</a>
                       <a href="notes.txt">t</a> <a href="http://elsewhere.invalid/p.html">away</a>"#,
                )
            }),
        )
        .route(
            "/sub/",
            get(|| async { Html(r#"<a href="c.zip">c</a> <a href="deeper/">d</a>"#) }),
        )
        .route("/sub/deeper/", get(|| async { Html(r#"<a href="e.zip">e</a>"#) }))
        .route("/skip/index.html", get(|| async { Html(r#"<a href="b.zip">b</a>"#) }))
        .route("/a.zip", get(|| async { "not html" }))
        .route(
            "/many",
            get(|| async {
                Html(
                    (0..300)
                        .map(|i| format!("<a href=\"/p/{i}\">p</a>"))
                        .collect::<String>(),
                )
            }),
        )
        .route("/p/{n}", get(|| async { Html(r#"<a href="/x.zip">x</a>"#) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn zips(base: &str, depth: u8) -> Search {
    Search {
        page: format!("{base}/"),
        types: vec!["zip".into()],
        depth,
        ..Search::default()
    }
}

fn names(files: &[String]) -> Vec<&str> {
    files
        .iter()
        .map(|f| f.rsplit('/').next().unwrap())
        .collect()
}

#[tokio::test]
async fn depth_is_how_many_pages_past_the_start_are_read_and_only_on_this_site() {
    let base = site().await;
    let client = edm_core::client();

    let top = crawl(&client, &zips(&base, 0)).await.unwrap();
    assert_eq!(names(&top.files), ["a.zip"]);
    assert_eq!(top.pages, 1);

    let one = crawl(&client, &zips(&base, 1)).await.unwrap();
    assert_eq!(names(&one.files), ["a.zip", "c.zip", "b.zip"]);
    // The start page, sub/ and skip/: the page elsewhere was never asked for.
    assert_eq!(one.pages, 3);

    let two = crawl(&client, &zips(&base, 2)).await.unwrap();
    assert_eq!(names(&two.files), ["a.zip", "c.zip", "b.zip", "e.zip"]);
    assert_eq!(two.pages, 4);

    // Every file, when no type is named: pages are still pages.
    let all = crawl(
        &client,
        &Search {
            types: Vec::new(),
            ..zips(&base, 0)
        },
    )
    .await
    .unwrap();
    assert_eq!(names(&all.files), ["a.zip", "notes.txt"]);
}

#[tokio::test]
async fn paths_are_kept_or_skipped_by_what_they_contain() {
    let base = site().await;
    let client = edm_core::client();
    let skipped = crawl(
        &client,
        &Search {
            exclude: vec!["Skip".into()],
            ..zips(&base, 1)
        },
    )
    .await
    .unwrap();
    assert_eq!(names(&skipped.files), ["a.zip", "c.zip"]);
    assert_eq!(skipped.pages, 2, "a skipped path is not read either");

    let only = crawl(
        &client,
        &Search {
            include: vec!["sub".into()],
            ..zips(&base, 1)
        },
    )
    .await
    .unwrap();
    assert_eq!(names(&only.files), ["c.zip"]);
}

#[tokio::test]
async fn a_search_stops_at_a_hundred_pages() {
    let base = site().await;
    let many = crawl(
        &edm_core::client(),
        &Search {
            page: format!("{base}/many"),
            ..zips(&base, 1)
        },
    )
    .await
    .unwrap();
    assert_eq!(many.pages, MAX_PAGES);
    assert_eq!(names(&many.files), ["x.zip"], "each file once");
}

#[tokio::test]
async fn a_start_address_that_is_a_file_or_missing_is_said() {
    let base = site().await;
    let client = edm_core::client();
    let file = crawl(
        &client,
        &Search {
            page: format!("{base}/a.zip"),
            ..zips(&base, 0)
        },
    )
    .await
    .unwrap_err();
    assert!(file.contains("not a page"), "{file}");
    let missing = crawl(
        &client,
        &Search {
            page: format!("{base}/nowhere.html"),
            ..zips(&base, 0)
        },
    )
    .await
    .unwrap_err();
    assert!(missing.contains("404"), "{missing}");
}

#[tokio::test]
async fn a_start_page_that_sends_everyone_to_another_host_is_searched_there() {
    /* `example.com` that redirects to `www.example.com`: every link on the
     * page is on the second host, and held to the first, nothing past the
     * start page was read. Here `localhost` sends everyone to `127.0.0.1`. */
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = axum::Router::new()
        .route(
            "/",
            get(move |headers: axum::http::HeaderMap| async move {
                use axum::response::IntoResponse;
                let host = headers.get("host").unwrap().to_str().unwrap();
                if host.starts_with("localhost") {
                    axum::response::Redirect::permanent(&format!("http://127.0.0.1:{port}/"))
                        .into_response()
                } else {
                    Html(r#"<a href="sub/">sub</a> <a href="a.zip">a</a>"#).into_response()
                }
            }),
        )
        .route("/sub/", get(|| async { Html(r#"<a href="c.zip">c</a>"#) }));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let found = crawl(
        &edm_core::client(),
        &zips(&format!("http://localhost:{port}"), 1),
    )
    .await
    .unwrap();
    assert_eq!(found.pages, 2, "the page past the start was not read");
    assert_eq!(names(&found.files), ["a.zip", "c.zip"]);
}

/// A run given a time limit stops reading pages when it runs out, with
/// what it found so far, and says that it stopped.
#[tokio::test]
async fn a_search_stops_when_its_time_runs_out() {
    let app = axum::Router::new()
        .route(
            "/",
            get(|| async { Html(r#"<a href="a.zip">a</a> <a href="slow/">slow</a>"#) }),
        )
        .route(
            "/slow/",
            get(|| async {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                Html(r#"<a href="late.zip">late</a>"#)
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let search = zips(&format!("http://{addr}"), 1);
    let started = std::time::Instant::now();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
    let found = edm_shell::lists::crawl_until(&edm_core::client(), &search, Some(deadline))
        .await
        .unwrap();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    assert!(found.stopped);
    assert_eq!(names(&found.files), ["a.zip"]);
}

#[tokio::test]
async fn pages_on_other_sites_are_read_only_when_asked() {
    let other = axum::Router::new().route(
        "/more.html",
        get(|| async { Html(r#"<a href="far.zip">far</a>"#) }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, other).await.unwrap() });
    // `localhost` is another site than `127.0.0.1`, as two names are.
    let away = format!("http://localhost:{port}/more.html");
    let home = axum::Router::new().route(
        "/",
        get(move || {
            let away = away.clone();
            async move {
                Html(format!(
                    r#"<a href="near.zip">n</a> <a href="{away}">away</a>"#
                ))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, home).await.unwrap() });
    let base = format!("http://{addr}");
    let client = edm_core::client();

    let own = crawl(&client, &zips(&base, 1)).await.unwrap();
    assert_eq!(names(&own.files), ["near.zip"]);
    assert_eq!(own.pages, 1);

    let wide = crawl(
        &client,
        &Search {
            other_sites: true,
            ..zips(&base, 1)
        },
    )
    .await
    .unwrap();
    assert_eq!(names(&wide.files), ["near.zip", "far.zip"]);
    assert_eq!(wide.pages, 2);
}

#[tokio::test]
async fn a_site_that_wants_a_login_is_read_with_the_one_it_was_given_and_never_says_it() {
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::{IntoResponse, Response};
    // "me:pw" as Basic carries it, in pieces so no tracked file holds one whole.
    fn gate(headers: &HeaderMap, page: &'static str) -> Response {
        let wanted = ["Basic ", "bWU6", "cHc="].concat();
        if headers.get("authorization").and_then(|v| v.to_str().ok()) == Some(wanted.as_str()) {
            Html(page).into_response()
        } else {
            StatusCode::UNAUTHORIZED.into_response()
        }
    }
    let app = axum::Router::new()
        .route(
            "/",
            get(move |h: HeaderMap| async move {
                gate(&h, r#"<a href="a.zip">a</a> <a href="sub/">sub</a>"#)
            }),
        )
        .route(
            "/sub/",
            get(move |h: HeaderMap| async move { gate(&h, r#"<a href="b.zip">b</a>"#) }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = edm_core::client();

    // Without a login the start page is refused, and the answer is said.
    let refused = crawl(&client, &zips(&format!("http://{addr}"), 1))
        .await
        .unwrap_err();
    assert!(refused.contains("401"), "{refused}");

    // With one, the start page and the page under it are read, the login
    // carried to each, and no file link takes it along.
    let login = ["me", "pw"].join(":");
    let found = crawl(&client, &zips(&format!("http://{login}@{addr}"), 1))
        .await
        .unwrap();
    assert_eq!(names(&found.files), ["a.zip", "b.zip"]);
    assert!(
        found.files.iter().all(|f| !f.contains('@')),
        "{:?}",
        found.files
    );
}

/// The Grabber's size filters (gaps row 92): a file under the floor or over
/// the ceiling is left out; one whose size the server does not say is kept.
#[tokio::test]
async fn files_outside_the_size_limits_are_left_out_and_unknown_sizes_kept() {
    let app = axum::Router::new()
        .route(
            "/",
            get(|| async {
                Html(r#"<a href="small.zip">s</a> <a href="mid.zip">m</a> <a href="big.zip">b</a> <a href="unsaid.zip">u</a>"#)
            }),
        )
        .route("/small.zip", get(|| async { vec![0u8; 2 * 1024] }))
        .route("/mid.zip", get(|| async { vec![0u8; 50 * 1024] }))
        .route("/big.zip", get(|| async { vec![0u8; 900 * 1024] }))
        // Streamed, so no length is said, to a HEAD or a GET.
        .route(
            "/unsaid.zip",
            get(|| async {
                axum::body::Body::from_stream(futures_util::stream::iter([Ok::<_, std::io::Error>(
                    vec![0u8; 10],
                )]))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let search = Search {
        page: format!("http://{addr}/"),
        types: vec!["zip".into()],
        min_kb: 10,
        max_kb: 500,
        ..Search::default()
    };
    let found = crawl(&edm_core::client(), &search).await.unwrap();
    let mut kept = names(&found.files);
    kept.sort_unstable();
    assert_eq!(kept, ["mid.zip", "unsaid.zip"]);
}

/// Stop pressed during a run ends it after the page it is on, with what it found, said to have
/// stopped; the watch counts the pages read and the files found (gaps row U25).
#[tokio::test]
async fn a_search_stopped_keeps_what_it_found_and_counts_as_it_goes() {
    let app = axum::Router::new()
        .route(
            "/",
            get(|| async { Html(r#"<a href="a.zip">a</a> <a href="more/">more</a>"#) }),
        )
        .route(
            "/more/",
            get(|| async { Html(r#"<a href="late.zip">late</a>"#) }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let search = zips(&format!("http://{addr}"), 1);
    let watch = edm_shell::lists::CrawlWatch::default();
    watch.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let found = edm_shell::lists::crawl_watched(&edm_core::client(), &search, None, &watch)
        .await
        .unwrap();
    assert!(found.stopped);
    assert_eq!(found.pages, 1, "the start page alone is read");
    assert_eq!(names(&found.files), ["a.zip"]);
    let counted = (
        watch.pages.load(std::sync::atomic::Ordering::Relaxed),
        watch.files.load(std::sync::atomic::Ordering::Relaxed),
    );
    assert_eq!(counted, (1, 1));
}

/// A three-page site for "Save pages for offline reading" (gaps row F2): the start page in
/// windows-1252, said only by the server, with a stylesheet (which imports another and names a
/// picture), a picture, links to the two other pages and one to a page on another host; a page
/// further in with a `<base href>`. The other host counts what it is asked for.
async fn three_pages() -> (
    String,
    String,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    use axum::http::header::CONTENT_TYPE;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let asked = std::sync::Arc::new(AtomicUsize::new(0));
    let counted = asked.clone();
    let outside = axum::Router::new().fallback(move || {
        counted.fetch_add(1, Ordering::SeqCst);
        async { Html("<p>elsewhere</p>") }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    // Another host to the site: `localhost`, not `127.0.0.1`.
    let away = format!("http://localhost:{}", listener.local_addr().unwrap().port());
    tokio::spawn(async move { axum::serve(listener, outside).await.unwrap() });

    let mut index =
        br#"<!DOCTYPE html><html><head><link rel="stylesheet" href="css/site.css"><title>Caf"#
            .to_vec();
    index.push(0xE9);
    index.extend_from_slice(
        format!(
            r#"</title></head><body><img src="img/logo.png" alt=""><a href="about.html">About</a>
<a href="docs/guide?id=2#part">Guide</a> <a href="{away}/away.html">Away</a></body></html>"#
        )
        .as_bytes(),
    );
    let png = || async { ([(CONTENT_TYPE, "image/png")], vec![0x89, b'P', b'N', b'G']) };
    let css = |body: &'static str| move || async move { ([(CONTENT_TYPE, "text/css")], body) };
    let app = axum::Router::new()
        .route(
            "/",
            get(move || {
                let index = index.clone();
                async move { ([(CONTENT_TYPE, "text/html; charset=windows-1252")], index) }
            }),
        )
        .route(
            "/about.html",
            get(|| async {
                Html(
                    r#"<link rel=stylesheet href="/css/site.css"><a href="./">Home</a><a href="docs/guide?id=2">Guide</a>"#,
                )
            }),
        )
        .route(
            "/docs/guide",
            get(|| async {
                Html(
                    r#"<head><base href="/img/"></head><img src="logo.png"><a href="../about.html">About</a><a href="/">Home</a>"#,
                )
            }),
        )
        .route(
            "/css/site.css",
            get(css(r#"@import url(more.css); body { background: url("../img/bg.png") }"#)),
        )
        .route("/css/more.css", get(css("h1 { color: red }")))
        .route("/img/logo.png", get(png))
        .route("/img/bg.png", get(png));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), away, asked)
}

/// Every `href` and `src` value in a saved page.
fn links_in(html: &str) -> Vec<String> {
    let mut found = Vec::new();
    for attribute in ["href=\"", "src=\""] {
        for (at, _) in html.match_indices(attribute) {
            let rest = &html[at + attribute.len()..];
            found.push(rest[..rest.find('"').unwrap()].to_string());
        }
    }
    found
}

/// Every file under `folder`.
fn files_under(folder: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(files_under(&path));
        } else {
            files.push(path);
        }
    }
    files
}

/// Gaps row F2's close: a three-page site saved, whose saved index names no outside address.
#[tokio::test]
async fn pages_saved_for_offline_reading_link_to_each_other_and_name_no_outside_address() {
    let (base, away, asked) = three_pages().await;
    let folder = tempfile::tempdir().unwrap();
    let search = Search {
        page: format!("{base}/"),
        depth: 1,
        ..Search::default()
    };
    let saved = edm_shell::lists::offline::save_pages(&edm_core::client(), &search, folder.path())
        .await
        .unwrap();
    assert_eq!((saved.pages, saved.resources, saved.stopped), (3, 4, false));
    let site = folder
        .path()
        .join(base.trim_start_matches("http://").replace(':', "_"));
    assert_eq!(saved.index, site.join("index.html"));

    let bytes = std::fs::read(&saved.index).unwrap();
    let index = String::from_utf8_lossy(&bytes).into_owned();
    // Every address in the saved index is a relative path to a file saved beside it, but the one
    // to the other host, left as it was.
    let links = links_in(&index);
    assert_eq!(links.len(), 5, "{links:?}");
    for link in &links {
        if link.starts_with(&away) {
            assert_eq!(*link, format!("{away}/away.html"));
            continue;
        }
        assert!(
            !link.contains("://") && !link.starts_with('/'),
            "{link} in {index}"
        );
        let file = link.split('#').next().unwrap();
        assert!(site.join(file).is_file(), "{link} names no saved file");
    }
    assert!(index.contains(r#"href="about.html""#), "{index}");
    assert!(index.contains("#part\""), "the fragment is kept: {index}");
    assert_eq!(
        index.matches("://").count(),
        1,
        "one outside address, no other: {index}"
    );
    // The charset the server said is written in, and the page's own bytes are as they came.
    assert!(
        index.contains(r#"<meta charset="windows-1252">"#),
        "{index}"
    );
    assert!(bytes.windows(2).any(|w| w == [b'f', 0xE9]));

    // No saved file names the site's own address, and the other host was never asked.
    let files = files_under(folder.path());
    assert_eq!(files.len(), 7, "{files:?}");
    for file in &files {
        let text = String::from_utf8_lossy(&std::fs::read(file).unwrap()).into_owned();
        assert!(
            !text.contains(base.trim_start_matches("http://")),
            "{file:?}: {text}"
        );
    }
    assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 0);

    // The page further in reads its `<base href>`'s picture, and links back up.
    let guide = files
        .iter()
        .find(|f| f.parent().unwrap().ends_with("docs"))
        .unwrap();
    let guide_text = std::fs::read_to_string(guide).unwrap();
    assert!(
        guide_text.contains(r#"<img src="../img/logo.png">"#),
        "{guide_text}"
    );
    assert!(
        guide_text.contains(r#"<a href="../about.html">"#),
        "{guide_text}"
    );
    assert!(
        guide_text.contains(r#"<a href="../index.html">"#),
        "{guide_text}"
    );
    assert!(
        !guide_text.contains("/img/\""),
        "the base is left out: {guide_text}"
    );
    let css = std::fs::read_to_string(site.join("css").join("site.css")).unwrap();
    assert!(
        css.contains("@import url(more.css);") && css.contains("url(../img/bg.png)"),
        "{css}"
    );
}

#[tokio::test]
async fn a_page_past_the_depth_is_not_saved_and_its_link_stays_a_web_address() {
    let (base, _, asked) = three_pages().await;
    let folder = tempfile::tempdir().unwrap();
    let search = Search {
        page: format!("{base}/"),
        depth: 0,
        ..Search::default()
    };
    let saved = edm_shell::lists::offline::save_pages(&edm_core::client(), &search, folder.path())
        .await
        .unwrap();
    assert_eq!((saved.pages, saved.resources), (1, 4));
    let index = String::from_utf8_lossy(&std::fs::read(&saved.index).unwrap()).into_owned();
    assert!(
        index.contains(&format!(r#"href="{base}/about.html""#)),
        "{index}"
    );
    assert!(
        index.contains(&format!(r#"href="{base}/docs/guide?id=2#part""#)),
        "{index}"
    );
    assert!(index.contains(r#"href="css/site.css""#), "{index}");
    assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 0);
}
