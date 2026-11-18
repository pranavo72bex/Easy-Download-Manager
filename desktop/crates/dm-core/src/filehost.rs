// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The public share address of a file-storage service, turned into the file's own address
//! (gaps row W7).
//!
//! A share link ("anyone with the link") is a page about the file: given to Add URL as it is,
//! its HTML was what came. Each service here was looked at signed out (2026-11-18) and answers
//! as follows; what is done is what its own page's Download button does:
//!
//! * **Dropbox** (`www.dropbox.com/s/…`, `/scl/fi/…`, `/sh/…`, `/scl/fo/…`): the same
//!   address with `dl=1` in place of `dl=0` is the file (a folder's is a ZIP of it), sent on to
//!   `dl.dropboxusercontent.com` under its own name. No request is needed to know it.
//! * **Google Drive** (`drive.google.com/file/d/<id>/view`, `/open?id=`, `/uc?id=`): the file
//!   is `drive.usercontent.google.com/download?id=<id>&export=download`. A file too large for
//!   Drive to scan for viruses answers that with a page, "Google Drive can't scan this file
//!   for viruses", whose form (`id="download-form"`, its hidden `id`, `export`, `confirm`
//!   and `uuid`) is the "Download anyway" button: the form's address is the file.
//! * **MediaFire** (`www.mediafire.com/file/<key>/…`, `/download/<key>`, `/?<key>`): the file
//!   page's Download button (`id="downloadButton"`) links to the file on a
//!   `downloadNNNN.mediafire.com` server.
//!
//! Every other address is left as it is, with no request made for it. Where a service does
//! not answer as above (a private file's sign-in page, a removed file, a page that changed),
//! the address is also left as it is, so what happened before happens: the page is asked
//! about (contract row B40). Nothing is signed in: the requests here carry no cookie.
//!
//! Not done, and why: OneDrive (its share links now open a page that fetches the file through
//! a token of the page's own, not a documented address; its documented `shares` address
//! answered 404 for a public link), MEGA (the file is encrypted and decrypted in the browser by
//! MEGA's page), iCloud (no public address documented), and the hosts that make a visitor wait,
//! solve a check or sign in (`docs/site-list.md`), which the browser hands over instead.
//!
//! Test knob: `DM_FILE_HOST_STAND_IN` (`http://127.0.0.1:<port>`) moves every address this
//! asks or gives onto a test's server, as `DM_YOUTUBE_HOST` does for YouTube.

// Reads a service's page: an index out of range here is a crash a page could cause, so every
// access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

use url::Url;

use crate::Error;

/// The most of a service's page read to find the file's address; MediaFire's file page was
/// 330 KB on 2026-11-18, Drive's question 2.4 KB.
const PAGE_MOST: usize = 2 * 1024 * 1024;

/// A service whose share addresses this knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Service {
    Dropbox,
    Drive,
    MediaFire,
}

/// What a share address asks for: an address known without asking, or one to be read from a
/// page the service answers.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    /// The file's address.
    File(String),
    /// Drive's download address, which may answer with its "can't scan" question.
    DriveDownload(String),
    /// MediaFire's file page, whose Download button names the file.
    MediaFirePage(String),
}

fn service_of(url: &Url) -> Option<Service> {
    let host = url.host_str()?.to_ascii_lowercase();
    match host.as_str() {
        "www.dropbox.com" | "dropbox.com" => Some(Service::Dropbox),
        "drive.google.com" | "drive.usercontent.google.com" => Some(Service::Drive),
        "www.mediafire.com" | "mediafire.com" => Some(Service::MediaFire),
        _ => None,
    }
}

fn query(url: &Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
        .filter(|v| !v.is_empty())
}

