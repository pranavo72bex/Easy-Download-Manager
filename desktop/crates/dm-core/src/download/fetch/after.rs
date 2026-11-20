// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What `fetch_found` does once a file's bytes are in: the digest it is checked against where
//! none was typed, and the server's date on the finished file. Out of `fetch.rs` so that file
//! stays under the size gate's 800 lines (gaps row A12).

use super::*;

/// The digest to check where none was typed and no Metalink gave one: the server's own word
/// for it, or a checksum file it publishes beside the file. Whether it came from beside.
pub(super) async fn checksum_beside(
    client: &reqwest::Client,
    url: &str,
    info: &Probe,
    options: &Options,
    mut checksum: Option<Checksum>,
    listed_mirrors: bool,
) -> (Option<Checksum>, bool) {
    /* Nothing typed and no Metalink's: a checksum file the server publishes beside the file
     * (`dm.zip.sha256`, `SHA256SUMS`) is the digest, checked as a typed one is (gaps row F5).
     * Not for a video caught from a page or a web page saved as one, which no release
     * vouches for. */
    let beside = checksum.is_none()
        && !listed_mirrors
        && !is_stream()
        && !options.media
        && !options.accept_page;
    /* The server's own word for the file's digest, where its answer gave one (`Repr-Digest`,
     * `Digest`, `Content-MD5`), is taken first under the same rules: it costs no request, and no
     * checksum file is then asked for (gaps row W19). A mismatch fails the file as a checksum
     * file's does; the row says nothing more of a match than a typed digest's. */
    let from_header = beside && info.digest.is_some();
    if from_header {
        checksum = info.digest.clone();
    } else if beside {
        // A pause or a cancel meanwhile ends the look with nothing found; the assembly below
        // honours it as it does any other.
        checksum =
            sidecar::published_beside(client, url, &info.final_url, &info.filename, options).await;
    }
    let from_beside = beside && !from_header && checksum.is_some();
    (checksum, from_beside)
}

/// The server's date on the finished file, where asked.
pub(super) fn set_server_date(options: &Options, info: &Probe, path: &Path) {
    // The server's date on the file, where asked: a file a person sorts by
    // date then sorts by when it was made, not when it was fetched.
    if options.server_dates {
        if let Some(when) = info.modified.as_deref().and_then(http_date) {
            // A file system that keeps no such date leaves the download's
            // own, which is no reason to fail a whole file.
            let _ = std::fs::File::options()
                .write(true)
                .open(path)
                .and_then(|file| file.set_modified(when));
        }
    }
}
