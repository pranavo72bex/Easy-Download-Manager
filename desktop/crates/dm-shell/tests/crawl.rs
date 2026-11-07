// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! "Site files" past the first page: how deep it reads, what it keeps
//! and skips, and where it stops.

use axum::response::Html;
use axum::routing::get;
use dm_shell::lists::{crawl, Search, MAX_PAGES};

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
    let client = dm_core::client();

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
    let client = dm_core::client();
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
        &dm_core::client(),
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
    let client = dm_core::client();
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
        &dm_core::client(),
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
    let found = dm_shell::lists::crawl_until(&dm_core::client(), &search, Some(deadline))
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
    let client = dm_core::client();

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
    let client = dm_core::client();

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
    let found = crawl(&dm_core::client(), &search).await.unwrap();
    let mut kept = names(&found.files);
    kept.sort_unstable();
    assert_eq!(kept, ["mid.zip", "unsaid.zip"]);
}