/// What `url` asks for, if it is a share address of a service this knows; `None` for every
/// other address, and for one that is already the file's.
fn step_of(url: &str) -> Option<Step> {
    let parsed = Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let path = parsed.path().to_string();
    match service_of(&parsed)? {
        Service::Dropbox => {
            let shared = ["/s/", "/scl/fi/", "/sh/", "/scl/fo/"]
                .iter()
                .any(|p| path.starts_with(p));
            let file_already = query(&parsed, "dl").as_deref() == Some("1")
                || query(&parsed, "raw").as_deref() == Some("1");
            if !shared || file_already {
                return None;
            }
            let mut out = parsed.clone();
            let kept: Vec<(String, String)> = parsed
                .query_pairs()
                .filter(|(k, _)| k != "dl")
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            out.query_pairs_mut()
                .clear()
                .extend_pairs(kept)
                .append_pair("dl", "1");
            Some(Step::File(out.into()))
        }
        Service::Drive => {
            let host = parsed.host_str()?.to_ascii_lowercase();
            let id = if host == "drive.usercontent.google.com" {
                // Already the download address: asked only where its question has not been
                // answered.
                if path != "/download" || query(&parsed, "confirm").is_some() {
                    return None;
                }
                query(&parsed, "id")?
            } else if let Some(rest) = path.strip_prefix("/file/d/") {
                rest.split('/').next()?.to_string()
            } else if path == "/open" || path == "/uc" {
                query(&parsed, "id")?
            } else {
                return None;
            };
            if !is_id(&id) {
                return None;
            }
            let mut out = Url::parse("https://drive.usercontent.google.com/download").ok()?;
            out.query_pairs_mut()
                .append_pair("id", &id)
                .append_pair("export", "download");
            // An older file shared by link names its key too, and is not served without it.
            if let Some(key) = query(&parsed, "resourcekey") {
                out.query_pairs_mut().append_pair("resourcekey", &key);
            }
            Some(Step::DriveDownload(out.into()))
        }
        Service::MediaFire => {
            let page = path.starts_with("/file/")
                || path.starts_with("/file_premium/")
                || path.starts_with("/download/")
                || (path == "/" && parsed.query().is_some_and(|q| !q.is_empty()));
            page.then(|| Step::MediaFirePage(url.to_string()))
        }
    }
}

/// Whether `id` is a Drive file's id: letters, digits, `-` and `_`.
fn is_id(id: &str) -> bool {
    let allowed = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    !id.is_empty() && id.chars().all(allowed)
}

/// The test's stand-in for every service, if one is named.
fn stand_in() -> Option<Url> {
    std::env::var("DM_FILE_HOST_STAND_IN")
        .ok()
        .and_then(|v| Url::parse(&v).ok())
}

/// `url` as it is asked: on the stand-in where there is one.
fn asked(url: &str) -> String {
    let (Some(stand_in), Ok(mut moved)) = (stand_in(), Url::parse(url)) else {
        return url.to_string();
    };
    if moved.set_scheme(stand_in.scheme()).is_err()
        || moved.set_host(stand_in.host_str()).is_err()
        || moved.set_port(stand_in.port()).is_err()
    {
        return url.to_string();
    }
    moved.into()
}

/// Whether `found` (an address a service's page named) is one of that service's own, or the
/// stand-in's: a page names nothing else that is fetched in its place.
fn own_address(found: &Url, domain: &str) -> bool {
    if !matches!(found.scheme(), "http" | "https") {
        return false;
    }
    let host = found.host_str().unwrap_or_default().to_ascii_lowercase();
    if let Some(stand_in) = stand_in() {
        if stand_in.host_str() == found.host_str() && stand_in.port() == found.port() {
            return true;
        }
    }
    found.scheme() == "https" && (host == domain || host.ends_with(&format!(".{domain}")))
}

/// The file's own address for `url`, where `url` is a share address of a file-storage
/// service this knows (the module's notes); `Ok(None)` for any other address, and where the
/// service's answer is not the one expected, so the address is fetched as it always was.
///
/// # Errors
///
/// Only where the service could not be reached (a network error), as the download itself
/// could not have been.
pub async fn file_address(client: &reqwest::Client, url: &str) -> Result<Option<String>, Error> {
    let Some(step) = step_of(url) else {
        return Ok(None);
    };
    match step {
        Step::File(file) => Ok(Some(asked(&file))),
        Step::DriveDownload(download) => {
            let download = asked(&download);
            let Some(page) = page_at(client, &download).await? else {
                // The file itself: a file small enough to be scanned has no question.
                return Ok(Some(download));
            };
            Ok(drive_form(&page))
        }
        Step::MediaFirePage(page) => {
            let page = asked(&page);
            match page_at(client, &page).await? {
                Some(html) => Ok(mediafire_button(&html)),
                None => Ok(None),
            }
        }
    }
}

