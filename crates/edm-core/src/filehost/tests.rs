// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// Drive's question as it answered a public 476 MB file signed out on 2026-11-18, its styles
/// and scripts left out, a line to a tag.
const DRIVE_QUESTION: &str = r#"<!DOCTYPE html><html><head>
<title>Google Drive - Virus scan warning</title></head><body>
<div class="uc-main"><div id="uc-text">
<p class="uc-warning-caption">Google Drive can't scan this file for viruses.</p>
<p class="uc-warning-subcaption"><span class="uc-name-size">
<a href="/open?id=FILE_ID-1">model.npz</a> (476M)</span> is too large for Google to scan for
viruses. Would you still like to download this file?</p>
<form id="download-form" action="https://drive.usercontent.google.com/download" method="get">
<input type="submit" id="uc-download-link" class="goog-inline-block jfk-button jfk-button-action"
 value="Download anyway"/>
<input type="hidden" name="id" value="FILE_ID-1">
<input type="hidden" name="export" value="download">
<input type="hidden" name="confirm" value="t">
<input type="hidden" name="uuid" value="b6280b25-032d-4a6d-8d9d-ed084e52459f">
</form></div></div></body></html>"#;

/// The address Drive's question's form sends to.
const DRIVE_ANSWER: &str = concat!(
    "https://drive.usercontent.google.com/download?id=FILE_ID-1&export=download&confirm=t",
    "&uuid=b6280b25-032d-4a6d-8d9d-ed084e52459f"
);

/// MediaFire's Download button as its file page had it on 2026-11-18.
const MEDIAFIRE_PAGE: &str = r#"<html><head><title>Panel.zip</title></head><body>
<a class="input popsok"
           aria-label="Download file"
           href="https://download1507.mediafire.com/1owwn/0wpllm9h07b60b2/Panel.zip"
           id="downloadButton"
           rel="nofollow">
                Download (656.28KB)
        </a>
<a class="retry"
 href="https://www.mediafire.com/download_repair.php?qkey=0wpllm9h07b60b2&amp;dkey=1owwn">
<span>Repair</span></a>
</body></html>"#;

/// The file MediaFire's button names.
const MEDIAFIRE_FILE: &str = "https://download1507.mediafire.com/1owwn/0wpllm9h07b60b2/Panel.zip";

/// A public Dropbox share address, the new `/scl/fi/` kind with its `rlkey`.
const DROPBOX_SHARE: &str = concat!(
    "https://www.dropbox.com/scl/fi/3o8ooqje4f497npxdeiwg/test.txt",
    "?rlkey=x9jsd8u7k147x6fzc7stxozqe&dl=0"
);

/// [`DROPBOX_SHARE`]'s file.
const DROPBOX_FILE: &str = concat!(
    "https://www.dropbox.com/scl/fi/3o8ooqje4f497npxdeiwg/test.txt",
    "?rlkey=x9jsd8u7k147x6fzc7stxozqe&dl=1"
);

/// A public Drive file's id.
const DRIVE_ID: &str = "1l_5RK28JRL19wpT22B-DY9We3TVXnnQQ";

#[test]
fn every_other_address_is_left_as_it_is() {
    for url in [
        "https://example.com/file.zip",
        "https://example.com/s/abc?dl=0",
        "https://www.dropbox.com/home",
        "https://www.dropbox.com/",
        "https://dropbox.com.example.net/s/abc/a.zip?dl=0",
        "https://drive.google.com/drive/folders/15uNXeRBIhVvZJIhL4yTw4IsStMhUaaxl",
        "https://drive.google.com/",
        "https://docs.google.com/document/d/abc/edit",
        "https://www.mediafire.com/",
        "https://www.mediafire.com/help",
        "ftp://www.dropbox.com/s/abc/a.zip?dl=0",
        "not an address",
    ] {
        assert_eq!(step_of(url), None, "{url}");
    }
}

#[test]
fn a_dropbox_share_asks_for_its_file() {
    assert_eq!(
        step_of(DROPBOX_SHARE),
        Some(Step::File(DROPBOX_FILE.into()))
    );
    let file = |url: &str| Some(Step::File(url.into()));
    assert_eq!(
        step_of("https://www.dropbox.com/s/abc123/report.pdf"),
        file("https://www.dropbox.com/s/abc123/report.pdf?dl=1")
    );
    // A shared folder: its ZIP.
    assert_eq!(
        step_of("https://www.dropbox.com/sh/0ru09jmk0w9tdnr/AAA-PPON2sYmwUUoGQpBQh1Ia?dl=0"),
        file("https://www.dropbox.com/sh/0ru09jmk0w9tdnr/AAA-PPON2sYmwUUoGQpBQh1Ia?dl=1")
    );
    // Already the file.
    assert_eq!(step_of(DROPBOX_FILE), None);
    assert_eq!(
        step_of("https://www.dropbox.com/s/abc123/report.pdf?raw=1"),
        None
    );
}

