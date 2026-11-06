//! What dm's own YouTube reader makes of each address, with no sign-in: how
//! many formats it found and the best height, or its words where it failed.
//! For gaps row 122, which waits on a measurement before reading more from
//! the page:
//!
//!     cargo run -p dm-core --example youtube_read -- <watch-url>...
//!     DM_CODEC=av01 cargo run -p dm-core --example youtube_read -- <watch-url>...
//!
//! It reads only; nothing is downloaded. Its staging folder, and the tools it
//! fetches for YouTube's challenges, are in a temporary one.

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
    // The JavaScript engine and the solver YouTube's challenges need, fetched
    // into a folder of this run's (checked against their digests), as the
    // app fetches them into its own.
    dm_core::tools::set_folder(staging.path().join("tools"));
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
                for chosen in value["requested_formats"].as_array().into_iter().flatten() {
                    println!(
                        "CHOSE {} {} {}",
                        chosen["vcodec"].as_str().unwrap_or(""),
                        chosen["acodec"].as_str().unwrap_or(""),
                        chosen["height"]
                            .as_u64()
                            .map_or(String::new(), |h| format!("{h}p"))
                    );
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
