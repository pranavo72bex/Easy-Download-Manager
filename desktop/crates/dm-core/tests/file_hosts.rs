// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A file-storage service's public share address lands the file itself, under the name the
//! service gives it (gaps row W7), against a stand-in shaped like each service's answers as
//! they were looked at signed out on 2026-11-18: Dropbox's `dl=0` page and `dl=1` redirect,
//! Google Drive's download address and its "can't scan this file for viruses" question for a
//! large file, and MediaFire's file page with its Download button. Every other address is
//! fetched as before.
//!
//! One test in this binary: the stand-in is named by an environment variable, which is the
//! process's.

mod common;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use common::{options, serve, tempdir};

const REPORT: &[u8] = b"%PDF-1.4 a report shared from Dropbox\n";
const SMALL: &[u8] = b"a small file, which Drive scans and serves at once\n";
const PANEL: &[u8] = b"PK\x03\x04 a zip shared from MediaFire\n";

/// What a page about a file looks like: never what should land.
const SHARE_PAGE: &str = "<!doctype html><title>Shared with you</title><p>Download</p>";

/// Drive's question for a file too large to scan, as Drive asks it, its styles and scripts
/// left out.
const DRIVE_QUESTION: &str = r#"<!DOCTYPE html><html><head>
<title>Google Drive - Virus scan warning</title></head><body>
<div class="uc-main"><div id="uc-text">
<p class="uc-warning-caption">Google Drive can't scan this file for viruses.</p>
<p class="uc-warning-subcaption"><span class="uc-name-size">
<a href="/open?id=BigFile-2">model.npz</a> (1M)</span> is too large for Google to scan for
viruses. Would you still like to download this file?</p>
<form id="download-form" action="https://drive.usercontent.google.com/download" method="get">
<input type="submit" id="uc-download-link" class="goog-inline-block jfk-button jfk-button-action"
 value="Download anyway"/>
<input type="hidden" name="id" value="BigFile-2">
<input type="hidden" name="export" value="download">
<input type="hidden" name="confirm" value="t">
<input type="hidden" name="uuid" value="6c2a-uuid">
</form></div></div></body></html>"#;

/// The share addresses given to dm, as a person copies them from each service.
const DROPBOX: &str = "https://www.dropbox.com/scl/fi/r3p0rt/report.pdf?rlkey=k3y&dl=0";
const DRIVE_SMALL: &str = "https://drive.google.com/file/d/SmallFile_1/view?usp=sharing";
const DRIVE_BIG: &str = "https://drive.google.com/file/d/BigFile-2/view?usp=drive_link";
const DRIVE_BIG_UC: &str = "https://drive.google.com/uc?id=BigFile-2&export=download";
const MEDIAFIRE: &str = "https://www.mediafire.com/file/0wpllm9h07b60b2/Panel.zip/file";

type Asked = Query<HashMap<String, String>>;

fn big() -> Vec<u8> {
    (0..300_000u32).flat_map(|n| n.to_le_bytes()).collect()
}

#[derive(Clone)]
struct Site {
    addr: Arc<Mutex<Option<SocketAddr>>>,
    asked: Arc<Mutex<Vec<String>>>,
}

impl Site {
    fn origin(&self) -> String {
        format!("http://{}", self.addr.lock().unwrap().unwrap())
    }
    fn saw(&self, what: String) {
        self.asked.lock().unwrap().push(what);
    }
}

fn page(body: String) -> Response {
    let kind = [(header::CONTENT_TYPE, "text/html; charset=utf-8")];
    (kind, body).into_response()
}

fn file(name: &str, body: Vec<u8>) -> Response {
    let encoded = name.replace(' ', "%20");
    let disposition = format!("attachment; filename=\"{name}\"; filename*=UTF-8''{encoded}");
    let headers = [
        (header::CONTENT_TYPE, "application/octet-stream".to_string()),
        (header::CONTENT_DISPOSITION, disposition),
    ];
    (headers, body).into_response()
}

fn found(to: &'static str) -> Response {
    let to = [(header::LOCATION, to)];
    (StatusCode::FOUND, to).into_response()
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "no such file").into_response()
}

/// Dropbox: the share page at `dl=0`, a redirect to the content server at `dl=1`.
async fn dropbox(State(site): State<Site>, Query(q): Asked) -> Response {
    site.saw(format!("dropbox dl={:?}", q.get("dl")));
    if q.get("rlkey").map(String::as_str) != Some("k3y") {
        return not_found();
    }
    match q.get("dl").map(String::as_str) {
        Some("1") => found("/cd/0/get/AbC/file?dl=1"),
        _ => page(SHARE_PAGE.into()),
    }
}

