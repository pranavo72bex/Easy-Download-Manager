//! How a row's numbers are said. The same words the web window used, so a
//! person moving between them reads the same thing.

use std::time::Duration;

use crate::i18n::{t, tf};

/// A byte count in the unit a person reads it in.
pub fn bytes(n: u64) -> String {
    bytes_marked(n, crate::region::system().map(|r| r.decimal))
}

/// [`bytes`] with the computer's regional decimal mark, where it has one.
pub fn bytes_marked(n: u64, regional: Option<char>) -> String {
    const K: f64 = 1024.0;
    let f = n as f64;
    // The units in the language in force: Russian's КБ and МБ, French's
    // Ko and Mo; the status bar said КБ/с beside a list saying KB/s. And
    // its decimal mark: "3,8 MB" in German and Russian, where "3.8" read
    // as a thousands mark.
    // The computer's regional mark where it has one, as a person set it.
    let regional = regional.map(|c| c.to_string());
    let mark = regional
        .as_deref()
        .unwrap_or_else(|| unit(crate::i18n::t("unit:.")));
    let number = |value: f64, places: usize| format!("{value:.places$}").replace('.', mark);
    if n < 1024 {
        return format!("{n} {}", unit(crate::i18n::t("unit:B")));
    }
    // Each unit with its places; the unit chosen once the number is
    // rounded, so a size just under 1 MB reads "1.0 MB", not "1024.0 KB".
    let units = [
        ("unit:KB", 1),
        ("unit:MB", 1),
        ("unit:GB", 2),
        ("unit:TB", 2),
    ];
    let mut value = f / K;
    let mut at = 0;
    while at + 1 < units.len() {
        let places = units[at].1;
        let scale = 10f64.powi(places as i32);
        if (value * scale).round() / scale < K {
            break;
        }
        value /= K;
        at += 1;
    }
    let (word, places) = units[at];
    let word = match word {
        "unit:KB" => crate::i18n::t("unit:KB"),
        "unit:MB" => crate::i18n::t("unit:MB"),
        "unit:GB" => crate::i18n::t("unit:GB"),
        _ => crate::i18n::t("unit:TB"),
    };
    format!("{} {}", number(value, places), unit(word))
}

