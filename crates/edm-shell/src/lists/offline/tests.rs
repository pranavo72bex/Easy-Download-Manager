// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The link rewriting and the names of "Save pages for offline reading" (gaps row F2).

use url::Url;

use super::markup::{addresses, css_addresses, relative, rewrite_css, rewrite_html, Kind};
use super::names::Names;

fn parts(path: &str) -> Vec<String> {
    path.split('/').map(String::from).collect()
}

/// A save of `site.example`'s `/`, `/about.html`, `/docs/guide?id=2`, `/css/site.css` and
/// `/img/logo.png`, its links made from `from` (a saved file's path).
fn saved_from(from: &'static str) -> impl Fn(&Url) -> Option<String> {
    move |link: &Url| {
        let to = match (link.path(), link.query()) {
            ("/", None) => "site.example/index.html",
            ("/about.html", None) => "site.example/about.html",
            ("/docs/guide", Some("id=2")) => "site.example/docs/guide-x.html",
            ("/css/site.css", None) => "site.example/css/site.css",
            ("/img/logo.png", None) => "site.example/img/logo.png",
            _ => return None,
        };
        (link.host_str() == Some("site.example")).then(|| relative(&parts(from), &parts(to)))
    }
}

fn page(path: &str) -> Url {
    Url::parse(&format!("https://site.example{path}")).unwrap()
}

fn rewritten(html: &str, at: &str, from: &'static str) -> String {
    String::from_utf8(rewrite_html(
        html.as_bytes(),
        &page(at),
        None,
        &saved_from(from),
    ))
    .unwrap()
}

#[test]
fn a_link_between_saved_files_is_a_relative_path() {
    let p = |s: &str| parts(s);
    assert_eq!(
        relative(&p("h/index.html"), &p("h/about.html")),
        "about.html"
    );
    assert_eq!(
        relative(&p("h/docs/a/b.html"), &p("h/index.html")),
        "../../index.html"
    );
    assert_eq!(
        relative(&p("h/docs/b.html"), &p("h/docs/img/x.png")),
        "img/x.png"
    );
    assert_eq!(
        relative(&p("h/index.html"), &p("other/x.css")),
        "../other/x.css"
    );
    assert_eq!(
        relative(&p("h/index.html"), &p("h/index.html")),
        "index.html"
    );
    // A name the disk allows but a link cannot hold as it is: percent-encoded.
    assert_eq!(
        relative(&p("h/index.html"), &p("h/a b#c.html")),
        "a%20b%23c.html"
    );
    assert_eq!(
        relative(&p("h/index.html"), &p("h/ré.html")),
        "r%C3%A9.html"
    );
}

