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
                return word.to_string();
            }
            let start = word.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(0);
            let address = word[start..]
                .trim_end_matches(|c: char| matches!(c, '.' | ',' | ';' | ')' | '"' | '\'' | ':'));
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
    let when = chrono::Local::now()
        .format("%Y-%m-%d %H:%M UTC%:z")
        .to_string();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn failed(url: &str, reason: &str, cookies: &str, referrer: &str) -> Entry {
        serde_json::from_value(serde_json::json!({
            "id": 7,
            "url": url,
            "state": { "kind": "failed", "reason": reason },
            "progress": { "downloaded": 0, "total": null, "speed": 0.0, "eta": null },
            "filename": "a.bin",
            "cookies": cookies,
            "referrer": referrer,
        }))
        .expect("an entry")
    }

    fn setting() -> Setting<'static> {
        Setting {
            version: "0.8.6",
            os: "windows",
            when: "2026-11-01 09:30 UTC+05:45",
            connections: 8,
            free: Some(5 * 1024 * 1024 * 1024),
        }
    }

    #[test]
    fn a_row_carrying_a_login_and_a_token_gives_away_neither() {
        let row = failed(
            "http://user:hunter2@files.example.org:8080/private/dir/a.bin?token=abc123&sig=zzz9",
            "the server answered 403 Forbidden for http://user:hunter2@files.example.org:8080/private/dir/a.bin?token=abc123&sig=zzz9",
            "session=COOKIEVALUE; other=COOKIEVALUE2",
            "https://page.example.net/watch?v=1&auth=REFERERSECRET",
        );
        let text = block(&row, &setting());
        for secret in [
            "hunter2",
            "user:",
            "abc123",
            "zzz9",
            "COOKIEVALUE",
            "REFERERSECRET",
            "/private",
            "dir/a.bin",
            "token",
            "?",
        ] {
            assert!(!text.contains(secret), "{secret:?} is in:\n{text}");
        }
        assert!(text.contains("Host: files.example.org"), "{text}");
        assert!(text.contains("Status: 403"), "{text}");
        assert!(text.contains("Cookie (value not shown)"), "{text}");
        assert!(text.contains("Authorization (value not shown)"), "{text}");
        assert!(text.contains("Referer"), "{text}");
    }

    #[test]
    fn the_block_says_the_version_the_system_the_state_and_the_disk() {
        let row = failed(
            "https://example.com/f.bin",
            "the connection was refused",
            "",
            "",
        );
        let text = block(&row, &setting());
        assert!(
            text.starts_with("dm 0.8.6 on windows\n2026-11-01 09:30 UTC+05:45\nState: failed\n"),
            "{text}"
        );
        assert!(
            text.contains("Reason: the connection was refused"),
            "{text}"
        );
        assert!(text.contains("Status: none"), "{text}");
        assert!(text.contains("Connections: 8"), "{text}");
        assert!(text.contains("Free disk space: 5"), "{text}");
        assert!(!text.contains("Cookie"), "{text}");
        assert!(!text.contains("Authorization"), "{text}");
    }

    #[test]
    fn a_status_is_read_from_the_words_and_not_from_an_address() {
        assert_eq!(status_code("the server answered 404 Not Found"), Some(404));
        assert_eq!(
            status_code("Details: HTTP/1.1 503 Service Unavailable"),
            Some(503)
        );
        assert_eq!(status_code("could not reach 127.0.0.1:443"), None);
        assert_eq!(status_code("the connection was refused"), None);
        assert_eq!(status_code("the server answered 999"), None);
    }

    #[test]
    fn an_address_in_a_reason_is_cut_to_its_host() {
        assert_eq!(
            scrubbed("no good (http://u:p@h.example/a/b?c=d), sorry"),
            "no good h.example sorry"
        );
        assert_eq!(scrubbed("see ://broken"), "see (address hidden)");
        assert_eq!(scrubbed("plain words only"), "plain words only");
    }

    #[test]
    fn a_finished_or_running_row_has_no_reason_line() {
        let mut row = failed("https://example.com/f.bin", "x", "", "");
        row.state = State::Running;
        let text = block(&row, &setting());
        assert!(text.contains("State: running"), "{text}");
        assert!(!text.contains("Reason:"), "{text}");
    }
}
