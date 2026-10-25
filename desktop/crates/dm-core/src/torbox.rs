//! Fetching a link through TorBox: the account's own API key asks TorBox to
//! download the link on its servers, and the address it gives back for the
//! finished file is handed to the ordinary engine.
//!
//! Nothing here runs unless a person asks for a link to go through TorBox;
//! only that link and the person's key are sent, and only to TorBox.

use std::time::Duration;

const API: &str = "https://api.torbox.app/v1/api";

/// A file TorBox has ready, with the address to fetch it from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ready {
    pub url: String,
    pub name: String,
    pub size: u64,
}

/// How long TorBox is asked about a link it is still fetching.
#[derive(Debug, Clone, Copy)]
pub struct Patience {
    pub every: Duration,
    pub asks: u32,
}

impl Default for Patience {
    /// Ten minutes, asking every three seconds.
    fn default() -> Self {
        Patience {
            every: Duration::from_secs(3),
            asks: 200,
        }
    }
}

/// Has TorBox fetch `link` and returns the files it made ready.
pub async fn fetch(
    client: &reqwest::Client,
    key: &str,
    link: &str,
    patience: Patience,
) -> Result<Vec<Ready>, String> {
    fetch_from(client, API, key, link, patience).await
}

/// [`fetch`] against another address for the API.
pub async fn fetch_from(
    client: &reqwest::Client,
    base: &str,
    key: &str,
    link: &str,
    patience: Patience,
) -> Result<Vec<Ready>, String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("no TorBox API key is set (Options › Sign-ins)".into());
    }
    if !link.starts_with("http://") && !link.starts_with("https://") {
        return Err("TorBox takes web links only".into());
    }
    let form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("link", link)
        .finish();
    let created = call(
        client
            .post(format!("{base}/webdl/createwebdownload"))
            .bearer_auth(key)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(form),
    )
    .await?;
    let id = created["webdownload_id"]
        .as_u64()
        .or_else(|| {
            created["webdownload_id"]
                .as_str()
                .and_then(|s| s.parse().ok())
        })
        .ok_or("TorBox gave no id for the download")?;

    for ask in 0..patience.asks.max(1) {
        if ask > 0 {
            tokio::time::sleep(patience.every).await;
        }
        let item = call(
            client
                .get(format!("{base}/webdl/mylist"))
                .query(&[("id", id.to_string()), ("bypass_cache", "true".into())])
                .bearer_auth(key),
        )
        .await?;
        let state = item["download_state"].as_str().unwrap_or_default();
        if state.starts_with("error") || state.starts_with("failed") {
            return Err(format!("TorBox could not fetch the link ({state})"));
        }
        if item["download_finished"].as_bool() == Some(true)
            || item["download_present"].as_bool() == Some(true)
        {
            return links(client, base, key, id, &item).await;
        }
    }
    Err("TorBox is still fetching the link; try again in a while".into())
}

/// The address of each file of a finished item.
async fn links(
    client: &reqwest::Client,
    base: &str,
    key: &str,
    id: u64,
    item: &serde_json::Value,
) -> Result<Vec<Ready>, String> {
    let files = item["files"].as_array().cloned().unwrap_or_default();
    if files.is_empty() {
        return Err("TorBox fetched the link but listed no file".into());
    }
    let mut out = Vec::new();
    for file in &files {
        let Some(file_id) = file["id"].as_u64() else {
            continue;
        };
        // TorBox takes the key as a query value here; the address it gives
        // back is for this file alone.
        let answer = call(client.get(format!("{base}/webdl/requestdl")).query(&[
            ("token", key.to_string()),
            ("web_id", id.to_string()),
            ("file_id", file_id.to_string()),
        ]))
        .await?;
        let url = answer
            .as_str()
            .filter(|u| u.starts_with("https://"))
            .ok_or("TorBox gave no secure address for the file")?;
        let name = file["short_name"]
            .as_str()
            .or_else(|| file["name"].as_str().and_then(|n| n.rsplit('/').next()))
            .unwrap_or_default();
        out.push(Ready {
            url: url.to_string(),
            name: name.to_string(),
            size: file["size"].as_u64().unwrap_or(0),
        });
    }
    if out.is_empty() {
        return Err("TorBox listed no file it could give".into());
    }
    Ok(out)
}

/// One request, and the `data` of an answer that says it worked.
async fn call(request: reqwest::RequestBuilder) -> Result<serde_json::Value, String> {
    // Never the address in an error: one of them carries the key.
    let response = request
        .send()
        .await
        .map_err(|e| format!("TorBox cannot be reached: {}", e.without_url()))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| format!("TorBox cannot be reached: {}", e.without_url()))?;
    parse(status.as_u16(), &text)
}

