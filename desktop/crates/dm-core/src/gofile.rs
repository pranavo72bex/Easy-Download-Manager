//! Sharing a finished file through Gofile: the file goes up as a guest
//! upload and its page, the link to give someone, comes back.
//!
//! Nothing here runs unless a person asks for it on a file; no account or
//! key is kept, so what is sent is the file and nothing about the person.

use futures_util::StreamExt;
use std::path::Path;
use tokio::io::AsyncReadExt;

const SERVERS: &str = "https://api.gofile.io/servers";

/// What Gofile gave back for an upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uploaded {
    /// The page to share: where someone downloads the file.
    pub page: String,
    /// The file's id on Gofile.
    pub id: String,
}

/// Uploads `path` to one of Gofile's servers and returns its page.
pub async fn upload(client: &reqwest::Client, path: &Path) -> Result<Uploaded, String> {
    upload_to(
        client,
        SERVERS,
        |name| format!("https://{name}.gofile.io/contents/uploadfile"),
        path,
    )
    .await
}

/// [`upload`] against other addresses: the list of servers, and where a
/// named server takes a file.
pub async fn upload_to(
    client: &reqwest::Client,
    servers: &str,
    upload_url: impl Fn(&str) -> String,
    path: &Path,
) -> Result<Uploaded, String> {
    let meta = tokio::fs::metadata(path)
        .await
        .map_err(|e| format!("the file cannot be read: {e}"))?;
    if !meta.is_file() {
        return Err("only a file can be shared".into());
    }
    let name = server_name(&get_json(client, servers).await?)?;
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|e| format!("the file cannot be read: {e}"))?;

    let shown = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let boundary = format!("dm-{}", hex::encode(random_bytes()));
    let head = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\nContent-Type: application/octet-stream\r\n\r\n",
        header_safe(&shown)
    );
    let tail = format!("\r\n--{boundary}--\r\n");
    let length = head.len() as u64 + meta.len() + tail.len() as u64;

    // The file is read as it is sent, not held: it may be many gigabytes.
    let body =
        futures_util::stream::once(
            async move { Ok::<_, std::io::Error>(bytes::Bytes::from(head)) },
        )
        .chain(futures_util::stream::unfold(file, |mut file| async move {
            let mut buf = vec![0u8; 256 * 1024];
            match file.read(&mut buf).await {
                Ok(0) => None,
                Ok(n) => {
                    buf.truncate(n);
                    Some((Ok(bytes::Bytes::from(buf)), file))
                }
                Err(e) => Some((Err(e), file)),
            }
        }))
        .chain(futures_util::stream::once(async move {
            Ok(bytes::Bytes::from(tail))
        }));

    let response = client
        .post(upload_url(&name))
        .header(
            reqwest::header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header(reqwest::header::CONTENT_LENGTH, length)
        .body(reqwest::Body::wrap_stream(body))
        .send()
        .await
        .map_err(|e| format!("the upload failed: {e}"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| format!("the upload failed: {e}"))?;
    if !status.is_success() && serde_json::from_str::<serde_json::Value>(&text).is_err() {
        return Err(format!("Gofile answered {status}"));
    }
    parse_upload(&text)
}

async fn get_json(client: &reqwest::Client, url: &str) -> Result<String, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Gofile cannot be reached: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("Gofile answered {}", response.status()));
    }
    response
        .text()
        .await
        .map_err(|e| format!("Gofile cannot be reached: {e}"))
}

/// The first server of the list Gofile gave.
fn server_name(json: &str) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|_| "Gofile's answer was not understood")?;
    if value["status"] != "ok" {
        return Err(format!("Gofile refused: {}", value["status"]));
    }
    let name = value["data"]["servers"]
        .as_array()
        .and_then(|s| s.iter().find_map(|s| s["name"].as_str()))
        .ok_or("Gofile named no server to upload to")?;
    // Becomes part of a host name, so nothing else may get through.
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err("Gofile named a server dm will not use".into());
    }
    Ok(name.to_string())
}

