// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A reply's body read whole, but never past a limit (gaps row H14): a server that states no
//! length, or states one and sends more, was read for as long as a timer allowed, and a fast
//! one filled memory in that time.

use std::fmt;

/// Why a body was not read whole.
#[derive(Debug)]
pub enum BodyError {
    /// It went on past the most it may hold.
    TooLong {
        /// The most it may hold, in bytes.
        most: usize,
    },
    /// The connection failed while it was read.
    Read(reqwest::Error),
}

impl fmt::Display for BodyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BodyError::TooLong { most } => {
                write!(f, "the server sent more than the {most} bytes expected")
            }
            BodyError::Read(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BodyError {}

/// `res`'s body, read to its end, or `TooLong` as soon as it passes `most` bytes. A stated
/// length over `most` is refused before anything is read.
pub async fn at_most(mut res: reqwest::Response, most: usize) -> Result<Vec<u8>, BodyError> {
    if res.content_length().is_some_and(|n| n > most as u64) {
        return Err(BodyError::TooLong { most });
    }
    let mut body = Vec::new();
    while let Some(chunk) = res.chunk().await.map_err(BodyError::Read)? {
        if body.len() + chunk.len() > most {
            return Err(BodyError::TooLong { most });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// A server that answers once with `head` and then `body`, its connection closed after.
    fn serve(head: &'static str, body: Vec<u8>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let at = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut asked = [0u8; 1024];
            let _ = s.read(&mut asked);
            let _ = s.write_all(head.as_bytes());
            let _ = s.write_all(&body);
        });
        format!("http://{at}/")
    }

    async fn get(url: &str) -> reqwest::Response {
        crate::http_client::client().get(url).send().await.unwrap()
    }

    #[tokio::test]
    async fn a_body_with_no_length_is_cut_off_past_the_most() {
        let url = serve(
            "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n",
            vec![b'x'; 70_000],
        );
        let res = get(&url).await;
        assert!(res.content_length().is_none());
        assert!(matches!(
            at_most(res, 65_536).await,
            Err(BodyError::TooLong { most: 65_536 })
        ));
    }

    #[tokio::test]
    async fn a_stated_length_over_the_most_is_refused_unread() {
        let url = serve(
            "HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\nConnection: close\r\n\r\n",
            vec![b'x'; 16],
        );
        assert!(matches!(
            at_most(get(&url).await, 1024).await,
            Err(BodyError::TooLong { most: 1024 })
        ));
    }

    #[tokio::test]
    async fn a_body_within_the_most_is_read_whole() {
        let url = serve(
            "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n",
            b"the whole of it".to_vec(),
        );
        assert_eq!(
            at_most(get(&url).await, 1024).await.unwrap(),
            b"the whole of it"
        );
    }
}