/// Drive's download address: a small file at once; a large one asks first, and is served
/// only to the form's answer.
async fn drive(State(site): State<Site>, Query(q): Asked) -> Response {
    let said = |key: &str| q.get(key).map(String::as_str);
    site.saw(format!(
        "drive id={:?} confirm={:?}",
        said("id"),
        said("confirm")
    ));
    match said("id") {
        Some("SmallFile_1") => file("small notes.txt", SMALL.to_vec()),
        Some("BigFile-2") => {
            let answered = said("confirm") == Some("t")
                && said("uuid") == Some("6c2a-uuid")
                && said("export") == Some("download");
            if answered {
                file("model.npz", big())
            } else {
                page(DRIVE_QUESTION.into())
            }
        }
        _ => not_found(),
    }
}

/// MediaFire's file page, its Download button pointing at a download server.
async fn mediafire(State(site): State<Site>) -> Response {
    site.saw("mediafire page".into());
    let origin = site.origin();
    page(format!(
        r#"<html><head><title>Panel.zip</title></head><body>
<a class="input popsok"
           aria-label="Download file"
           href="{origin}/1owwn/0wpllm9h07b60b2/Panel.zip"
           id="downloadButton"
           rel="nofollow">
                Download (656.28KB)
        </a>
<a class="retry"
 href="https://www.mediafire.com/download_repair.php?qkey=0wpllm9h07b60b2&amp;dkey=1">
<span>Repair</span></a></body></html>"#
    ))
}

/// `share` is told to Download details as its file, `name` and `body`'s size, and lands as
/// that file under that name.
async fn lands(client: &dm_core::reqwest::Client, share: &str, name: &str, body: &[u8]) {
    let probed = dm_core::probe(client, share, None, None)
        .await
        .unwrap_or_else(|e| panic!("{share}: {e}"));
    assert_eq!(probed.filename, name, "{share}");
    assert_eq!(probed.size, Some(body.len() as u64), "{share}");
    let kind = probed.content_type.as_deref();
    assert!(
        !dm_core::videopage::is_page(kind),
        "{share}: probed as a page"
    );

    let dir = tempdir();
    let done = dm_core::download(client, share, &options(dir.path()))
        .await
        .unwrap_or_else(|e| panic!("{share}: {e}"));
    let landed = done.path.file_name().unwrap().to_string_lossy();
    assert_eq!(landed, name, "{share}");
    assert_eq!(std::fs::read(&done.path).unwrap(), body, "{share}");
}

#[tokio::test(flavor = "multi_thread")]
async fn share_addresses_land_their_files_under_their_own_names() {
    let site = Site {
        addr: Arc::new(Mutex::new(None)),
        asked: Arc::new(Mutex::new(Vec::new())),
    };
    let report = || async { file("report.pdf", REPORT.to_vec()) };
    let panel = || async { file("Panel.zip", PANEL.to_vec()) };
    let share_page = || async { page(SHARE_PAGE.into()) };
    let app = Router::new()
        .route("/scl/fi/r3p0rt/report.pdf", get(dropbox))
        .route("/cd/0/get/AbC/file", get(report))
        .route("/file/d/{id}/view", get(share_page))
        .route("/download", get(drive))
        .route("/file/0wpllm9h07b60b2/Panel.zip/file", get(mediafire))
        .route("/1owwn/0wpllm9h07b60b2/Panel.zip", get(panel))
        .route("/watch", get(share_page))
        .with_state(site.clone());
    let addr = serve(app).await;
    *site.addr.lock().unwrap() = Some(addr);
    std::env::set_var("DM_FILE_HOST_STAND_IN", format!("http://{addr}"));
    let client = dm_core::client();

    lands(&client, DROPBOX, "report.pdf", REPORT).await;
    lands(&client, DRIVE_SMALL, "small notes.txt", SMALL).await;
    lands(&client, DRIVE_BIG, "model.npz", &big()).await;
    lands(&client, DRIVE_BIG_UC, "model.npz", &big()).await;
    lands(&client, MEDIAFIRE, "Panel.zip", PANEL).await;

    let asked = site.asked.lock().unwrap().clone();
    // Dropbox's page was never fetched, only its file; Drive's question was answered.
    let dropbox_page = "dropbox dl=Some(\"0\")";
    assert!(!asked.iter().any(|a| a == dropbox_page), "{asked:?}");
    let answered = "drive id=Some(\"BigFile-2\") confirm=Some(\"t\")";
    assert!(asked.iter().any(|a| a == answered), "{asked:?}");

    // Every other address as before: a page is still a page, asked about rather than saved
    // (contract row B40), with the stand-in named.
    let dir = tempdir();
    let watch = format!("http://{addr}/watch");
    let err = dm_core::download(&client, &watch, &options(dir.path()))
        .await
        .unwrap_err();
    let said = err.to_string();
    assert!(said.contains("web page rather than the file"), "{said}");
    std::env::remove_var("DM_FILE_HOST_STAND_IN");
}