/// What a Drive share address of `id` asks for, `more` after its query.
fn download(id: &str, more: &str) -> Option<Step> {
    Some(Step::DriveDownload(format!(
        "https://drive.usercontent.google.com/download?id={id}&export=download{more}"
    )))
}

#[test]
fn a_drive_share_asks_for_its_download() {
    for url in [
        format!("https://drive.google.com/file/d/{DRIVE_ID}/view?usp=sharing"),
        format!("https://drive.google.com/file/d/{DRIVE_ID}/view"),
        format!("https://drive.google.com/file/d/{DRIVE_ID}"),
        format!("https://drive.google.com/open?id={DRIVE_ID}"),
        format!("https://drive.google.com/uc?id={DRIVE_ID}&export=download"),
        format!("https://drive.usercontent.google.com/download?id={DRIVE_ID}&export=download"),
    ] {
        assert_eq!(step_of(&url), download(DRIVE_ID, ""), "{url}");
    }
    // An older file's key goes with it.
    assert_eq!(
        step_of("https://drive.google.com/file/d/0Bxyz/view?usp=sharing&resourcekey=0-AbC"),
        download("0Bxyz", "&resourcekey=0-AbC")
    );
    // The question already answered: the file's address.
    assert_eq!(step_of(DRIVE_ANSWER), None);
    // No id, or one that is not an id.
    assert_eq!(step_of("https://drive.google.com/uc?export=download"), None);
    assert_eq!(step_of("https://drive.google.com/open?id=a%2Fb"), None);
    assert_eq!(step_of("https://drive.google.com/file/d//view"), None);
}

#[test]
fn a_mediafire_file_page_is_read_for_its_button() {
    for url in [
        "https://www.mediafire.com/file/0wpllm9h07b60b2/OWUL_Caster_Panel.zip/file",
        "http://www.mediafire.com/file/0wpllm9h07b60b2/OWUL_Caster_Panel.zip",
        "https://www.mediafire.com/download/0wpllm9h07b60b2",
        "https://www.mediafire.com/?0wpllm9h07b60b2",
    ] {
        assert_eq!(step_of(url), Some(Step::MediaFirePage(url.into())), "{url}");
    }
}

#[test]
fn drives_question_is_answered_by_its_form() {
    assert_eq!(drive_form(DRIVE_QUESTION).as_deref(), Some(DRIVE_ANSWER));
    // A page with no such form (a sign-in, a quota page): nothing.
    let sign_in = r#"<html><form action="https://accounts.google.com/x">
<input name="id" value="1"></form></html>"#;
    assert_eq!(drive_form(sign_in), None);
    // A form sending elsewhere, or in the clear, or by POST, is not followed.
    let action = "https://drive.usercontent.google.com/download";
    let elsewhere = DRIVE_QUESTION.replace(action, "https://example.com/download");
    assert_eq!(drive_form(&elsewhere), None);
    let plain = DRIVE_QUESTION.replace(action, "http://drive.usercontent.google.com/download");
    assert_eq!(drive_form(&plain), None);
    let posted = DRIVE_QUESTION.replace("method=\"get\"", "method=\"post\"");
    assert_eq!(drive_form(&posted), None);
}

#[test]
fn mediafires_button_names_the_file() {
    assert_eq!(
        mediafire_button(MEDIAFIRE_PAGE).as_deref(),
        Some(MEDIAFIRE_FILE)
    );
    let elsewhere = MEDIAFIRE_PAGE.replace("download1507.mediafire.com", "mediafire.example.net");
    assert_eq!(mediafire_button(&elsewhere), None);
    let script = MEDIAFIRE_PAGE.replace(MEDIAFIRE_FILE, "javascript:void(0)");
    assert_eq!(mediafire_button(&script), None);
    // A link that is not the Download button.
    let other = r#"<html><a href="https://download1.mediafire.com/x">x</a></html>"#;
    assert_eq!(mediafire_button(other), None);
}

#[test]
fn attributes_are_read_whole_and_unescaped() {
    let tag = r#"<a data-id='no' id='downloadButton' href="https://x.mf.com/a?b=1&amp;c=2">"#;
    assert_eq!(attribute(tag, "id").as_deref(), Some("downloadButton"));
    let href = attribute(tag, "href");
    assert_eq!(href.as_deref(), Some("https://x.mf.com/a?b=1&c=2"));
    assert_eq!(
        attribute("<input name=id value=7>", "value").as_deref(),
        Some("7")
    );
    assert_eq!(attribute("<input hidden>", "value"), None);
    // `<abbr>` is not an `<a>`.
    assert_eq!(tags(r#"<abbr id="x"><a id="y">"#, "a").count(), 1);
}