/// A unit's word: its translation, or the key without its `unit:`.
fn unit(word: &'static str) -> &'static str {
    word.strip_prefix("unit:").unwrap_or(word)
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
    if time.is_zero() {
        String::new()
    } else if s == 0 {
        // Under a second, said: Time left went blank at 99%.
        format!("<{}", tf("{}s", &[&1]))
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
/// An address to copy: without any sign-in it carries, which belongs to
/// the row and not to wherever the link is pasted.
pub fn copyable_url(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut parsed) if !parsed.username().is_empty() || parsed.password().is_some() => {
            let _ = parsed.set_password(None);
            let _ = parsed.set_username("");
            parsed.to_string()
        }
        _ => url.to_string(),
    }
}

pub fn shown_url(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut parsed) if parsed.password().is_some() => {
            // Masked after formatting: put into the URL, the dots were
            // percent-encoded and read "user:%E2%80%A2…@". The
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
        return host(url);
    }
    // A page made by a script, named by its query (`watch?v=…`), is shown
    // by its short address until it is named: a failed YouTube video's row
    // said only "watch".
    match parsed.query().filter(|q| !q.is_empty()) {
        Some(query) if !last.contains('.') => {
            let host = parsed.host_str().unwrap_or_default();
            let host = host.strip_prefix("www.").unwrap_or(host);
            let path = parsed.path().trim_start_matches('/');
            let shown = format!("{host}/{path}?{query}");
            match shown.char_indices().nth(60) {
                Some((at, _)) => format!("{}…", &shown[..at]),
                None => shown,
            }
        }
        _ => last,
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
    let region = crate::region::system();
    if at.date_naive() == today {
        // On a 12-hour clock where the computer keeps one.
        return if region.is_some_and(|r| r.hour12) {
            at.format("%-I:%M %p").to_string()
        } else {
            at.format("%H:%M").to_string()
        };
    }
    let month = MONTHS
        .get(at.month0() as usize)
        .copied()
        .map(t)
        .unwrap_or_default();
    // The day and the month in the order, and with the marks, a language
    // writes them: "5. März" in German.
    // In the order the computer writes dates, where it says one.
    use crate::region::Order;
    let this_year = at.year() == today.year();
    match region.map(|r| r.order) {
        Some(Order::MonthDayYear) if this_year => format!("{month} {}", at.day()),
        Some(Order::MonthDayYear) => format!("{month} {}, {}", at.day(), at.year()),
        Some(Order::YearMonthDay) if this_year => format!("{month} {}", at.day()),
        Some(Order::YearMonthDay) => format!("{} {month} {}", at.year(), at.day()),
        _ if this_year => tf("{} {}", &[&at.day(), &month]),
        _ => tf("{} {} {}", &[&at.day(), &month, &at.year()]),
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
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

/// The common engine failures in plain words, by what their text holds
/// (lower case): the engine's own sentence where it has one, and a plain
/// one for what the network library says in its own terms ("error decoding
/// response body" is a connection cut mid-file). The first that matches
/// wins; anything else is shown as the engine said it.
pub const REASONS: [(&[&str], &str); 32] = [
    (
        &[
            "error decoding response body",
            "error reading a body",
            "unexpected eof",
            "unexpected end of file",
            "connection reset",
            "connection closed before",
            "broken pipe",
            "the connection ended after",
            "incomplete message",
        ],
        "the server ended the connection before the file was complete",
    ),
    (
        &["the server sent ", "bytes of a file it said was"],
        "the server sent less than the whole file",
    ),
    (
        &["too many redirects"],
        "the site sends the address round in a loop of redirects",
    ),
    (
        &["the server's name could not be found"],
        "the server's name could not be found (check the address)",
    ),
    (
        &["the server refused the connection"],
        "the server refused the connection",
    ),
    (
        &["the server did not answer in time", "timed out"],
        "the server did not answer in time",
    ),
    (
        &["certificate"],
        "the server's security certificate could not be checked",
    ),
    (
        &["video helper is not fetched"],
        "a video page needs dm's video helper, which Options › Downloads says not to fetch: install yt-dlp, or let dm fetch it",
    ),
    (
        &["cannot be reached (was its drive"],
        "its folder's drive cannot be reached; plug it back in, or choose another folder",
    ),
    (
        &["insecure (http)"],
        "the secure address sent dm on to an insecure one, where the file could be swapped on the way, so dm stopped",
    ),
    (
        &["copy-protected", "drm protected"],
        "this video is copy-protected (DRM); only the site's own player can play it, and dm does not download it",
    ),
    (
        &["no space left", "os error 28", "disk full"],
        "the disk is full",
    ),
    (
        &["too large for this drive"],
        "the file is too large for this drive, which holds no file over 4 GB (FAT32): choose a folder on another drive",
    ),
    (
        &["disk: permission denied", "os error 13"],
        "dm may not write to that folder",
    ),
    (
        &["the server answered 404"],
        "this link no longer works",
    ),
    (
        &["error decoding torrent", "this file is not a torrent"],
        "the file is not a torrent",
    ),
    (
        &["could not read the torrent file"],
        "the .torrent file could not be read",
    ),
    // Raw engine text shown for these.
    (
        &["the server answered 401"],
        "the server wants a sign-in (Properties › Login, or Options › Sign-ins for the whole site)",
    ),
    (
        &[
            "socks error",
            "proxy connect",
            "error connecting to proxy",
            "proxy server",
        ],
        "the proxy could not be reached",
    ),
    // A file's own errors, as `dm_core::file_error` words them.
    (&["it is not there"], "it is not there"),
    (
        &["dm is not allowed to use it"],
        "dm is not allowed to use it",
    ),
    (
        &["something of that name is already there"],
        "something of that name is already there",
    ),
    // The server's own trouble, and its refusals, said as what to do: the
    // row read "the server answered 500 Internal Server Error for http://…".
    (
        &[
            "the server answered 500",
            "the server answered 502",
            "the server answered 503",
            "the server answered 504",
        ],
        "the server had a problem on its side: try again later",
    ),
    (
        &["the server answered 429"],
        "the site is turning away too many requests just now: try again later",
    ),
    (
        &["the server answered 403"],
        "the site would not give dm the file: it may need you signed in (Properties › Login)",
    ),
    // Video pages: yt-dlp and QuickJS are "dm's video helper" to a person,
    // who never installs them (dm fetches them).
    (
        &["video helper"],
        "dm could not get ready for videos: check that you are online, then click Retry",
    ),
    (
        &[
            "yt-dlp does not know this site",
            "yt-dlp could not read that page",
        ],
        "dm found no video on this page",
    ),
    // What yt-dlp says most, from YouTube and the like.
    (
        &["confirm you're not a bot", "confirm you’re not a bot", "sign in to confirm"],
        "the site wants to check you are not a bot: play the video in your browser, then use its Download video button",
    ),
    (
        &[
            "video unavailable",
            "private video",
            "this video is not available",
            "has been removed",
            "not available in your country",
        ],
        "this video is not available: it may be private, removed or not shown in your country",
    ),
    (&["yt-dlp could not download that"], VIDEO_NOT_GOT),
    (
        &["an encrypted stream dm cannot record"],
        "dm cannot record this kind of encrypted stream",
    ),
    // Last: the specific failures above say more.
    (
        &["error sending request"],
        "the server could not be reached",
    ),
];

/// What a video page that yt-dlp failed on is said to be when nothing more
/// specific is known (`REASONS`), whatever yt-dlp's own words were.
pub const VIDEO_NOT_GOT: &str = "dm could not get the video from this page";

/// yt-dlp's own words for why it could not download a page, for the status to
/// say after [`VIDEO_NOT_GOT`]: its last `ERROR:` line, without the extractor
/// and video id it starts with and without any address (one may be signed),
/// and short enough for a row. `None` for a failure that is not yt-dlp's or
/// that said nothing.
pub fn ytdlp_said(raw: &str) -> Option<String> {
    let rest = raw.strip_prefix("yt-dlp could not download that:")?;
    let line = rest
        .lines()
        .map(str::trim)
        .rev()
        .find(|l| l.contains("ERROR:"))
        .or_else(|| rest.lines().map(str::trim).rev().find(|l| !l.is_empty()))?;
    let mut said = line.rsplit("ERROR:").next()?.trim();
    // "[youtube] jNQXAC9IVRw: text": the extractor, then the id, then the words.
    if let Some(after) = said.strip_prefix('[').and_then(|s| s.split_once(']')) {
        said = after.1.trim_start();
        if let Some((id, text)) = said.split_once(": ") {
            if !id.contains(' ') {
                said = text;
            }
        }
    }
    let said: String = said
        .split_whitespace()
        .filter(|word| !word.contains("://"))
        .collect::<Vec<_>>()
        .join(" ");
    let mut chars = said.chars();
    let short: String = chars.by_ref().take(120).collect();
    let said = if chars.next().is_some() {
        format!("{}…", short.trim_end())
    } else {
        short
    };
    (!said.is_empty()).then_some(said)
}

/// Why a download is being tried again, in two or three words that fit
/// the Status column beside the attempt: the server's own trouble, its
/// turning dm away briefly, and the network failures `REASONS` names.
/// `None` for anything else, which is then not guessed at.
pub fn retry_cause(raw: &str) -> Option<&'static str> {
    let lower = raw.to_lowercase();
    if ["500", "502", "503", "504"]
        .iter()
        .any(|code| lower.starts_with(&format!("the server answered {code}")))
    {
        return Some("Server error");
    }
    if lower.starts_with("the server answered 429") {
        return Some("Too many requests");
    }
    match reason(raw)? {
        "the server ended the connection before the file was complete" => {
            Some("Connection dropped")
        }
        "the server's name could not be found (check the address)" => Some("Server not found"),
        "the server refused the connection" => Some("Connection refused"),
        "the server did not answer in time" => Some("No answer"),
        "the server could not be reached" => Some("Server unreachable"),
        _ => None,
    }
}

/// Every word [`retry_cause`] can give, for the catalogs.
pub const RETRY_CAUSES: [&str; 7] = [
    "Server error",
    "Too many requests",
    "Connection dropped",
    "Server not found",
    "Connection refused",
    "No answer",
    "Server unreachable",
];

/// A file error in the window's language, without the system's number.
pub fn file_error(e: &std::io::Error) -> String {
    let words = dm_core::file_error(e);
    reason(&words).map(t).map(String::from).unwrap_or(words)
}

/// A `file://` address for an absolute path, every byte but the
/// unreserved ones and `/` escaped, as a file manager reads it back.
pub fn file_uri(path: &str) -> String {
    let mut uri = String::from("file://");
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

/// A failure's plain reason, in English, if it is one dm knows; see
/// `REASONS`.
///
/// Only the words of dm and of its network library decide, never an
/// address or path quoted in them or what a server sent back after its
/// status: a 404 for `birth-certificate.pdf` was shown as a certificate
/// that could not be checked, and a body saying "timed out" as a timeout.
pub fn reason(raw: &str) -> Option<&'static str> {
    let said = raw.split(" — ").next().unwrap_or(raw);
    let lower = said
        .split_whitespace()
        .filter(|word| !word.contains('/') && !word.contains('\\'))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    REASONS
        .iter()
        .find(|(signs, _)| signs.iter().any(|sign| lower.contains(sign)))
        .map(|(_, plain)| *plain)
}

/// A key shortcut as a menu shows it: the modifiers as egui names them
/// (symbols on a Mac), and the key by its symbol where it has one — "." and
/// "," rather than "Period" and "Comma", which no keyboard prints and no
/// translation turns into anything.
pub fn shortcut(ctx: &egui::Context, shortcut: &egui::KeyboardShortcut) -> String {
    let key = shortcut.logical_key;
    let modifiers = ctx.format_modifiers(shortcut.modifiers);
    // A Mac's symbols are every language's; the names are words, and a
    // German keyboard says Strg, Umschalt and Entf.
    let symbols = !modifiers.is_ascii();
    let mut said = if symbols {
        modifiers
    } else {
        let names = egui::ModifierNames {
            alt: key_word(t("key:Alt")),
            ctrl: key_word(t("key:Ctrl")),
            shift: key_word(t("key:Shift")),
            mac_cmd: key_word(t("key:Cmd")),
            mac_alt: key_word(t("key:Option")),
            ..egui::ModifierNames::NAMES
        };
        let mut said = names.format(&shortcut.modifiers, ctx.os().is_mac());
        if !said.is_empty() {
            said.push('+');
        }
        said
    };
    said.push_str(&key_name(key));
    said
}

/// A key's name on its keycap, in the language in force: the named keys a
/// shortcut uses; any other by its symbol (".", ",") or its letter. The
/// arrows by name, their symbols not being in every font.
pub fn key_name(key: egui::Key) -> String {
    use egui::Key;
    let named = match key {
        Key::Delete => t("key:Delete"),
        Key::Backspace => t("key:Backspace"),
        Key::Enter => t("key:Enter"),
        Key::Escape => t("key:Esc"),
        Key::Tab => t("key:Tab"),
        Key::Space => t("key:Space"),
        Key::Insert => t("key:Insert"),
        Key::Home => t("key:Home"),
        Key::End => t("key:End"),
        Key::PageUp => t("key:Page Up"),
        Key::PageDown => t("key:Page Down"),
        Key::ArrowUp => t("key:Up"),
        Key::ArrowDown => t("key:Down"),
        Key::ArrowLeft => t("key:Left"),
        Key::ArrowRight => t("key:Right"),
        _ => return key.symbol_or_name().to_string(),
    };
    key_word(named).to_string()
}

/// A key's word without the `key:` its catalog entry is told apart by:
/// "Delete" the key is "Entf" in German, "Delete" the command "Löschen".
fn key_word(word: &str) -> &str {
    word.strip_prefix("key:").unwrap_or(word)
}

/// What each piece of a download is shown holding: its part's length on
/// disk, held to its range, and evened out so the pieces add up to
/// `downloaded`, the count the window shows above them.
/// The two part company under a speed limit: a chunk is written as it
/// arrives and counted as the limit lets it through, so the parts on disk
/// ran seconds ahead of "Downloaded" and the pieces summed to more than it.
/// What the pieces hold beyond the count is taken from those under way, in
/// proportion to what each holds; a count ahead of the parts (read a moment
/// apart) is given to those under way, up to each one's range.
pub fn pieces_shown(pieces: &[(u64, u64)], downloaded: u64) -> Vec<u64> {
    let open: Vec<bool> = pieces.iter().map(|&(_, done)| done > 0).collect();
    pieces_shown_open(pieces, &open, downloaded)
}

/// [`pieces_shown`], knowing which pieces are under way (`open`): a count
/// ahead of the parts on disk is shared among those evenly, as their
/// connections fetch side by side. Given to the first piece in order, the
/// first connection showed "1.5 MB of 3.0 MB" while the other fifteen,
/// fetching too, said "waiting".
pub fn pieces_shown_open(pieces: &[(u64, u64)], open: &[bool], downloaded: u64) -> Vec<u64> {
    let mut shown: Vec<u64> = pieces.iter().map(|&(span, done)| done.min(span)).collect();
    let whole: u64 = pieces
        .iter()
        .map(|&(span, _)| span)
        .fold(0, u64::saturating_add);
    let target = downloaded.min(whole);
    let sum: u64 = shown.iter().fold(0, |a, &b| a.saturating_add(b));
    let under_way = |shown: &[u64], i: usize| {
        pieces
            .get(i)
            .zip(shown.get(i))
            .is_some_and(|(&(span, _), &s)| s < span)
    };
    if sum > target {
        let mut excess = sum - target;
        // In proportion among the pieces under way, then whatever is left
        // from the last pieces back.
        let busy: Vec<usize> = (0..shown.len()).filter(|&i| under_way(&shown, i)).collect();
        let held: u64 = busy.iter().filter_map(|&i| shown.get(i)).sum();
        if held > 0 {
            let total = excess;
            for &i in &busy {
                if let Some(s) = shown.get_mut(i) {
                    let cut = ((u128::from(total) * u128::from(*s)) / u128::from(held)) as u64;
                    let cut = cut.min(*s).min(excess);
                    *s -= cut;
                    excess -= cut;
                }
            }
        }
        let order: Vec<usize> = busy
            .iter()
            .rev()
            .copied()
            .chain((0..shown.len()).rev())
            .collect();
        for i in order {
            if excess == 0 {
                break;
            }
            if let Some(s) = shown.get_mut(i) {
                let cut = excess.min(*s);
                *s -= cut;
                excess -= cut;
            }
        }
    } else if sum < target {
        let mut short = target - sum;
        // Evenly among the pieces under way, a share at a time, each up to
        // its range.
        loop {
            let room: Vec<usize> = (0..shown.len())
                .filter(|&i| {
                    open.get(i).copied().unwrap_or(false)
                        && pieces
                            .get(i)
                            .zip(shown.get(i))
                            .is_some_and(|(&(span, _), &s)| s < span)
                })
                .collect();
            if short == 0 || room.is_empty() {
                break;
            }
            let share = (short / room.len() as u64).max(1);
            for i in room {
                let (Some(&(span, _)), Some(s)) = (pieces.get(i), shown.get_mut(i)) else {
                    continue;
                };
                let add = share.min(span - *s).min(short);
                *s += add;
                short -= add;
            }
        }
        for (i, &(span, _)) in pieces.iter().enumerate() {
            if short == 0 {
                break;
            }
            if let Some(s) = shown.get_mut(i) {
                if *s > 0 && *s < span {
                    let add = short.min(span - *s);
                    *s += add;
                    short -= add;
                }
            }
        }
        for (i, &(span, _)) in pieces.iter().enumerate() {
            if short == 0 {
                break;
            }
            if let Some(s) = shown.get_mut(i) {
                let add = short.min(span - *s);
                *s += add;
                short -= add;
            }
        }
    }
    shown
}

/// The name for the `n`th (from 1) of several files from a pattern: `{n}` is
/// the number and `{name}` the old name without its ending, and the ending
/// is kept. An empty pattern keeps the old name.
pub fn pattern_name(pattern: &str, n: usize, old: &str) -> String {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return old.to_string();
    }
    let ending_at = ["tar.gz", "tar.xz", "tar.bz2", "tar.zst"]
        .iter()
        .find(|e| old.to_ascii_lowercase().ends_with(&format!(".{e}")))
        .map(|e| old.len() - e.len() - 1)
        .or_else(|| {
            old.rfind('.')
                .filter(|&at| at > 0 && old.len() - at <= 8 && !old[at..].contains(' '))
        });
    let (stem, ending) = old.split_at(ending_at.unwrap_or(old.len()));
    let base = pattern
        .replace("{n}", &n.to_string())
        .replace("{name}", stem);
    format!("{base}{ending}")
}

#[cfg(test)]
mod tests;