#[test]
fn saved_pages_and_files_are_linked_relatively_and_others_made_absolute() {
    let html = r##"<a href="about.html">A</a> <a href='/docs/guide?id=2#part'>G</a>
<link rel="stylesheet" href="css/site.css"><img src=img/logo.png alt="">
<a href="/not-saved.html">N</a> <a href="https://away.example/x">Away</a>
<a href="#top">Top</a> <a href="mailto:a@site.example">Mail</a> <img src="data:image/png;base64,AA==">
<a href="//cdn.example/lib.js">CDN</a> <a href="?page=2">Next</a>"##;
    let out = rewritten(html, "/", "site.example/index.html");
    assert!(out.contains(r#"<a href="about.html">"#), "{out}");
    assert!(
        out.contains(r#"<a href="docs/guide-x.html#part">"#),
        "{out}"
    );
    assert!(out.contains(r#"href="css/site.css""#), "{out}");
    assert!(out.contains(r#"<img src="img/logo.png" alt="">"#), "{out}");
    assert!(
        out.contains(r#"<a href="https://site.example/not-saved.html">"#),
        "{out}"
    );
    assert!(
        out.contains(r#"<a href="https://away.example/x">"#),
        "{out}"
    );
    assert!(out.contains(r##"<a href="#top">"##), "{out}");
    assert!(out.contains(r#"href="mailto:a@site.example""#), "{out}");
    assert!(out.contains(r#"src="data:image/png;base64,AA==""#), "{out}");
    assert!(
        out.contains(r#"<a href="https://cdn.example/lib.js">"#),
        "{out}"
    );
    assert!(
        out.contains(r#"<a href="https://site.example/?page=2">"#),
        "{out}"
    );
}

#[test]
fn a_page_further_in_links_back_up_the_folders() {
    let out = rewritten(
        r#"<a href="../">Home</a><a href="../about.html#team">About</a><img src="/img/logo.png">"#,
        "/docs/guide?id=2",
        "site.example/docs/guide-x.html",
    );
    assert_eq!(
        out,
        r#"<a href="../index.html">Home</a><a href="../about.html#team">About</a><img src="../img/logo.png">"#
    );
}

#[test]
fn a_base_href_is_what_links_are_read_against_and_is_left_out() {
    let html = r#"<head><base href="/img/" target="_self"></head><img src="logo.png"><a href="../about.html">A</a><a href="x.html">X</a>"#;
    let found = addresses(html.as_bytes(), &page("/docs/guide?id=2"));
    assert!(
        found.contains(&(page("/img/logo.png"), Kind::Resource)),
        "{found:?}"
    );
    assert!(
        found.contains(&(page("/about.html"), Kind::Page)),
        "{found:?}"
    );
    let out = rewritten(html, "/docs/guide?id=2", "site.example/docs/guide-x.html");
    assert!(!out.contains("/img/\""), "the base is gone: {out}");
    assert!(out.contains(r#"target="_self""#), "{out}");
    assert!(out.contains(r#"<img src="../img/logo.png">"#), "{out}");
    assert!(out.contains(r#"<a href="../about.html">"#), "{out}");
    assert!(
        out.contains(r#"<a href="https://site.example/img/x.html">"#),
        "{out}"
    );
}

#[test]
fn a_query_is_part_of_what_is_saved_and_ampersands_stay_escaped() {
    let html = r#"<a href="/docs/guide?id=2">G</a><a href="/list?a=1&amp;b=2">L</a>"#;
    let out = rewritten(html, "/", "site.example/index.html");
    assert!(out.contains(r#"<a href="docs/guide-x.html">"#), "{out}");
    assert!(
        out.contains(r#"<a href="https://site.example/list?a=1&amp;b=2">"#),
        "{out}"
    );
}

#[test]
fn stylesheets_urls_and_imports_are_found_and_rewritten() {
    let css = br#"@import "site.css"; @import url('/img/logo.png');
/* url(commented.png) */ body { background: url( ../img/logo.png ) } .x{background:URL("missing.png")}
.y{content:"url(not-a-link.png)"} .z{background:url(data:image/png;base64,AA==)} .w{background:url("https://away.example/a(1).png")}"#;
    let base = page("/css/site.css");
    let found = css_addresses(css, &base);
    assert_eq!(
        found,
        vec![
            page("/css/site.css"),
            page("/img/logo.png"),
            page("/css/missing.png"),
            Url::parse("https://away.example/a(1).png").unwrap(),
        ]
    );
    let out = String::from_utf8(rewrite_css(
        css,
        &base,
        &saved_from("site.example/css/site.css"),
    ))
    .unwrap();
    assert!(
        out.starts_with("@import url(site.css); @import url(../img/logo.png);"),
        "{out}"
    );
    assert!(out.contains("/* url(commented.png) */"), "{out}");
    assert!(out.contains("background: url(../img/logo.png) }"), "{out}");
    assert!(
        out.contains("background:url(https://site.example/css/missing.png)"),
        "{out}"
    );
    assert!(out.contains(r#"content:"url(not-a-link.png)""#), "{out}");
    assert!(out.contains("url(data:image/png;base64,AA==)"), "{out}");
    assert!(
        out.contains(r#"url("https://away.example/a(1).png")"#),
        "an absolute address not saved is left as it was: {out}"
    );
}

#[test]
fn styles_in_a_page_srcsets_and_refreshes_are_rewritten() {
    let html = r#"<style>body{background:url("img/logo.png")}</style>
<div style="background: url(&quot;/img/logo.png&quot;)"></div>
<img srcset="img/logo.png 1x, /img/big.png 2x" src="img/logo.png">
<meta http-equiv="refresh" content="5; url=/about.html">"#;
    let found = addresses(html.as_bytes(), &page("/"));
    assert!(
        found.contains(&(page("/img/big.png"), Kind::Resource)),
        "{found:?}"
    );
    assert!(
        found.contains(&(page("/about.html"), Kind::Elsewhere)),
        "{found:?}"
    );
    let out = rewritten(html, "/", "site.example/index.html");
    assert!(
        out.contains("<style>body{background:url(img/logo.png)}</style>"),
        "{out}"
    );
    assert!(
        out.contains(r#"style="background: url(img/logo.png)""#),
        "{out}"
    );
    assert!(
        out.contains(r#"srcset="img/logo.png 1x, https://site.example/img/big.png 2x""#),
        "{out}"
    );
    assert!(out.contains(r#"content="5; url=about.html""#), "{out}");
}

#[test]
fn scripts_and_comments_are_not_read_as_markup() {
    let html = r#"<script>var s = '<img src="/img/logo.png">';</script><!-- <a href="about.html"> --><textarea><a href="x.html"></textarea>"#;
    assert!(addresses(html.as_bytes(), &page("/")).is_empty());
    assert_eq!(rewritten(html, "/", "site.example/index.html"), html);
}

#[test]
fn an_integrity_check_goes_with_a_file_now_read_from_the_disk() {
    let html = r#"<link rel="stylesheet" href="/css/site.css" integrity="sha384-x" crossorigin><script src="https://cdn.example/a.js" integrity="sha384-y"></script>"#;
    let out = rewritten(html, "/", "site.example/index.html");
    assert!(
        out.contains(r#"<link rel="stylesheet" href="css/site.css"  crossorigin>"#),
        "{out}"
    );
    assert!(
        out.contains(r#"integrity="sha384-y""#),
        "kept where the file is not saved: {out}"
    );
}

#[test]
fn the_charset_the_server_said_is_kept_and_every_other_byte_too() {
    let mut html = b"<!DOCTYPE html><html><head><title>Caf".to_vec();
    html.push(0xE9);
    html.extend_from_slice(b"</title></head><a href=\"about.html\">x</a>");
    let out = rewrite_html(
        &html,
        &page("/"),
        Some("windows-1252"),
        &saved_from("site.example/index.html"),
    );
    let mut wanted =
        b"<!DOCTYPE html><html><head><meta charset=\"windows-1252\"><title>Caf".to_vec();
    wanted.push(0xE9);
    wanted.extend_from_slice(b"</title></head><a href=\"about.html\">x</a>");
    assert_eq!(out, wanted);
    // A page that says its own charset is not told again.
    let own = br#"<head><meta charset="utf-8"></head>"#;
    assert_eq!(
        rewrite_html(
            own,
            &page("/"),
            Some("utf-8"),
            &saved_from("site.example/index.html")
        ),
        own
    );
}

#[test]
fn names_mirror_the_site_are_safe_and_never_given_twice() {
    let mut names = Names::default();
    let mut place = |address: &str, page: bool, mime: &str| {
        names
            .place(&Url::parse(address).unwrap(), page, mime)
            .join("/")
    };
    assert_eq!(
        place("http://h.example:8080/", true, "text/html"),
        "h.example_8080/index.html"
    );
    assert_eq!(
        place("https://h.example/docs/", true, "text/html"),
        "h.example/docs/index.html"
    );
    assert_eq!(
        place("https://h.example/about", true, "text/html"),
        "h.example/about.html"
    );
    // `about.html` is a page of its own: the second is numbered, not written over the first,
    // and a name that differs only in case is the same name on most disks.
    assert_eq!(
        place("https://h.example/about.html", true, "text/html"),
        "h.example/about-2.html"
    );
    assert_eq!(
        place("https://h.example/About.html", true, "text/html"),
        "h.example/About-3.html"
    );
    assert_eq!(
        place("https://h.example/list.php", true, "text/html"),
        "h.example/list.php.html"
    );
    let two = place("https://h.example/list.php?page=2", true, "text/html");
    let three = place("https://h.example/list.php?page=3", true, "text/html");
    assert!(
        two.starts_with("h.example/list.php-") && two.ends_with(".html"),
        "{two}"
    );
    assert_ne!(two, three);
    let css = place("https://h.example/style?v=1", false, "text/css");
    assert!(css.ends_with(".css"), "{css}");
    assert_eq!(
        place("https://h.example/pic", false, "image/png"),
        "h.example/pic.png"
    );
    assert_eq!(
        place("https://h.example/a.png", false, "image/jpeg"),
        "h.example/a.png"
    );
    // Nothing leaves the folder, and the disk is given nothing it refuses.
    let dots = place(
        "https://h.example/a/%2e%2e/%2e%2e/b/c%3Ad.png",
        false,
        "image/png",
    );
    assert!(
        !dots.split('/').any(|p| p == ".." || p.contains(':')),
        "{dots}"
    );
    assert_ne!(
        place("https://h.example/con", true, "text/html"),
        "h.example/con.html"
    );
    // A file where a folder is wanted, or a folder where a file is: neither is written over.
    assert_eq!(
        place("https://h.example/f", false, "image/png"),
        "h.example/f.png"
    );
    let under_a_file = place("https://h.example/f.png/x.png", false, "image/png");
    assert!(
        under_a_file.starts_with("h.example/") && under_a_file.matches('/').count() == 1,
        "{under_a_file}"
    );
    assert_eq!(
        place("https://h.example/docs", false, ""),
        "h.example/docs-2"
    );
    // Too deep for a disk: straight in the site's folder.
    let d = "d".repeat(150);
    let deep = place(
        &format!("https://h.example/{d}/{d}/{d}/x.png"),
        false,
        "image/png",
    );
    assert!(deep.len() <= 120 && deep.ends_with("-x.png"), "{deep}");
}
