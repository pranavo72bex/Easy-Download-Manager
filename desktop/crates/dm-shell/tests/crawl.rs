//! "Files on a site" past the first page: how deep it reads, what it keeps
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
        .route("/p/:n", get(|| async { Html(r#"<a href="/x.zip">x</a>"#) }));
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
