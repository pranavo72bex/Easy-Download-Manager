//! How a row's numbers are said. The same words the web window used, so a
//! person moving between them reads the same thing.

use std::time::Duration;

use crate::i18n::{t, tf};

/// A byte count in the unit a person reads it in.
pub fn bytes(n: u64) -> String {
    const K: f64 = 1024.0;
    let f = n as f64;
    // The units in the language in force: Russian's КБ and МБ, French's
    // Ko and Mo; the status bar said КБ/с beside a list saying KB/s. And
    // its decimal mark: "3,8 MB" in German and Russian, where "3.8" read
    // as a thousands mark (round 17).
    let mark = unit(crate::i18n::t("unit:."));
    let number = |value: f64, places: usize| format!("{value:.places$}").replace('.', mark);
    if n < 1024 {
        return format!("{n} {}", unit(crate::i18n::t("unit:B")));
    }
    // Each unit with its places; the unit chosen once the number is
    // rounded, so a size just under 1 MB reads "1.0 MB", not "1024.0 KB"
    // (UI round 5).
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

/// The common engine failures in plain words, by what their text holds
/// (lower case): the engine's own sentence where it has one, and a plain
/// one for what the network library says in its own terms ("error decoding
/// response body" is a connection cut mid-file). The first that matches
/// wins; anything else is shown as the engine said it.
pub const REASONS: [(&[&str], &str); 14] = [
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
        &["no space left", "os error 28", "disk full"],
        "the disk is full",
    ),
    (
        &["disk: permission denied", "os error 13"],
        "dm may not write to that folder",
    ),
    (
        &["the server answered 404"],
        "the server has no file at this address (404)",
    ),
    (
        &["error decoding torrent", "this file is not a torrent"],
        "the file is not a torrent",
    ),
    (
        &["could not read the torrent file"],
        "the .torrent file could not be read",
    ),
    // UI round 14: raw engine text shown for these.
    (
        &["the server answered 401"],
        "the server wants a sign-in (add one in Options › Sign-ins)",
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
    // Last: the specific failures above say more.
    (
        &["error sending request"],
        "the server could not be reached",
    ),
];

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
/// translation turns into anything (gap 158).
pub fn shortcut(ctx: &egui::Context, shortcut: &egui::KeyboardShortcut) -> String {
    let key = shortcut.logical_key;
    let modifiers = ctx.format_modifiers(shortcut.modifiers);
    // A Mac's symbols are every language's; the names are words, and a
    // German keyboard says Strg, Umschalt and Entf (gap 168).
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
/// `downloaded`, the count the window shows above them (gap 166).
///
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
/// fetching too, said "waiting" (UI round 8).
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

#[cfg(test)]
mod tests {
    #[test]
    fn a_file_uri_escapes_what_an_address_cannot_hold() {
        assert_eq!(
            super::file_uri("/home/a b/50%#ü.zip"),
            "file:///home/a%20b/50%25%23%C3%BC.zip"
        );
    }

    use super::*;

    /// Gap 166: no piece holds more than its range, and they add up to
    /// what "Downloaded" says, whichever of the two ran ahead.
    #[test]
    fn pieces_add_up_to_downloaded_and_stay_in_their_ranges() {
        // As round 11 saw: one 1.4 MB piece holding 1.3 MB on disk while
        // 864 KB had been counted through the limit.
        assert_eq!(pieces_shown(&[(1_400_000, 1_300_000)], 864_000), [864_000]);
        // Eight pieces each 2.2 MB on disk, 14.7 MB counted.
        let eight = [(7_200_000, 2_200_000); 8];
        let shown = pieces_shown(&eight, 14_700_000);
        assert_eq!(shown.iter().sum::<u64>(), 14_700_000);
        assert!(
            shown.iter().all(|&s| s <= 2_200_000 && s > 1_700_000),
            "{shown:?}"
        );
        // A finished piece stays finished while there are others to take from.
        assert_eq!(pieces_shown(&[(100, 100), (100, 60)], 150), [100, 50]);
        // The count ahead of the parts goes to the one under way.
        assert_eq!(
            pieces_shown(&[(100, 100), (100, 40), (100, 0)], 160),
            [100, 60, 0]
        );
        // Never more than the ranges hold.
        assert_eq!(pieces_shown(&[(100, 500), (50, 0)], 10_000), [100, 50]);
        for downloaded in [0, 1, 99, 120, 199, 250, 300, 1000] {
            let pieces = [(100, 100), (100, 77), (100, 3), (100, 0)];
            let shown = pieces_shown(&pieces, downloaded);
            assert_eq!(
                shown.iter().sum::<u64>(),
                downloaded.min(400),
                "{downloaded}"
            );
            assert!(shown.iter().zip(&pieces).all(|(s, (span, _))| s <= span));
        }
    }

    /// UI round 8: what is counted ahead of the parts goes evenly to the
    /// pieces under way, not all to the first.
    #[test]
    fn a_count_ahead_of_the_parts_is_shared_by_the_pieces_under_way() {
        let pieces = [(1000, 0); 4];
        let shown = pieces_shown_open(&pieces, &[true, true, true, false], 600);
        assert_eq!(shown, [200, 200, 200, 0]);
        // Each up to its range, the rest to the others.
        let shown = pieces_shown_open(&[(100, 0), (1000, 0)], &[true, true], 600);
        assert_eq!(shown, [100, 500]);
        // None under way: in order, as before.
        assert_eq!(
            pieces_shown_open(&[(100, 0), (100, 0)], &[false, false], 150),
            [100, 50]
        );
    }

    /// Gap 158: a key with a symbol is shown by it.
    #[test]
    fn a_shortcut_shows_its_keys_symbol() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
        out.textures_delta.clear();
        let ctrl = |key| egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, key);
        let shown = shortcut(&ctx, &ctrl(egui::Key::Period));
        assert!(shown.ends_with('.'), "{shown}");
        assert!(!shown.contains("Period"), "{shown}");
        assert!(shortcut(&ctx, &ctrl(egui::Key::Comma)).ends_with(','));
        assert!(shortcut(&ctx, &ctrl(egui::Key::R)).ends_with('R'));
        let plain = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Delete);
        assert_eq!(shortcut(&ctx, &plain), "Delete");
        assert_eq!(key_name(egui::Key::ArrowUp), "Up");
    }

    /// Gap 168: a German menu says Strg, Umschalt and Entf.
    #[test]
    fn a_shortcut_names_its_keys_in_the_language_in_force() {
        let ctx = egui::Context::default();
        ctx.set_os(egui::os::OperatingSystem::Nix);
        let both = egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::N,
        );
        let delete = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Delete);
        assert_eq!(shortcut(&ctx, &both), "Ctrl+Shift+N");
        crate::i18n::set("de");
        let german = (shortcut(&ctx, &both), shortcut(&ctx, &delete));
        crate::i18n::set("en");
        assert_eq!(german, ("Strg+Umschalt+N".to_string(), "Entf".to_string()));
    }

    /// Gap 153: the network library's words for a cut connection are said
    /// plainly; what dm does not know is left as it was.
    #[test]
    fn common_failures_have_plain_reasons() {
        let cut = "network: error decoding response body: error reading a body from connection";
        assert_eq!(
            reason(cut),
            Some("the server ended the connection before the file was complete")
        );
        assert_eq!(
            reason("network: connection reset by peer (os error 54)"),
            reason(cut)
        );
        assert_eq!(
            reason("the connection ended after 10 of 20 bytes"),
            reason(cut)
        );
        assert_eq!(
            reason("disk: No space left on device (os error 28)"),
            Some("the disk is full")
        );
        assert_eq!(reason("a sentence of the engine's own"), None);
        // What the address or the server's page says is not the reason.
        assert_eq!(
            reason("the server answered 404 Not Found for https://e.com/birth-certificate.pdf"),
            Some("the server has no file at this address (404)")
        );
        assert_eq!(
            reason("the torrent could not be added: error decoding torrent: invalid value"),
            Some("the file is not a torrent")
        );
        assert_eq!(
            reason("could not read the torrent file /home/a/x.torrent: No such file or directory (os error 2)"),
            Some("the .torrent file could not be read")
        );
        assert_eq!(
            reason("network: error sending request for url (http://h/x) — SOCKS error: failed to create underlying connection"),
            Some("the server could not be reached"),
            "only the words before the dash decide"
        );
        assert_eq!(
            reason("the server answered 401 Unauthorized for http://h/x"),
            Some("the server wants a sign-in (add one in Options › Sign-ins)")
        );
        assert_eq!(
            reason("the server answered 502 Bad Gateway for https://e.com/a — upstream timed out"),
            None
        );
        assert_eq!(
            reason("could not write C:\\Users\\me\\no space left\\a.zip"),
            None
        );
        assert_eq!(
            reason("error sending request for url (https://e.com/a): invalid peer certificate: UnknownIssuer"),
            Some("the server's security certificate could not be checked")
        );
    }

    #[test]
    fn sizes_read_the_way_the_web_window_read_them() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(1536), "1.5 KB");
        // Rounded up to the next unit, not "1024.0 KB" (UI round 5).
        assert_eq!(bytes(1024 * 1024 - 1), "1.0 MB");
        assert_eq!(bytes(1024 * 1024 - 60), "1023.9 KB");
        assert_eq!(bytes(1023 * 1024), "1023.0 KB");
        assert_eq!(bytes(1024 * 1024 * 1024 - 1), "1.00 GB");
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
        let shown = shown_url(concat!("https://me:", "secret", "@e.com/f.zip"));
        assert!(!shown.contains("secret"), "{shown}");
        assert!(shown.contains("me:"));
        // Dots a person can read, not their percent-encoding (gap 146).
        assert_eq!(shown, concat!("https://me:", "•••", "@e.com/f.zip"));
        assert_eq!(
            shown_url(concat!("https://a%40b:", "p%3Aw", "@e.com/f.zip")),
            concat!("https://a%40b:", "•••", "@e.com/f.zip")
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
