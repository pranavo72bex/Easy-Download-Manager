#[test]
fn a_file_uri_escapes_what_an_address_cannot_hold() {
    crate::region::pin(None);
    assert_eq!(
        super::file_uri("/home/a b/50%#ü.zip"),
        "file:///home/a%20b/50%25%23%C3%BC.zip"
    );
}

use super::*;

/// No piece holds more than its range, and they add up to
/// what "Downloaded" says, whichever of the two ran ahead.
#[test]
fn pieces_add_up_to_downloaded_and_stay_in_their_ranges() {
    crate::region::pin(None);
    // One 1.4 MB piece holding 1.3 MB on disk while 864 KB had been
    // counted through the limit.
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

/// What is counted ahead of the parts goes evenly to the
/// pieces under way, not all to the first.
#[test]
fn a_count_ahead_of_the_parts_is_shared_by_the_pieces_under_way() {
    crate::region::pin(None);
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

/// A key with a symbol is shown by it.
#[test]
fn a_shortcut_shows_its_keys_symbol() {
    crate::region::pin(None);
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

/// A German menu says Strg, Umschalt and Entf.
#[test]
fn a_shortcut_names_its_keys_in_the_language_in_force() {
    crate::region::pin(None);
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

/// The network library's words for a cut connection are said
/// plainly; what dm does not know is left as it was.
#[test]
fn common_failures_have_plain_reasons() {
    crate::region::pin(None);
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
        retry_cause("the server answered 500 Internal Server Error for http://a/x"),
        Some("Server error")
    );
    assert_eq!(
        retry_cause("the connection ended after 10 of 20 bytes"),
        Some("Connection dropped")
    );
    for raw in [
        "the server answered 503 Service Unavailable for http://a/x",
        "the server answered 429 Too Many Requests for http://a/x",
        "the connection ended after 10 of 20 bytes",
        "network: the server refused the connection",
        "network: operation timed out",
    ] {
        let cause = retry_cause(raw).unwrap_or_else(|| panic!("{raw}"));
        assert!(RETRY_CAUSES.contains(&cause), "{cause}");
    }
    assert_eq!(retry_cause("disk: No space left on device"), None);
    assert_eq!(
        reason("network: error following redirect for url (http://a/loop): too many redirects"),
        Some("the site sends the address round in a loop of redirects")
    );
    assert_eq!(
        reason("disk: No space left on device (os error 28)"),
        Some("the disk is full")
    );
    assert_eq!(reason("a sentence of the engine's own"), None);
    // What the address or the server's page says is not the reason.
    assert_eq!(
        reason("the server answered 404 Not Found for https://e.com/birth-certificate.pdf"),
        Some("this link no longer works (404)")
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
            Some("the server wants a sign-in (Properties › Login, or Options › Sign-ins for the whole site)")
        );
    assert_eq!(
        reason("the server answered 502 Bad Gateway for https://e.com/a — upstream timed out"),
        Some("the server had a problem on its side: try again later"),
        "the server's status decides, not the page's \"timed out\""
    );
    assert_eq!(
        reason(
            "the server answered 500 Internal Server Error for http://127.0.0.1:1/broken-link.zip"
        ),
        Some("the server had a problem on its side: try again later")
    );
    assert_eq!(
            reason("the server answered 429 Too Many Requests for https://www.google.com/sorry/index — In order to continue"),
            Some("the site is turning away too many requests just now: try again later")
        );
    assert_eq!(
            reason("yt-dlp could not download that: ERROR: [youtube] x: Sign in to confirm you're not a bot"),
            Some("the site wants to check you are not a bot: play the video in your browser, then use its Download video button")
        );
    assert_eq!(
        reason("yt-dlp could not download that: ERROR: [youtube] x: Video unavailable"),
        Some(
            "this video is not available: it may be private, removed or not shown in your country"
        )
    );
    assert_eq!(
        reason("yt-dlp could not download that: ERROR: Requested format is not available"),
        Some("dm could not get the video from this page")
    );
    assert_eq!(
            reason("dm's video helper could not be got ready (network: error sending request for url (https://github.com/x))"),
            Some("dm could not get ready for videos: check that you are online, then click Retry")
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
    crate::region::pin(None);
    assert_eq!(bytes(512), "512 B");
    assert_eq!(bytes(1536), "1.5 KB");
    // Rounded up to the next unit, not "1024.0 KB".
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
    crate::region::pin(None);
    assert_eq!(clock(None), "");
    assert_eq!(clock(Some(Duration::ZERO)), "");
    assert_eq!(clock(Some(Duration::from_secs(42))), "42s left");
    assert_eq!(clock(Some(Duration::from_secs(125))), "2m 5s left");
    assert_eq!(clock(Some(Duration::from_secs(7260))), "2h 1m left");
    // Over a day, in days: not "1494h 36m left".
    let long = Duration::from_secs(1494 * 3600 + 36 * 60);
    assert_eq!(clock(Some(long)), "62d 6h left");
    assert_eq!(span(Duration::from_secs(7380)), "2h 3m", "no \"left\"");
    // Under a second is said; nothing is nothing.
    assert_eq!(span(Duration::from_millis(400)), "<1s");
    assert_eq!(span(Duration::ZERO), "");
}

#[test]
fn time_left_is_said_in_the_language_in_force() {
    crate::region::pin(None);
    crate::i18n::set("de");
    let said = clock(Some(Duration::from_secs(90_000)));
    crate::i18n::set("en");
    assert_eq!(said, "noch 1 T. 1 h");
}

#[test]
fn a_date_of_another_year_says_the_year_and_the_month_is_translated() {
    crate::region::pin(None);
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
    crate::region::pin(None);
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
    crate::region::pin(None);
    let shown = shown_url(concat!("https://me:", "secret", "@e.com/f.zip"));
    assert!(!shown.contains("secret"), "{shown}");
    assert!(shown.contains("me:"));
    // Dots a person can read, not their percent-encoding.
    assert_eq!(shown, concat!("https://me:", "•••", "@e.com/f.zip"));
    assert_eq!(
        shown_url(concat!("https://a%40b:", "p%3Aw", "@e.com/f.zip")),
        concat!("https://a%40b:", "•••", "@e.com/f.zip")
    );
    assert_eq!(shown_url("https://e.com/f.zip"), "https://e.com/f.zip");
}

#[test]
fn a_row_is_named_before_the_server_names_it() {
    crate::region::pin(None);
    assert_eq!(
        provisional("https://www.e.com/a/My%20File.zip?x=1"),
        "My File.zip"
    );
    assert_eq!(provisional("https://e.com/"), "e.com");
    assert_eq!(
        provisional("https://www.youtube.com/watch?v=jNQXAC9IVRw"),
        "youtube.com/watch?v=jNQXAC9IVRw"
    );
    // A file is still its file's name, whatever follows it.
    assert_eq!(
        provisional("https://e.com/get/report.pdf?token=1"),
        "report.pdf"
    );
    assert_eq!(
        provisional("magnet:?xt=urn:btih:abc&dn=Big+Film"),
        "Big Film"
    );
    assert_eq!(provisional("magnet:?xt=urn:btih:abc"), "Magnet link");
    assert_eq!(host("https://www.e.com/a"), "e.com");
    assert_eq!(host("magnet:?xt=x"), "torrent");
}

#[test]
fn a_pattern_names_several_files_and_keeps_their_endings() {
    assert_eq!(pattern_name("Trip {n}", 1, "IMG_0001.jpg"), "Trip 1.jpg");
    assert_eq!(pattern_name("Trip {n}", 12, "clip.mp4"), "Trip 12.mp4");
    assert_eq!(
        pattern_name("{name} (copy)", 3, "report.pdf"),
        "report (copy).pdf"
    );
    assert_eq!(pattern_name("{n}-{name}", 2, "notes"), "2-notes");
    assert_eq!(
        pattern_name("Backup {n}", 1, "site.TAR.GZ"),
        "Backup 1.TAR.GZ"
    );
    assert_eq!(pattern_name("v{n}", 1, "a.b.c.txt"), "v1.txt");
    // Empty keeps the name; a dot that is not an ending is part of the name.
    assert_eq!(pattern_name("  ", 1, "keep.me"), "keep.me");
    assert_eq!(pattern_name("x{n}", 1, ".hidden"), "x1");
    assert_eq!(pattern_name("x{n}", 1, "a.very-long-ending-here"), "x1");
}
