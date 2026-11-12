// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What a YouTube player script answers to its checks, cut and run as dm's own reader runs it
//! (`youtube/player.rs`, in the computer's own JavaScript engine): for a person looking into a
//! player that changed, when the weekly check (`.github/workflows/youtube-check.yml`) fails.
//!
//!     cargo run -p dm-core --example player_answers -- <player.js> [n-value]... [sig:signature]...
//!
//! With no values it asks two of each kind, made up. It prints each answer, or dm's words for
//! what in the player it did not know.

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: player_answers <player.js> [n-value]... [sig:signature]...");
        return ExitCode::from(2);
    };
    let player = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::from(2);
        }
    };
    let asked: Vec<String> = args.collect();
    let (mut ns, mut sigs): (Vec<&str>, Vec<&str>) = (Vec::new(), Vec::new());
    for value in &asked {
        match value.strip_prefix("sig:") {
            Some(sig) => sigs.push(sig),
            None => ns.push(value),
        }
    }
    if asked.is_empty() {
        ns = vec!["ZdbaQ6n9tvTeTJ1Xu", "aBcDeFgHiJkLmNoP"];
        sigs = vec!["AOq0QJ8wRQIhAJ2bQ2dOTmZQqkLXvq0VvMcSiY8yZ7LKzJbLf3zuYJ3eAiB8Hc2hFJ2y5yQy"];
    }
    let started = std::time::Instant::now();
    match dm_core::youtube::answers(&player, &ns, &sigs).await {
        Ok((n, sig)) => {
            for value in &ns {
                println!(
                    "n   {value} -> {}",
                    n.get(*value).map_or("?", String::as_str)
                );
            }
            for value in &sigs {
                println!(
                    "sig {value} -> {}",
                    sig.get(*value).map_or("?", String::as_str)
                );
            }
            println!("answered in {:.1} s", started.elapsed().as_secs_f64());
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("FAIL {e}");
            ExitCode::FAILURE
        }
    }
}
