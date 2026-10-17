//! Runs a real queue and prints what the window would be shown.
//!
//! The README's screenshots claim to be a queue the engine really was
//! serving. This is what makes that true and repeatable: it downloads the
//! URLs it is given, and every 400 ms prints the exact JSON that `invoke
//! ('list')` returns to the window — same struct, same serialiser, same
//! field names. `scripts/screenshots.mjs` feeds one of those lines to
//! `desktop/ui/index.html` in a browser and photographs the result.
//!
//!     cargo run -p dm-core --example snapshot -- <dir> <url>...
//!
//! Downloads land in `<dir>`, which is expected to be a temporary one.

use std::path::PathBuf;
use std::time::Duration;

use dm_core::stream::Quality;
use dm_core::{client, Categories, Control, Limiter, Options, Queue};

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let Some(dir) = args.next().map(PathBuf::from) else {
        eprintln!("usage: snapshot <dir> <url>...");
        std::process::exit(2);
    };
    let urls: Vec<String> = args.collect();
    if urls.is_empty() {
        eprintln!("usage: snapshot <dir> <url>...");
        std::process::exit(2);
    }

    let queue = Queue::new(
        client(),
        Options {
            connections: 4,
            control: Control::new(),
            dest_dir: dir.join("done"),
            parts_dir: dir.join("parts"),
            limiter: Limiter::default(),
            // On, because a finished row that says which folder it was sorted
            // into is one of the things the picture is meant to show.
            categories: Categories {
                enabled: true,
                ..Default::default()
            },
            checksum: None,
            quality: Quality::default(),
            referrer: None,
            proxy: None,
            container: None,
            attempts: 5,
            host_limits: Vec::new(),
            proxy_bypass: Vec::new(),
            subtitles: false,
            remux: false,
            sign_in: None,
            system_proxy: false,
        },
        3,
    );

    for url in &urls {
        queue.add(url).await;
    }

    let reporter = {
        let queue = queue.clone();
        tokio::spawn(async move {
            loop {
                let rows = queue.list().await;
                println!("{}", serde_json::to_string(&rows).unwrap());
                tokio::time::sleep(Duration::from_millis(400)).await;
            }
        })
    };

    queue.run().await;
    reporter.abort();

    // A closing line, so the caller always has a settled queue to draw as
    // well as the ones in flight.
    println!("{}", serde_json::to_string(&queue.list().await).unwrap());
}
