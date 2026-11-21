// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! FTP, taken over from a link as HTTP is.
//!
//! A client of dm's own rather than a library: what a download needs of FTP
//! is small — sign in, ask the size and the date, open a passive data
//! connection, start it at an offset — and every reply it reads comes from
//! outside, so it is parsed here, where it is fuzzed with the rest.
//!
//! Each range is its own session: `REST` sets where `RETR` starts, and a
//! range that ends before the file does is cut off by closing both
//! connections, which is how FTP stops a transfer early. Each range has a
//! part file of its own, and the plan is written beside them as an HTTP
//! download's is, so a pause resumes and a resume keeps the plan it started
//! with.
//!
//! `ftps://` is FTP over TLS: on port 990, as such an address means unless
//! it names another, the connection is TLS from its first byte; on any
//! other port dm asks for it (`AUTH TLS`) before signing in. The data
//! connections are TLS too (`PROT P`), each begun once the server has
//! answered the command it carries, which is when a server starts its end.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::fs;
use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWrite, AsyncWriteExt, BufReader,
    ReadHalf, WriteHalf,
};
use tokio::net::TcpStream;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::ClientConfig;

use crate::download::{Finished, Options, Update};
use crate::plan::{self, Segment};
use crate::{category, retry, Error};

/// Whether this is an FTP address dm fetches itself: `ftp://`, or
/// `ftps://` for FTP over TLS.
pub fn is_ftp(url: &str) -> bool {
    url.get(..6)
        .is_some_and(|s| s.eq_ignore_ascii_case("ftp://"))
        || url
            .get(..7)
            .is_some_and(|s| s.eq_ignore_ascii_case("ftps://"))
}

mod listing;
mod reply;
mod session;
mod target;
mod transfer;
mod transport;

pub use listing::*;
use reply::*;
use session::*;
use target::*;
pub use transfer::*;
use transport::*;

#[cfg(test)]
mod tls_tests;

#[cfg(test)]
mod tests;
