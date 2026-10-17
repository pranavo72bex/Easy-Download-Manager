//! How a row's numbers are said. The same words the web window used, so a
//! person moving between them reads the same thing.

use std::time::Duration;

/// A byte count in the unit a person reads it in.
pub fn bytes(n: u64) -> String {
    const K: f64 = 1024.0;
    let f = n as f64;
    if n < 1024 {
        format!("{n} B")
    } else if f < K * K {
        format!("{:.1} KB", f / K)
    } else if f < K * K * K {
        format!("{:.1} MB", f / (K * K))
    } else {
        format!("{:.2} GB", f / (K * K * K))
    }
}

/// A rate, from the engine's bytes a second.
pub fn rate(per_second: f64) -> String {
    format!("{}/s", bytes(per_second.max(0.0) as u64))
}

/// Time remaining; nothing at all when there is nothing honest to say.
pub fn clock(eta: Option<Duration>) -> String {
    let Some(eta) = eta else {
        return String::new();
    };
    let s = eta.as_secs();
    if s == 0 {
        return String::new();
    }
    let (h, m) = (s / 3600, (s % 3600) / 60);
    if h > 0 {
        format!("{h}h {m}m left")
    } else if m > 0 {
        format!("{m}m {}s left", s % 60)
    } else {
        format!("{s}s left")
    }
}

/// Where a URL came from, which is the second thing anybody wants to know
/// about a download. A magnet has no host — it is a swarm, not a server.
pub fn host(url: &str) -> String {
    if url.starts_with("magnet:") {
        return "torrent".into();
    }
    url::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| h.trim_start_matches("www.").to_string())
        })
        .unwrap_or_default()
}

/// An address as the window shows it: without a password in it.
///
/// "Needs a sign-in" and Sign-ins put a user and password in the
/// address; a list, a tooltip or a screenshot must not show the password.
pub fn shown_url(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut parsed) if parsed.password().is_some() => {
            let _ = parsed.set_password(Some("•••"));
            parsed.to_string()
        }
        _ => url.to_string(),
    }
}

/// What to call a row the probe has not reached yet: the last path segment,
/// which is the engine's own fallback, or a magnet's `dn`.
pub fn provisional(url: &str) -> String {
    let Ok(parsed) = url::Url::parse(url) else {
        return url.to_string();
    };
    if parsed.scheme() == "magnet" {
        return parsed
            .query_pairs()
            .find(|(k, _)| k == "dn")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_else(|| "Magnet link".into());
    }
    let last = parsed
        .path_segments()
        .and_then(|mut s| s.next_back().map(str::to_owned))
        .unwrap_or_default();
    let last = percent_decode(&last);
    if last.is_empty() {
        host(url)
    } else {
        last
    }
}

fn percent_decode(text: &str) -> String {
    url::form_urlencoded::parse(format!("x={}", text.replace('+', "%2B")).as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| text.to_string())
}

/// When a finished row finished: the time today, the date otherwise.
pub fn when(seconds: Option<u64>) -> String {
    use chrono::{Local, TimeZone};
    let Some(seconds) = seconds else {
        return String::new();
    };
    let Some(at) = Local.timestamp_opt(seconds as i64, 0).single() else {
        return String::new();
    };
    if at.date_naive() == Local::now().date_naive() {
        at.format("%H:%M").to_string()
    } else {
        at.format("%-d %b").to_string()
    }
}

/// A category as prose rather than as the wire spells it.
pub fn titled(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_the_way_the_web_window_read_them() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(1536), "1.5 KB");
        assert_eq!(bytes(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(bytes(3 * 1024 * 1024 * 1024), "3.00 GB");
        assert_eq!(rate(2048.0), "2.0 KB/s");
    }

    #[test]
    fn time_left_is_said_only_when_there_is_some() {
        assert_eq!(clock(None), "");
        assert_eq!(clock(Some(Duration::ZERO)), "");
        assert_eq!(clock(Some(Duration::from_secs(42))), "42s left");
        assert_eq!(clock(Some(Duration::from_secs(125))), "2m 5s left");
        assert_eq!(clock(Some(Duration::from_secs(7260))), "2h 1m left");
    }

    #[test]
    fn a_password_in_an_address_is_never_shown() {
        let shown = shown_url("https://me:secret@e.com/f.zip");
        assert!(!shown.contains("secret"), "{shown}");
        assert!(shown.contains("me:"));
        assert_eq!(shown_url("https://e.com/f.zip"), "https://e.com/f.zip");
    }

    #[test]
    fn a_row_is_named_before_the_server_names_it() {
        assert_eq!(
            provisional("https://www.e.com/a/My%20File.zip?x=1"),
            "My File.zip"
        );
        assert_eq!(provisional("https://e.com/"), "e.com");
        assert_eq!(
            provisional("magnet:?xt=urn:btih:abc&dn=Big+Film"),
            "Big Film"
        );
        assert_eq!(provisional("magnet:?xt=urn:btih:abc"), "Magnet link");
        assert_eq!(host("https://www.e.com/a"), "e.com");
        assert_eq!(host("magnet:?xt=x"), "torrent");
    }
}
