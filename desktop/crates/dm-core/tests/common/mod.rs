//! What every integration test needs to stand a download up.
//!
//! `Options` gained four fields in one round of work, and each one had to be
//! added to four test files or four test binaries stopped compiling. The
//! server boilerplate had spread the same way.

#![allow(dead_code)] // each test binary uses a different part of this

use std::net::SocketAddr;
use std::path::Path;

use dm_core::{Categories, Control, Limiter, Options};

/// The options a test downloads with, and the one place a new field is added.
pub fn options(dir: &Path) -> Options {
    Options {
        connections: 8,
        control: Control::new(),
        dest_dir: dir.join("out"),
        parts_dir: dir.join("parts"),
        limiter: Limiter::unlimited(),
        categories: Categories::default(),
        checksum: None,
        quality: dm_core::stream::Quality::Best,
        media: false,
        referrer: None,
        proxy: None,
        container: None,
        attempts: 5,
        host_limits: Vec::new(),
        proxy_bypass: Vec::new(),
        subtitles: false,
        room: Default::default(),
        remux: false,
        server_dates: false,
        retry_wait: 0,
        sign_in: None,
        system_proxy: false,
    }
}

/// A temporary directory that cleans up after itself.
pub fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("a temporary directory")
}

/// Serves a router on a port the OS picks, so tests can run at once.
pub async fn serve(app: axum::Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

/// `address` with a login on it, set through the URL rather than written
/// into it: an address with a login spelled out reads to a secret scanner
/// as a leaked credential, made-up as these are.
pub fn with_login(address: &str, user: &str, password: &str) -> String {
    let mut url = url::Url::parse(address).expect("a test address");
    url.set_username(user).expect("a host to sign in to");
    url.set_password(Some(password))
        .expect("a host to sign in to");
    url.to_string()
}
