// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

#[test]
fn a_file_is_kept_in_folders_that_mirror_its_address() {
    assert_eq!(
        super::layout_folder("https://files.example.org/pub/a%20b/c/file.zip?x=1").as_deref(),
        Some("files.example.org/pub/a b/c")
    );
    assert_eq!(
        super::layout_folder("http://h.example:8080/file.zip").as_deref(),
        Some("h.example_8080")
    );
    assert_eq!(super::layout_folder("magnet:?xt=urn:btih:abc"), None);
    let dotted = super::layout_folder("https://h.example/a/%2e%2e/b/f.zip").unwrap();
    assert!(!dotted.contains(".."), "{dotted}");
}

#[test]
fn tracking_parameters_do_not_make_an_address_new() {
    let a = super::bare("https://v.example/watch?v=1&si=abc&utm_source=x");
    assert_eq!(a, super::bare("https://v.example/watch?v=1"));
    assert_ne!(a, super::bare("https://v.example/watch?v=2"));
    assert_eq!(
        super::bare("https://a.example/f.zip?fbclid=z"),
        super::bare("https://a.example/f.zip")
    );
}

use super::*;

/// An FTP server's folders walked as a site's pages are: down to the
/// depth asked, files kept by type and path, names with spaces made
/// into addresses.
#[tokio::test]
async fn ftp_folders_are_walked_to_their_depth_with_the_filters() {
    use edm_core::ftp::Listed;
    let tree = |folder: String| async move {
        let entry = |name: &str, folder: bool| Listed {
            name: name.into(),
            folder,
        };
        Ok(match folder.as_str() {
            "ftp://host/pub/" => vec![
                entry("a.zip", false),
                entry("notes.txt", false),
                entry("old stuff", true),
                entry("skip", true),
            ],
            "ftp://host/pub/old%20stuff/" => {
                vec![entry("b c.zip", false), entry("deeper", true)]
            }
            "ftp://host/pub/old%20stuff/deeper/" => vec![entry("d.zip", false)],
            "ftp://host/pub/skip/" => vec![entry("e.zip", false)],
            other => panic!("listed {other}"),
        })
    };
    let search = Search {
        page: "ftp://host/pub".into(),
        types: vec!["zip".into()],
        depth: 1,
        exclude: vec!["skip".into()],
        ..Search::default()
    };
    let found = walk_folders(&search, None, tree).await.unwrap();
    assert_eq!(
        found.files,
        [
            "ftp://host/pub/a.zip",
            "ftp://host/pub/old%20stuff/b%20c.zip"
        ]
    );
    assert_eq!(found.pages, 2);
}

/// A last run in the future (a clock once set ahead)
/// held the search until that date; an interval read from a hand-edited
/// file overflowed when turned into seconds.
#[test]
fn a_saved_search_is_not_held_by_a_future_clock_or_a_huge_interval() {
    let now = 1_800_000_000;
    let every_day = Search {
        every_hours: 24,
        last_run: now - 3600,
        ..Search::default()
    };
    assert!(!every_day.due(now), "ran an hour ago");
    assert!(every_day.due(now + 23 * 3600), "a day since");
    let from_the_future = Search {
        last_run: now + 40 * 365 * 24 * 3600,
        ..every_day.clone()
    };
    assert!(from_the_future.due(now), "a future last run is stale");
    let huge = Search {
        every_hours: u64::MAX,
        last_run: now - MOST_HOURS * 3600,
        ..Search::default()
    };
    assert!(huge.due(now), "clamped to a month, not overflowed");
    assert!(!Search::default().due(now), "by hand only");
}

#[test]
fn a_batch_fills_the_star_with_numbers_or_letters() {
    assert_eq!(
        series("https://e.com/img*.jpg", "8", "11", 2).unwrap(),
        [
            "https://e.com/img08.jpg",
            "https://e.com/img09.jpg",
            "https://e.com/img10.jpg",
            "https://e.com/img11.jpg"
        ]
    );
    assert_eq!(
        series("https://e.com/part-*.zip", "a", "c", 0).unwrap(),
        [
            "https://e.com/part-a.zip",
            "https://e.com/part-b.zip",
            "https://e.com/part-c.zip"
        ]
    );
    use SeriesError::*;
    assert_eq!(series("https://e.com/no-star", "1", "2", 0), Err(Star));
    assert_eq!(series("https://e.com/*/*", "1", "2", 0), Err(Star));
    assert_eq!(series("https://e.com/*", "5", "1", 0), Err(Backwards));
    assert_eq!(series("https://e.com/*", "a", "Z", 0), Err(Ends));
    // Letters backwards say so, and a typed zero pads.
    assert_eq!(series("https://e.com/*", "z", "a", 0), Err(Backwards));
    assert_eq!(
        series("https://e.com/f*", "09", "11", 0).unwrap(),
        [
            "https://e.com/f09",
            "https://e.com/f10",
            "https://e.com/f11"
        ]
    );
    assert_eq!(series("https://e.com/*", "0", "99999", 0), Err(TooMany));
}