fn parse_upload(json: &str) -> Result<Uploaded, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|_| "Gofile's answer was not understood")?;
    if value["status"] != "ok" {
        return Err(format!("Gofile refused: {}", value["status"]));
    }
    let data = &value["data"];
    let page = data["downloadPage"]
        .as_str()
        .filter(|p| p.starts_with("https://"))
        .ok_or("Gofile gave no link")?;
    Ok(Uploaded {
        page: page.to_string(),
        id: data["id"]
            .as_str()
            .or_else(|| data["fileId"].as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

/// A file name that cannot end the header line or its quotes.
fn header_safe(name: &str) -> String {
    name.chars()
        .map(|c| {
            if matches!(c, '"' | '\r' | '\n' | '\\') {
                '_'
            } else {
                c
            }
        })
        .collect()
}

fn random_bytes() -> [u8; 8] {
    let mut b = [0u8; 8];
    // A boundary only has to differ from the file's own bytes.
    if getrandom::fill(&mut b).is_err() {
        b = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1)
            .to_le_bytes();
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn a_server_name_that_could_change_the_host_is_refused() {
        let ok = r#"{"status":"ok","data":{"servers":[{"name":"store3","zone":"eu"}]}}"#;
        assert_eq!(server_name(ok).unwrap(), "store3");
        for bad in ["evil.com/", "a@b", "A", ""] {
            let json = format!(r#"{{"status":"ok","data":{{"servers":[{{"name":"{bad}"}}]}}}}"#);
            assert!(server_name(&json).is_err(), "{bad}");
        }
        assert!(server_name(r#"{"status":"error-rateLimit"}"#).is_err());
    }

    #[test]
    fn the_page_comes_from_the_answer_and_only_if_it_is_https() {
        let ok = r#"{"status":"ok","data":{"downloadPage":"https://gofile.io/d/abc","id":"f1"}}"#;
        let up = parse_upload(ok).unwrap();
        assert_eq!(
            (up.page.as_str(), up.id.as_str()),
            ("https://gofile.io/d/abc", "f1")
        );
        assert!(parse_upload(r#"{"status":"ok","data":{"downloadPage":"http://x/d"}}"#).is_err());
        assert!(parse_upload(r#"{"status":"error-notFound"}"#).is_err());
        assert!(parse_upload("<html>").is_err());
    }

    #[test]
    fn a_name_cannot_break_out_of_its_header() {
        assert_eq!(header_safe("a\"b\r\nc\\d.zip"), "a_b__c_d.zip");
    }

    /// A one-request server: reads a whole request, answers with `reply`,
    /// and hands the request back.
    async fn serve(reply: String) -> (String, tokio::task::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut got = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                let n = sock.read(&mut buf).await.unwrap();
                got.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&got).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    let len = text[..end]
                        .to_ascii_lowercase()
                        .split("content-length: ")
                        .nth(1)
                        .and_then(|r| r.split("\r\n").next())
                        .and_then(|n| n.parse::<usize>().ok())
                        .unwrap_or(0);
                    if got.len() >= end + 4 + len {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let out = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
            sock.write_all(out.as_bytes()).await.unwrap();
            got
        });
        (format!("http://{addr}"), task)
    }

    #[tokio::test]
    async fn a_file_goes_up_whole_and_its_page_comes_back() {
        let dir = std::env::temp_dir().join(format!("dm-gofile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("big \"one\".bin");
        let content: Vec<u8> = (0..700_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&file, &content).unwrap();

        let (servers_base, servers) =
            serve(r#"{"status":"ok","data":{"servers":[{"name":"store1"}]}}"#.into()).await;
        let (upload_base, upload) = serve(
            r#"{"status":"ok","data":{"downloadPage":"https://gofile.io/d/xyz","id":"i1"}}"#.into(),
        )
        .await;

        let client = reqwest::Client::new();
        let up = upload_to(
            &client,
            &servers_base,
            |name| {
                assert_eq!(name, "store1");
                format!("{upload_base}/contents/uploadfile")
            },
            &file,
        )
        .await
        .unwrap();
        assert_eq!(up.page, "https://gofile.io/d/xyz");
        servers.await.unwrap();

        let request = upload.await.unwrap();
        let text = String::from_utf8_lossy(&request);
        assert!(text.starts_with("POST /contents/uploadfile"));
        assert!(text.contains("name=\"file\"; filename=\"big _one_.bin\""));
        // The file's bytes are in the body, unchanged.
        assert!(request
            .windows(content.len())
            .any(|w| w == content.as_slice()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_refusal_is_reported() {
        let (base, _task) = serve(r#"{"status":"error-rateLimit"}"#.into()).await;
        let file = std::env::temp_dir().join(format!("dm-gofile-refused-{}", std::process::id()));
        std::fs::write(&file, b"x").unwrap();
        let err = upload_to(&reqwest::Client::new(), &base, |_| String::new(), &file)
            .await
            .unwrap_err();
        assert!(err.contains("refused"), "{err}");
        std::fs::remove_file(&file).ok();
    }

    /// Against the real service; run by hand with `--ignored`.
    #[tokio::test]
    #[ignore]
    async fn live_upload() {
        let file = std::env::temp_dir().join("dm-gofile-live.txt");
        std::fs::write(&file, b"dm upload test, safe to delete\n").unwrap();
        let up = upload(&crate::client(), &file).await.unwrap();
        println!("LINK {} {}", up.page, up.id);
        assert!(up.page.starts_with("https://gofile.io/d/"));
        std::fs::remove_file(&file).ok();
    }
}
