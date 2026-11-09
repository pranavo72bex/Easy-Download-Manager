// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What dm's own YouTube reader makes of each address, with no sign-in: how
//! many formats it found and the best height, or its words where it failed.
//! For gaps row 122, which waits on a measurement before reading more from
//! the page:
//!
//!     cargo run -p dm-core --example youtube_read -- <watch-url>...
//!     DM_CODEC=av01 cargo run -p dm-core --example youtube_read -- <watch-url>...
//!
//! It reads only; nothing is downloaded. With DM_FETCH_FIRST set, it also asks
//! for the first kilobyte of each stream it chose, as a download does, and says
//! whether YouTube accepted the answers to its checks (`ACCEPTED`) or refused
//! them (`REFUSED`): the weekly check (`.github/workflows/youtube-check.yml`).
//! Its staging folder is a temporary one.

use dm_core::youtube;
use dm_core::Quality;

#[tokio::main]
async fn main() {
    let urls: Vec<String> = std::env::args().skip(1).collect();
    if urls.is_empty() {
        eprintln!("usage: youtube_read <watch-url>...");
        std::process::exit(2);
    }
    let staging = tempfile::tempdir().expect("no temporary folder");
    // Where a JavaScript engine is fetched (a system with none of its own and
    // none installed), into a folder of this run's, as the app fetches it into
    // its own.
    dm_core::tools::set_folder(staging.path().join("tools"));
    let fetch_first = std::env::var_os("DM_FETCH_FIRST").is_some();
    let client = dm_core::client();
    for url in urls {
        let started = std::time::Instant::now();
        // With DM_CODEC (av01, as a GIF on Linux asks), the picture in that
        // codec, from MP4 streams as a part is, at most 720 tall.
        let read = match std::env::var("DM_CODEC").ok().as_deref() {
            Some(codec) => {
                youtube::read_at(
                    &client,
                    &url,
                    Quality::UpTo(720),
                    Some("mp4"),
                    None,
                    None,
                    true,
                    Some(codec),
                    None,
                    staging.path(),
                )
                .await
            }
            None => {
                youtube::read(
                    &client,
                    &url,
                    Quality::Best,
                    None,
                    false,
                    None,
                    staging.path(),
                )
                .await
            }
        };
        let took = started.elapsed().as_secs_f64();
        match read {
            Ok(json) => {
                let value: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
                let formats = value["formats"].as_array().map_or(0, Vec::len);
                let best = value["formats"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|f| f["height"].as_u64())
                    .max()
                    .unwrap_or(0);
                let with_address = value["formats"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|f| f["url"].as_str().is_some_and(|u| u.starts_with("https://")))
                    .count();
                // With DM_SHOW_ADDRESS set, the first stream's address, to be
                // fetched by other means (gaps row 129: a token bound to an IP).
                if std::env::var_os("DM_SHOW_ADDRESS").is_some() {
                    if let Some(first) = value["formats"]
                        .as_array()
                        .and_then(|f| f.iter().find_map(|f| f["url"].as_str()))
                    {
                        println!("ADDRESS {first}");
                    }
                }
                if let Some(player) = value["player"].as_str() {
                    println!("PLAYER {player}");
                }
                let chosen: Vec<&serde_json::Value> = match value["requested_formats"].as_array() {
                    Some(pair) => pair.iter().collect(),
                    None => vec![&value],
                };
                for chosen in chosen {
                    // The address's `xtags` where it has them: which sound track
                    // (`acont=original:lang=es`) a dubbed video gave.
                    let tags = chosen["url"]
                        .as_str()
                        .and_then(|u| url::Url::parse(u).ok())
                        .and_then(|u| {
                            u.query_pairs()
                                .find(|(k, _)| k == "xtags")
                                .map(|(_, v)| format!(" {v}"))
                        })
                        .unwrap_or_default();
                    println!(
                        "CHOSE {} {} {}{tags}",
                        chosen["vcodec"].as_str().unwrap_or(""),
                        chosen["acodec"].as_str().unwrap_or(""),
                        chosen["height"]
                            .as_u64()
                            .map_or(String::new(), |h| format!("{h}p"))
                    );
                    if fetch_first {
                        println!("{}", first_kilobyte(&client, chosen).await);
                    }
                }
                println!(
                    "OK    {url}  {formats} formats, {with_address} with an address, best {best}p, {took:.1} s, title {:?}",
                    value["title"].as_str().unwrap_or("")
                );
            }
            Err(e) => println!("FAIL  {url}  {e}  ({took:.1} s)"),
        }
    }
}

/// Whether YouTube gives the first kilobyte of a chosen stream, asked for as a download asks:
/// a redirect followed (dm follows them itself), and a refusal asked again twice, five seconds
/// apart, as a download's own retries would. YouTube refuses a stream now and then that it
/// gives a moment later; the weekly check is for answers it never takes.
async fn first_kilobyte(client: &dm_core::reqwest::Client, chosen: &serde_json::Value) -> String {
    let Some(url) = chosen["url"].as_str() else {
        return "REFUSED no address".into();
    };
    let itag = chosen["format_id"].as_str().unwrap_or("?");
    let mut last = String::new();
    for attempt in 1..=3 {
        if attempt > 1 {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
        match once(client, url, chosen).await {
            Ok((status, bytes)) => {
                let tries = if attempt > 1 {
                    format!(", try {attempt}")
                } else {
                    String::new()
                };
                return format!("ACCEPTED {itag} {status} {bytes} bytes{tries}");
            }
            Err(why) => last = why,
        }
    }
    format!("REFUSED {itag} {last}")
}

/// One ask for the first kilobyte at `url`, its redirects followed: the status and the bytes
/// given, or why none were.
async fn once(
    client: &dm_core::reqwest::Client,
    url: &str,
    chosen: &serde_json::Value,
) -> Result<(u16, usize), String> {
    let mut url = url.to_string();
    for _ in 0..5 {
        let mut ask = client.get(&url).header("Range", "bytes=0-1023");
        if let Some(headers) = chosen["http_headers"].as_object() {
            for (name, value) in headers {
                if let Some(value) = value.as_str() {
                    ask = ask.header(name.as_str(), value);
                }
            }
        }
        let answer = ask.send().await.map_err(|e| e.to_string())?;
        let status = answer.status();
        if status.is_redirection() {
            let next = answer
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or("a redirect to nowhere")?;
            url = answer
                .url()
                .join(next)
                .map_err(|e| e.to_string())?
                .to_string();
            continue;
        }
        if !status.is_success() {
            let from = answer
                .remote_addr()
                .map_or(String::new(), |a| format!(" from {a}"));
            // With DM_SHOW_ADDRESS, the address refused, to be fetched by other means.
            let shown =
                std::env::var_os("DM_SHOW_ADDRESS").map_or(String::new(), |_| format!(" {url}"));
            return Err(format!("{}{from}{shown}", status.as_u16()));
        }
        let bytes = answer.bytes().await.map_err(|e| e.to_string())?;
        if bytes.is_empty() {
            return Err(format!("{} with nothing in it", status.as_u16()));
        }
        return Ok((status.as_u16(), bytes.len()));
    }
    Err("redirected more than five times".into())
}
