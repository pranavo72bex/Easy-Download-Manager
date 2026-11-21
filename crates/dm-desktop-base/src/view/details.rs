// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The text "Copy details" puts on the clipboard for a row that failed: what a person can paste to
//! somebody who is helping them, with nothing secret in it. Never a cookie, a login, a token, a
//! path or a query: the address is its host alone, the headers are their names alone, and what the
//! reason says about an address is cut to its host too.

use dm_core::{Entry, State};

/// What the block says besides what the row holds.
pub struct Setting<'a> {
    /// dm's version.
    pub version: &'a str,
    /// The operating system, as `std::env::consts::OS` names it.
    pub os: &'a str,
    /// The moment it was asked for, written by the caller.
    pub when: &'a str,
    /// How many connections one download may use.
    pub connections: usize,
    /// The free space of the download folder's disk, where it could be read.
    pub free: Option<u64>,
}

/// The block for `row`.
pub fn block(row: &Entry, setting: &Setting) -> String {
    let (state, reason) = match &row.state {
        State::Queued => ("waiting", ""),
        State::Running => ("running", ""),
        State::Paused => ("stopped", ""),
        State::Completed => ("finished", ""),
        State::Failed(why) => ("failed", why.as_str()),
        State::Cancelled => ("cancelled", ""),
    };
    let status = status_code(reason);
    let reason = scrubbed(reason);
    let mut lines = vec![
        format!("dm {} on {}", setting.version, setting.os),
        setting.when.to_string(),
        format!("State: {state}"),
    ];
    if !reason.is_empty() {
        lines.push(format!("Reason: {reason}"));
    }
    lines.push(format!("Host: {}", host_of(&row.url)));
    lines.push(match status {
        Some(code) => format!("Status: {code}"),
        None => "Status: none".to_string(),
    });
    lines.push(format!("Headers sent: {}", headers_sent(row).join(", ")));
    lines.push(format!("Connections: {}", setting.connections));
    lines.push(match setting.free {
        Some(free) => format!("Free disk space: {}", crate::format::bytes(free)),
        None => "Free disk space: unknown".to_string(),
    });
    lines.join("\n")
}

/// The host an address names, and nothing else of it.
fn host_of(address: &str) -> String {
    url::Url::parse(address)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_string())
}

/// The HTTP status a failure reason names ("the server answered 403", "HTTP/1.1 404 Not Found"):
/// the word before it is what tells it from a number in an address.
fn status_code(reason: &str) -> Option<u16> {
    let words: Vec<&str> = reason.split_whitespace().collect();
    words.windows(2).find_map(|pair| {
        let before = pair[0].to_ascii_lowercase();
        let names_one = before == "answered"
            || before == "status"
            || before == "error"
            || before.starts_with("http/");
        let code = pair[1].trim_end_matches(|c: char| !c.is_ascii_digit());
        match code.parse::<u16>() {
            Ok(code) if names_one && (100..600).contains(&code) => Some(code),
            _ => None,
        }
    })
}

/// The reason with every address in it cut to its host: a failure often says which address, and
/// that may carry a login or a signed token.
fn scrubbed(text: &str) -> String {
    text.split_whitespace()
        .map(|word| {
            if !word.contains("://") {
                // A folder named in a failure carries the account's name: a word
                // with a backslash in it, or one that starts like an absolute path,
                // is not given away.
                if looks_like_path(word) {
                    return "(path hidden)".to_string();
                }
                return word.to_string();
            }
            let start = word.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(0);
            let address = word[start..].trim_end_matches(['.', ',', ';', ')', '"', '\'', ':']);
            match url::Url::parse(address)
                .ok()
                .and_then(|parsed| parsed.host_str().map(str::to_owned))
            {
                Some(host) => host,
                None => "(address hidden)".to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether a word of a reason reads as part of a file path: it has a backslash, or starts
/// like an absolute path (a drive, a slash, `~`), after any opening bracket or quote.
fn looks_like_path(word: &str) -> bool {
    let word = word.trim_start_matches(['(', '[', '"', '\'']);
    let mut chars = word.chars();
    let drive = matches!((chars.next(), chars.next()), (Some(d), Some(':')) if d.is_ascii_alphabetic())
        && matches!(chars.next(), Some('/' | '\\'));
    word.contains('\\') || drive || word.starts_with('/') || word.starts_with('~')
}

/// The names of the headers a request of this row carries; the values of the ones that hold a
/// secret are never shown.
fn headers_sent(row: &Entry) -> Vec<&'static str> {
    let mut names = vec!["User-Agent", "Accept-Encoding", "Range"];
    if row
        .validator
        .as_deref()
        .is_some_and(|validator| !validator.starts_with("W/"))
    {
        names.push("If-Range");
    }
    names.push("Referer");
    if !row.cookies.is_empty() {
        names.push("Cookie (value not shown)");
    }
    let signed_in = url::Url::parse(&row.url)
        .map(|parsed| !parsed.username().is_empty())
        .unwrap_or(false);
    if signed_in {
        names.push("Authorization (value not shown)");
    }
    names
}

/// The block of each failed row among `rows`, one after another, and how many there are; `None`
/// when none of them failed.
pub fn blocks_for(
    rows: &[Entry],
    connections: usize,
    download_dir: &std::path::Path,
) -> Option<(String, usize)> {
    let now = dm_core::clock::local_now();
    let when = format!(
        "{} {:02}:{:02} UTC{}",
        now.date,
        now.hour,
        now.minute,
        now.offset_text()
    );
    let setting = Setting {
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        when: &when,
        connections,
        free: dm_core::free_space(download_dir),
    };
    let blocks: Vec<String> = rows
        .iter()
        .filter(|row| matches!(row.state, State::Failed(_)))
        .map(|row| block(row, &setting))
        .collect();
    if blocks.is_empty() {
        None
    } else {
        Some((blocks.join("\n\n"), blocks.len()))
    }
}

/// Help › "Copy a report" (gaps row W8): what a person can paste into an issue, sent nowhere:
/// dm's version and the system, how many downloads there are and how many did not finish, and
/// for each that failed its block, as "Copy details" writes it, with no path, query, cookie,
/// sign-in or name of a file in it.
pub fn report(rows: &[Entry], connections: usize, download_dir: &std::path::Path) -> String {
    let failed = rows
        .iter()
        .filter(|row| matches!(row.state, State::Failed(_)))
        .count();
    let mut text = format!(
        "dm {} on {} ({})\nDownloads: {}, failed: {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        rows.len(),
        failed
    );
    match blocks_for(rows, connections, download_dir) {
        Some((blocks, _)) => {
            text.push_str("\n\n");
            text.push_str(&blocks);
        }
        None => text.push_str("\nNo download failed."),
    }
    text
}

#[cfg(test)]
mod tests;
