//! How a row's numbers are said. The same words the web window used, so a
//! person moving between them reads the same thing.

use std::time::Duration;

use crate::i18n::{t, tf};

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
    } else if f < K * K * K * K {
        format!("{:.2} GB", f / (K * K * K))
    } else {
        format!("{:.2} TB", f / (K * K * K * K))
    }
}

/// A rate, from the engine's bytes a second.
pub fn rate(per_second: f64) -> String {
    format!("{}/s", bytes(per_second.max(0.0) as u64))
}

/// Time remaining; nothing at all when there is nothing honest to say.
pub fn clock(eta: Option<Duration>) -> String {
    match eta.map(span) {
        Some(left) if !left.is_empty() => tf("{} left", &[&left]),
        _ => String::new(),
    }
}

/// A length of time in its two largest units, in the language in force:
/// "2d 3h", "5h 2m", "3m 4s", "42s"; nothing for none. Over a day it is
/// said in days — "1494h 36m" was read by nobody.
pub fn span(time: Duration) -> String {
    let s = time.as_secs();
    let (d, h, m) = (s / 86_400, (s % 86_400) / 3600, (s % 3600) / 60);
    if s == 0 {
        String::new()
    } else if d > 0 {
        tf("{}d {}h", &[&d, &h])
    } else if h > 0 {
        tf("{}h {}m", &[&h, &m])
    } else if m > 0 {
        tf("{}m {}s", &[&m, &(s % 60)])
    } else {
        tf("{}s", &[&s])
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
            // Masked after formatting: put into the URL, the dots were
            // percent-encoded and read "user:%E2%80%A2…@" (gap 146). The
            // first ":mask@" is the user's, whose own `:` and `@` are encoded.
            let _ = parsed.set_password(Some("dmmasked"));
            parsed.to_string().replacen(":dmmasked@", ":•••@", 1)
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

/// The months as a date in the list says them, in English; each goes
/// through the catalogs.
pub const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// When a finished row finished: the time today, the day and month this
/// year, and the year too before that — "3 Mar" of two years ago read as
/// this spring.
pub fn when(seconds: Option<u64>) -> String {
    use chrono::{Datelike, Local, TimeZone};
    let Some(seconds) = seconds else {
        return String::new();
    };
    let Some(at) = Local.timestamp_opt(seconds as i64, 0).single() else {
        return String::new();
    };
    let today = Local::now().date_naive();
    if at.date_naive() == today {
        return at.format("%H:%M").to_string();
    }
    let month = MONTHS
        .get(at.month0() as usize)
        .copied()
        .map(t)
        .unwrap_or_default();
    // The day and the month in the order, and with the marks, a language
    // writes them: "5. März" in German.
    if at.year() == today.year() {
        tf("{} {}", &[&at.day(), &month])
    } else {
        tf("{} {} {}", &[&at.day(), &month, &at.year()])
    }
}

/// Names in the order a person expects: case aside, and the numbers in
/// them by value, so "part 2" comes before "part 10".
pub fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars<'_>>| {
                    let mut digits = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(c);
                        it.next();
                    }
                    digits
                };
                let (p, q) = (take(&mut x), take(&mut y));
                let (p, q) = (p.trim_start_matches('0'), q.trim_start_matches('0'));
                let order = p.len().cmp(&q.len()).then_with(|| p.cmp(q));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(c), Some(d)) => {
                let order = c.to_lowercase().cmp(d.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                x.next();
                y.next();
            }
        }
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
        assert_eq!(bytes(1023 * 1024 * 1024 * 1024), "1023.00 GB");
        assert_eq!(bytes(3 * 1024 * 1024 * 1024 * 1024), "3.00 TB");
        assert_eq!(rate(2048.0), "2.0 KB/s");
    }

    #[test]
    fn time_left_is_said_only_when_there_is_some() {
        assert_eq!(clock(None), "");
        assert_eq!(clock(Some(Duration::ZERO)), "");
        assert_eq!(clock(Some(Duration::from_secs(42))), "42s left");
        assert_eq!(clock(Some(Duration::from_secs(125))), "2m 5s left");
        assert_eq!(clock(Some(Duration::from_secs(7260))), "2h 1m left");
        // Over a day, in days: not "1494h 36m left".
        let long = Duration::from_secs(1494 * 3600 + 36 * 60);
        assert_eq!(clock(Some(long)), "62d 6h left");
        assert_eq!(span(Duration::from_secs(7380)), "2h 3m", "no \"left\"");
    }

    #[test]
    fn time_left_is_said_in_the_language_in_force() {
        crate::i18n::set("de");
        let said = clock(Some(Duration::from_secs(90_000)));
        crate::i18n::set("en");
        assert_eq!(said, "noch 1 T. 1 h");
    }

    #[test]
    fn a_date_of_another_year_says_the_year_and_the_month_is_translated() {
        use chrono::{Datelike, Local, TimeZone};
        let year = Local::now().year();
        let then = Local.with_ymd_and_hms(year - 2, 3, 5, 12, 0, 0).unwrap();
        assert_eq!(
            when(Some(then.timestamp() as u64)),
            format!("5 Mar {}", year - 2)
        );
        crate::i18n::set("fr");
        let said = when(Some(then.timestamp() as u64));
        crate::i18n::set("en");
        assert_eq!(said, format!("5 mars {}", year - 2));
    }

    #[test]
    fn names_sort_with_their_numbers_by_value() {
        use std::cmp::Ordering::*;
        assert_eq!(natural("part 2.zip", "part 10.zip"), Less);
        assert_eq!(natural("Part 10", "part 9"), Greater);
        assert_eq!(natural("a", "B"), Less);
        assert_ne!(natural("file007", "file7"), Equal, "a total order");
        let mut names = vec!["x10", "x2", "X1", "x02b"];
        names.sort_by(|a, b| natural(a, b));
        assert_eq!(names, ["X1", "x2", "x02b", "x10"]);
    }

    #[test]
    fn a_password_in_an_address_is_never_shown() {
        let shown = shown_url("https://me:secret@e.com/f.zip");
        assert!(!shown.contains("secret"), "{shown}");
        assert!(shown.contains("me:"));
        // Dots a person can read, not their percent-encoding (gap 146).
        assert_eq!(shown, "https://me:•••@e.com/f.zip");
        assert_eq!(
            shown_url("https://a%40b:p%3Aw@e.com/f.zip"),
            "https://a%40b:•••@e.com/f.zip"
        );
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
