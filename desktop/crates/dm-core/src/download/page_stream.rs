// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! One stream of a video page: how its server wants to be asked, and its download, where the
//! address is the file.

use super::*;

/// How the server of a stream wants to be asked, as the reading of the page
/// gave it. Data only: nothing here names a site.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct StreamHints {
    /// The rest of the headers its reading says the server is asked with.
    pub headers: Headers,
    /// The most bytes one request may ask for (`downloader_options`'
    /// `http_chunk_size`): a server that slows or cuts any one response
    /// after that is asked for the next slice on a fresh one.
    pub chunk: Option<u64>,
}

/// Headers a video site's reading gives for one stream (`http_headers`),
/// kept for that stream's requests alone.
///
/// Only what dm does not own: the agent goes by dm's table of agents, and
/// `Range`, `If-Range`, `Accept-Encoding` (the engine needs the file as it
/// is) and the connection's own are dm's. A `Cookie` can be a sign-in, so
/// the values are never printed, and it is sent only where dm has none of
/// its own for the site, as a `Referer` only where it has none.
#[derive(Clone, Default, PartialEq)]
pub(crate) struct Headers(Vec<(reqwest::header::HeaderName, reqwest::header::HeaderValue)>);

impl std::fmt::Debug for Headers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|(n, _)| n))
            .finish()
    }
}

impl Headers {
    /// The sendable ones of `(name, value)` pairs; the rest are dropped.
    pub fn of<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Headers {
        const DM_OWN: [&str; 14] = [
            "user-agent",
            "range",
            "if-range",
            "accept-encoding",
            "host",
            "content-length",
            "content-type",
            "transfer-encoding",
            "connection",
            "te",
            "upgrade",
            "expect",
            "proxy-authorization",
            "authorization",
        ];
        Headers(
            pairs
                .into_iter()
                .filter(|(name, _)| !DM_OWN.iter().any(|own| own.eq_ignore_ascii_case(name)))
                .filter_map(|(name, value)| {
                    Some((
                        reqwest::header::HeaderName::from_bytes(name.as_bytes()).ok()?,
                        reqwest::header::HeaderValue::from_str(value).ok()?,
                    ))
                })
                .collect(),
        )
    }

    /// `request` with these added, below what dm sets itself.
    pub fn onto(
        &self,
        mut request: reqwest::RequestBuilder,
        own_cookie: bool,
        own_referrer: bool,
    ) -> reqwest::RequestBuilder {
        for (name, value) in &self.0 {
            let held = (*name == reqwest::header::COOKIE && own_cookie)
                || (*name == reqwest::header::REFERER && own_referrer);
            if !held {
                request = request.header(name.clone(), value.clone());
            }
        }
        request
    }
}

/// The headers of the stream being fetched, if one is.
pub(crate) fn stream_headers() -> Headers {
    HINTS
        .try_with(|hints| hints.headers.clone())
        .unwrap_or_default()
}

/// [`download_reporting`] for one stream of a video page (`videopage::fetch`),
/// asked for as its reading says: the address is the file, so what the
/// server refuses, or answers with a page, is that stream's failure.
pub(crate) async fn download_video_stream_with(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
    hints: StreamHints,
) -> Result<Finished, Error> {
    // Boxed: a video page's streams are downloads that may themselves meet a
    // page (`videopage::download_page`), which an async fn cannot be recursive in.
    Box::pin(HINTS.scope(
        hints,
        STREAM.scope((), download_reporting(client, url, options, report)),
    ))
    .await
}

/// What [`download_numbered_stream_with`] fails with where the stream's first piece does not
/// say how many follow, or it failed before any piece after the first was whole: the video is
/// then fetched from its other formats (`videopage::download_page`), as before dm read
/// numbered pieces. Never shown: a fallback is always taken on it.
pub(crate) const NUMBERED_UNREAD: &str = "the stream's numbered pieces could not be read";

/// [`download_video_stream_with`] for a stream served only as numbered pieces (`&sq=N`, gaps
/// row Y14): fetched piece by piece by the DASH piece fetcher, and gathered into one file.
pub(crate) async fn download_numbered_stream_with(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
    hints: StreamHints,
) -> Result<Finished, Error> {
    Box::pin(HINTS.scope(
        hints,
        STREAM.scope((), dash::download_numbered(client, url, options, report)),
    ))
    .await
}

/// Whether the download in hand is one stream of a video page.
pub(super) fn is_stream() -> bool {
    STREAM.try_with(|()| ()).is_ok()
}