#[test]
fn a_list_reads_back_as_it_was_written() {
    let rows = vec![
        Listed {
            url: "https://e.com/a.zip".into(),
            referrer: "https://e.com/page".into(),
            queue: "Nightly".into(),
            note: "the big one".into(),
            name: "mine.zip".into(),
            checksum: format!("sha256:{}", "a".repeat(64)),
        },
        Listed {
            url: "magnet:?xt=urn:btih:abc".into(),
            ..Listed::default()
        },
    ];
    let written = to_list(&rows);
    assert!(written.contains("EDM download list"));
    assert_eq!(from_list(&written), rows);
    assert_eq!(from_list(&to_list(&[])), []);
}

#[test]
fn a_list_file_keeps_only_what_edm_can_fetch() {
    let file = r#"{"format":"EDM download list","version":1,"downloads":[
        {"url":"https://e.com/a.zip","referrer":"https://e.com/"},
        {"url":"javascript:alert(1)"},{"url":"file:///etc/passwd"}]}"#;
    assert_eq!(
        from_list(file),
        [Listed {
            url: "https://e.com/a.zip".into(),
            referrer: "https://e.com/".into(),
            ..Listed::default()
        }]
    );
}

#[test]
fn plain_text_with_links_reads_too() {
    let plain = "https://e.com/1.iso\n\nnot a link\nhttps://e.com/2.iso ftp://e.com/3.iso\n";
    assert_eq!(from_list(plain).len(), 3);
    assert!(from_list("").is_empty());
    // JSON that is not EDM's list is read for its links, not as a list.
    assert!(from_list(r#"{"format":"other","downloads":[]}"#).is_empty());
}

#[test]
fn a_link_in_prose_is_read_without_the_punctuation_around_it() {
    /* "see https://e.com/a.zip, then…" was queued as `a.zip,`, which
     * the server does not have. */
    let prose = "Get https://e.com/a.zip, then <https://e.com/b.zip>. \
        (\"https://e.com/c.zip\"); also https://e.com/d.zip) and \
        https://en.wikipedia.org/wiki/Rust_(programming_language). \
        https://e.com/q?x=[1]]";
    let urls: Vec<String> = from_list(prose).into_iter().map(|l| l.url).collect();
    assert_eq!(
        urls,
        [
            "https://e.com/a.zip",
            "https://e.com/b.zip",
            "https://e.com/c.zip",
            "https://e.com/d.zip",
            "https://en.wikipedia.org/wiki/Rust_(programming_language)",
            "https://e.com/q?x=[1]",
        ]
    );
}

#[test]
fn a_list_written_before_rows_kept_more_than_an_address_still_reads() {
    let old = r#"{"format":"EDM download list","version":1,"downloads":[
        {"url":"https://e.com/a.zip","referrer":"https://e.com/"}]}"#;
    let read = from_list(old);
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].referrer, "https://e.com/");
    assert!(read[0].queue.is_empty() && read[0].name.is_empty());
}

#[test]
fn the_site_file_finder_finds_links_and_files_on_a_page() {
    let page = r#"<a href="/files/a.zip">a</a> <A HREF='b.ZIP'>b</A>
        <img src="pic.png"> <a href=c.zip#x>c</a> <a href="mailto:x@e.com">m</a>
        <a href="/files/a.zip">again</a> <a href="?q=1&amp;r=2">q</a>"#;
    let found = links_on_page("https://e.com/dir/index.html", page);
    assert_eq!(
        found,
        [
            "https://e.com/files/a.zip",
            "https://e.com/dir/b.ZIP",
            "https://e.com/dir/c.zip",
            "https://e.com/dir/index.html?q=1&r=2",
            "https://e.com/dir/pic.png",
        ]
    );
    let zips: Vec<_> = found
        .iter()
        .filter(|l| has_extension(l, &["zip".into()]))
        .collect();
    assert_eq!(zips.len(), 3);
    assert!(has_extension("https://e.com/x", &[]));
}
