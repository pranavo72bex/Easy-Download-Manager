// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use dm_core::ftp::Listed as Entry;

fn file(name: &str) -> Entry {
    Entry {
        name: name.into(),
        folder: false,
    }
}

fn folder(name: &str) -> Entry {
    Entry {
        name: name.into(),
        folder: true,
    }
}

/// A local site: a start page linking to pages that answer at once, not
/// at all, and with an error, and to two files.
async fn site() -> std::net::SocketAddr {
    use axum::http::header::CONTENT_TYPE;
    let html = |body: &'static str| async move { ([(CONTENT_TYPE, "text/html")], body) };
    let app = axum::Router::new()
        .route(
            "/",
            axum::routing::get(move || {
                html(
                    r#"<a href="/fine/">f</a> <a href="/missing/">m</a>
                       <a href="/slow/">s</a> <a href="a.zip">a</a>"#,
                )
            }),
        )
        .route(
            "/fine/",
            axum::routing::get(move || html(r#"<a href="/b.zip">b</a>"#)),
        )
        .route(
            "/slow/",
            axum::routing::get(|| async {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                "never"
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

#[test]
fn each_series_error_says_its_sentence() {
    for (error, said) in [
        (SeriesError::Star, "exactly one *"),
        (SeriesError::Backwards, "runs backwards"),
        (SeriesError::TooMany, "Ten thousand"),
        (SeriesError::Ends, "both be numbers"),
    ] {
        assert!(error.english().contains(said), "{error:?}");
        assert_eq!(error.to_string(), error.english());
    }
    assert_eq!(series("a/*.jpg", "3", "1", 0), Err(SeriesError::Backwards));
}

#[test]
fn a_list_in_some_other_json_format_is_read_as_text() {
    let other = r#"{"format":"not-dm","version":1,"downloads":[{"url":"https://e.com/x.zip"}]}"#;
    let read = from_list(other);
    assert!(
        read.iter().all(|row| row.url != "https://e.com/x.zip"),
        "{read:?}"
    );
    let mine = to_list(&[Listed {
        url: "https://e.com/x.zip".into(),
        ..Listed::default()
    }]);
    assert_eq!(from_list(&mine)[0].url, "https://e.com/x.zip");
}

#[test]
fn a_page_with_href_as_a_word_or_a_bad_base_gives_no_links() {
    assert!(links_on_page("not a url", r#"<a href="x.zip">"#).is_empty());
    let found = links_on_page(
        "https://e.com/",
        r#"the href attribute <a href = "y.zip">y</a> src: none"#,
    );
    assert_eq!(found, ["https://e.com/y.zip"]);
}

#[test]
fn an_address_that_does_not_parse_is_kept_as_it_was() {
    assert_eq!(bare("not a url"), "not a url");
    // A made-up login, put together here (the privacy check refuses one written out).
    let login = ["u", "p"].join(":");
    assert_eq!(bare_of_login("not a url"), "not a url");
    assert_eq!(bare_of_login(&format!("ftp://{login}@h/x")), "ftp://h/x");
    let start = url::Url::parse(&format!("https://{login}@e.com/")).unwrap();
    assert_eq!(with_start_login("not a url", &start), "not a url");
    assert_eq!(
        with_start_login("https://e.com/next/", &start),
        format!("https://{login}@e.com/next/")
    );
    assert_eq!(
        with_start_login("https://other.com/", &start),
        "https://other.com/"
    );
}

#[tokio::test]
async fn site_files_grabs_the_wanted_files_on_one_page() {
    let addr = site().await;
    let found = grab(&format!("http://{addr}/"), &["zip".into()])
        .await
        .unwrap();
    assert_eq!(found, [format!("http://{addr}/a.zip")]);
    assert!(grab("http://127.0.0.1:9/", &[]).await.is_err());
}

#[tokio::test]
async fn a_crawl_skips_a_page_that_fails_and_stops_at_its_deadline() {
    let addr = site().await;
    let search = Search {
        page: format!("http://{addr}/"),
        types: vec!["zip".into()],
        depth: 1,
        ..Search::default()
    };
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(800);
    let found = crawl_until(&dm_core::client(), &search, Some(deadline))
        .await
        .unwrap();
    assert_eq!(
        found.files,
        [
            format!("http://{addr}/a.zip"),
            format!("http://{addr}/b.zip")
        ]
    );
    assert_eq!(found.pages, 2, "the start and the fine page");
    assert!(found.stopped, "the slow page ran past the deadline");
}

#[tokio::test]
async fn an_ftp_search_without_the_engines_settings_is_refused() {
    let search = Search {
        page: "ftp://127.0.0.1:9/pub/".into(),
        ..Search::default()
    };
    let why = crawl_with(&dm_core::client(), &search, None)
        .await
        .unwrap_err();
    assert!(why.contains("dm's settings"), "{why}");
}

#[tokio::test]
async fn ftp_folders_that_fail_are_skipped_and_the_first_is_the_error() {
    let search = Search {
        page: "ftp://host/pub/".into(),
        types: Vec::new(),
        depth: 2,
        include: vec!["keep".into()],
        ..Search::default()
    };
    let none = walk_folders(&search, None, |_| async {
        Err::<Vec<Entry>, _>(dm_core::Error::Changed)
    })
    .await
    .unwrap_err();
    assert!(none.contains("changed on the server"), "{none}");

    let found = walk_folders(&search, None, |at: String| async move {
        match at.as_str() {
            "ftp://host/pub/" => Ok(vec![folder("gone"), file("keep.bin"), file("x.bin")]),
            _ => Err(dm_core::Error::Changed),
        }
    })
    .await
    .unwrap();
    assert_eq!(found.files, ["ftp://host/pub/keep.bin"], "include kept one");
    assert_eq!(found.pages, 1, "the failed folder is not counted");
}

#[tokio::test]
async fn an_ftp_walk_stops_at_its_deadline_and_at_most_pages() {
    let search = Search {
        page: "ftp://host/pub".into(),
        types: Vec::new(),
        depth: 1,
        ..Search::default()
    };
    // The start folder is listed however slow; the next is past the deadline.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(100);
    let found = walk_folders(&search, Some(deadline), |at: String| async move {
        if at == "ftp://host/pub/" {
            Ok(vec![folder("fast"), folder("slow"), file("a")])
        } else if at.ends_with("/fast/") {
            Ok(vec![file("b")])
        } else {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            Ok(Vec::new())
        }
    })
    .await
    .unwrap();
    assert!(found.stopped);
    assert_eq!(found.files, ["ftp://host/pub/a", "ftp://host/pub/fast/b"]);

    // A tree wider than a search reads stops at MAX_PAGES folders.
    let wide = Search { depth: 1, ..search };
    let found = walk_folders(&wide, None, |at: String| async move {
        if at == "ftp://host/pub/" {
            Ok((0..MAX_PAGES + 10)
                .map(|i| folder(&format!("f{i}")))
                .collect())
        } else {
            Ok(Vec::new())
        }
    })
    .await
    .unwrap();
    assert_eq!(found.pages, MAX_PAGES);
}