/// The page `url` answers with, or `None` where it answers with a file (whose body is let go
/// unread) or a refusal (which the download meets again and says).
async fn page_at(client: &reqwest::Client, url: &str) -> Result<Option<String>, Error> {
    let mut answer = client
        .get(url)
        .header(reqwest::header::USER_AGENT, crate::agent_for(url))
        .header(reqwest::header::ACCEPT_ENCODING, crate::download::IDENTITY)
        .send()
        .await?;
    if !answer.status().is_success() {
        return Ok(None);
    }
    let kind = answer
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    if !crate::videopage::is_page(kind.as_deref()) {
        return Ok(None);
    }
    let mut body = Vec::new();
    while let Some(chunk) = answer.chunk().await? {
        body.extend_from_slice(&chunk);
        if body.len() >= PAGE_MOST {
            break;
        }
    }
    Ok(Some(String::from_utf8_lossy(&body).into_owned()))
}

/// The value of attribute `name` in the tag `tag` (its text from `<` to `>`), its character
/// references for `&`, quotes and `<`/`>` undone.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower.get(from..)?.find(name) {
        let start = from + at;
        from = start + name.len();
        // A whole attribute name: after a space, followed by `=`.
        let before = lower.get(..start)?.chars().next_back();
        if !before.is_some_and(char::is_whitespace) {
            continue;
        }
        let rest = tag.get(from..)?.trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim_start();
        let quote = rest.chars().next()?;
        let value = if quote == '"' || quote == '\'' {
            let inner = rest.get(1..)?;
            inner.get(..inner.find(quote)?)?
        } else {
            rest.get(..rest.find(|c: char| c.is_whitespace() || c == '>')?)?
        };
        return Some(
            value
                .replace("&quot;", "\"")
                .replace("&#39;", "'")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&"),
        );
    }
    None
}

/// Each tag named `name` in `html`, its text from `<` to `>`.
fn tags<'a>(html: &'a str, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    let open = format!("<{name}");
    let mut rest = html;
    std::iter::from_fn(move || loop {
        let at = rest.find(&open)?;
        let tag = rest.get(at..)?;
        let end = tag.find('>')?;
        rest = tag.get(end..)?;
        let whole = tag.get(..=end)?;
        // `<a` is not `<abbr`.
        if whole
            .get(open.len()..)
            .and_then(|r| r.chars().next())
            .is_some_and(|c| c.is_whitespace() || c == '>' || c == '/')
        {
            return Some(whole);
        }
    })
}

/// Drive's "Download anyway" form as the address it submits: its action, with each of its
/// inputs that has a name. `None` where the page holds no such form.
fn drive_form(html: &str) -> Option<String> {
    let start = html.find("id=\"download-form\"")?;
    let form_open = html.get(..start)?.rfind("<form")?;
    let form = html.get(form_open..)?;
    let form = form.get(..form.find("</form>").unwrap_or(form.len()))?;
    let open = tags(form, "form").next()?;
    let method = attribute(open, "method").unwrap_or_else(|| "get".into());
    if !method.eq_ignore_ascii_case("get") {
        return None;
    }
    let mut action = Url::parse(&asked(&attribute(open, "action")?)).ok()?;
    if !own_address(&action, "google.com") {
        return None;
    }
    {
        let mut pairs = action.query_pairs_mut();
        pairs.clear();
        for input in tags(form, "input") {
            let name = attribute(input, "name");
            if let (Some(name), Some(value)) = (name, attribute(input, "value")) {
                pairs.append_pair(&name, &value);
            }
        }
    }
    // Asked for nothing: not the form that was looked for.
    query(&action, "id")?;
    Some(action.into())
}

/// MediaFire's Download button's address. `None` where the page holds none.
fn mediafire_button(html: &str) -> Option<String> {
    let button =
        tags(html, "a").find(|tag| attribute(tag, "id").as_deref() == Some("downloadButton"))?;
    let href = Url::parse(&attribute(button, "href")?).ok()?;
    own_address(&href, "mediafire.com").then(|| href.into())
}

#[cfg(test)]
mod tests;