fn parse(status: u16, text: &str) -> Result<serde_json::Value, String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Err(format!("TorBox answered {status}"));
    };
    if value["success"].as_bool() == Some(true) {
        return Ok(value["data"].clone());
    }
    let why = value["detail"]
        .as_str()
        .or_else(|| value["error"].as_str())
        .unwrap_or("no reason given");
    Err(format!("TorBox refused: {why}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// A server that answers each request by its path from `routes` (first
    /// match wins) and records every request line plus body.
    async fn serve(routes: Vec<(&'static str, Vec<String>)>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        let routes = Arc::new(Mutex::new(routes));
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let (log, routes) = (Arc::clone(&log), Arc::clone(&routes));
                tokio::spawn(async move {
                    let mut got = Vec::new();
                    let mut buf = [0u8; 8192];
                    loop {
                        let n = sock.read(&mut buf).await.unwrap_or(0);
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
                    let text = String::from_utf8_lossy(&got).to_string();
                    let line = text.lines().next().unwrap_or("").to_string();
                    log.lock().unwrap().push(text.clone());
                    let reply = {
                        let mut routes = routes.lock().unwrap();
                        let hit = routes.iter_mut().find(|(p, _)| line.contains(p));
                        match hit {
                            Some((_, replies)) if replies.len() > 1 => replies.remove(0),
                            Some((_, replies)) => replies[0].clone(),
                            None => r#"{"success":false,"detail":"no route"}"#.into(),
                        }
                    };
                    let out = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                        reply.len()
                    );
                    let _ = sock.write_all(out.as_bytes()).await;
                });
            }
        });
        (format!("http://{addr}"), seen)
    }

    fn quick() -> Patience {
        Patience {
            every: Duration::from_millis(5),
            asks: 5,
        }
    }

    #[tokio::test]
    async fn a_link_is_created_waited_for_and_given_an_address() {
        let (base, seen) = serve(vec![
            (
                "createwebdownload",
                vec![r#"{"success":true,"data":{"webdownload_id":42}}"#.into()],
            ),
            (
                "mylist",
                vec![
                    r#"{"success":true,"data":{"download_state":"downloading","download_finished":false}}"#.into(),
                    r#"{"success":true,"data":{"download_state":"completed","download_finished":true,"files":[{"id":0,"name":"dir/a b.zip","short_name":"a b.zip","size":1234}]}}"#.into(),
                ],
            ),
            (
                "requestdl",
                vec![r#"{"success":true,"data":"https://cdn.example/f?x=1"}"#.into()],
            ),
        ])
        .await;
        let got = fetch_from(
            &reqwest::Client::new(),
            &base,
            " KEY123 ",
            "https://host.example/f?a=b&c=d",
            quick(),
        )
        .await
        .unwrap();
        assert_eq!(
            got,
            [Ready {
                url: "https://cdn.example/f?x=1".into(),
                name: "a b.zip".into(),
                size: 1234
            }]
        );
        let seen = seen.lock().unwrap();
        let create = &seen[0];
        assert!(create.starts_with("POST /webdl/createwebdownload"));
        assert!(create.contains("authorization: Bearer KEY123"), "{create}");
        // The link is form-encoded, so its own & and = stay inside it.
        assert!(create.ends_with("link=https%3A%2F%2Fhost.example%2Ff%3Fa%3Db%26c%3Dd"));
        assert!(seen
            .last()
            .unwrap()
            .contains("token=KEY123&web_id=42&file_id=0"));
    }

    #[tokio::test]
    async fn a_failed_fetch_and_a_refusal_are_reported() {
        let (base, _) = serve(vec![
            (
                "createwebdownload",
                vec![r#"{"success":true,"data":{"webdownload_id":"7"}}"#.into()],
            ),
            (
                "mylist",
                vec![r#"{"success":true,"data":{"download_state":"error"}}"#.into()],
            ),
        ])
        .await;
        let client = reqwest::Client::new();
        let err = fetch_from(&client, &base, "k", "https://h.example/f", quick())
            .await
            .unwrap_err();
        assert!(err.contains("could not fetch"), "{err}");

        let (base, _) = serve(vec![(
            "createwebdownload",
            vec![
                r#"{"success":false,"error":"BAD_TOKEN","detail":"Your token is invalid"}"#.into(),
            ],
        )])
        .await;
        let err = fetch_from(&client, &base, "k", "https://h.example/f", quick())
            .await
            .unwrap_err();
        assert_eq!(err, "TorBox refused: Your token is invalid");
    }

    #[tokio::test]
    async fn waiting_ends_and_bad_input_is_refused_before_anything_is_sent() {
        let (base, seen) = serve(vec![
            (
                "createwebdownload",
                vec![r#"{"success":true,"data":{"webdownload_id":1}}"#.into()],
            ),
            (
                "mylist",
                vec![r#"{"success":true,"data":{"download_state":"downloading"}}"#.into()],
            ),
        ])
        .await;
        let client = reqwest::Client::new();
        let err = fetch_from(&client, &base, "k", "https://h.example/f", quick())
            .await
            .unwrap_err();
        assert!(err.contains("still fetching"), "{err}");

        let before = seen.lock().unwrap().len();
        assert!(fetch_from(&client, &base, "  ", "https://h/f", quick())
            .await
            .is_err());
        assert!(fetch_from(&client, &base, "k", "magnet:?xt=1", quick())
            .await
            .is_err());
        assert_eq!(seen.lock().unwrap().len(), before);
    }

    #[tokio::test]
    async fn an_insecure_address_is_not_taken_and_the_key_stays_out_of_errors() {
        let (base, _) = serve(vec![
            (
                "createwebdownload",
                vec![r#"{"success":true,"data":{"webdownload_id":1}}"#.into()],
            ),
            (
                "mylist",
                vec![r#"{"success":true,"data":{"download_finished":true,"files":[{"id":0,"name":"a"}]}}"#.into()],
            ),
            (
                "requestdl",
                vec![r#"{"success":true,"data":"http://cdn.example/f"}"#.into()],
            ),
        ])
        .await;
        let client = reqwest::Client::new();
        let err = fetch_from(&client, &base, "k", "https://h.example/f", quick())
            .await
            .unwrap_err();
        assert!(err.contains("secure"), "{err}");

        // Nothing listens there: the failure must not print the address.
        let err = fetch_from(
            &client,
            "http://127.0.0.1:1",
            "SECRETKEY",
            "https://h/f",
            quick(),
        )
        .await
        .unwrap_err();
        assert!(!err.contains("SECRETKEY"), "{err}");
    }
}
