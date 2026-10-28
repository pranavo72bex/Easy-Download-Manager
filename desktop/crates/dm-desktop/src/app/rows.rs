use super::*;

/// A state's own word, as the wire spells it.
pub fn kind(state: &State) -> &'static str {
    match state {
        State::Queued => "queued",
        State::Running => "running",
        State::Paused => "paused",
        State::Completed => "completed",
        State::Failed(_) => "failed",
        State::Cancelled => "cancelled",
    }
}

pub fn category_word(c: Category) -> String {
    serde_json::to_value(c)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Whether `row` failed as a link that has run out does: refused (403,
/// 404 or 410) by its server after part of it had arrived, where the same
/// address worked a moment before.
pub fn link_expired(row: &Entry) -> bool {
    let State::Failed(why) = &row.state else {
        return false;
    };
    row.progress.downloaded > 0
        && ["answered 403", "answered 404", "answered 410"]
            .iter()
            .any(|code| why.contains(code))
}

/// What the filter looks in: a row's name, address and note, lowercased.
pub(super) fn haystack(row: &Entry) -> String {
    haystack_named(&name_of(row), row)
}

/// [`haystack`] with the row's name already worked out.
pub(super) fn haystack_named(name: &str, row: &Entry) -> String {
    format!("{name} {} {}", row.url, row.description).to_lowercase()
}

/// The name a row is shown by: the file's, or a guess at it until the
/// server says.
pub fn name_of(row: &Entry) -> String {
    if !row.filename.is_empty() {
        return row.filename.clone();
    }
    // The name chosen in Download details, until the file is saved under
    // it: a row added for later showed its address's name.
    match row.save_name.as_deref().map(str::trim) {
        Some(chosen) if !chosen.is_empty() => chosen.to_string(),
        _ => format::provisional(&row.url),
    }
}

/// A row's kind: what its name says, else what dm learned from the server.
/// Queued rows had none, so Videos listed one of a hundred queued videos
/// and their icons were blank; and with what the server said
/// first, a file renamed to "report (1).pdf" stayed under Other.
/// One named "get.php" is still known by what the server sent.
pub fn kind_of(row: &Entry) -> Option<Category> {
    dm_core::category::by_name(&name_of(row)).or(row.category)
}

/// Why a row paused by itself, in plain words: a full disk names its
/// folder, as a failed one does.
pub fn stalled_said(row: &Entry) -> String {
    let words = crate::i18n::t;
    match format::reason(&row.stalled) {
        Some("the disk is full") if !going_into(row).is_empty() => {
            crate::i18n::fill(words("the disk is full in {}"), &[&going_into(row)])
        }
        Some(plain) => words(plain).to_string(),
        None => row.stalled.clone(),
    }
}

/// What the Status column says.
pub fn status_of(row: &Entry) -> String {
    status_in(row, t)
}

thread_local! {
    /// The rows a range is being retried for, as the last refresh found
    /// them: the attempt and how many there may be (0 for no end). Kept
    /// beside the rows, which the engine hands over without it.
    static RETRYING: std::cell::RefCell<HashMap<Id, (u32, u32, String)>> =
        std::cell::RefCell::new(HashMap::new());
    /// When each retrying row tries again.
    static NEXT_TRY: std::cell::RefCell<HashMap<Id, std::time::Instant>> =
        std::cell::RefCell::new(HashMap::new());
}

/// What the engine now says about retries, for the statuses drawn next.
pub fn set_retrying(now: HashMap<Id, (u32, u32, String, std::time::Instant)>) {
    NEXT_TRY.with(|n| {
        *n.borrow_mut() = now.iter().map(|(id, r)| (*id, r.3)).collect();
    });
    RETRYING.with(|r| {
        *r.borrow_mut() = now
            .into_iter()
            .map(|(id, (attempt, of, why, _))| (id, (attempt, of, why)))
            .collect();
    });
}

/// How long until a retrying row tries again: nothing once it is due.
pub(crate) fn next_try(id: Id) -> Option<std::time::Duration> {
    let at = NEXT_TRY.with(|n| n.borrow().get(&id).copied())?;
    at.checked_duration_since(std::time::Instant::now())
        .filter(|left| !left.is_zero())
}

thread_local! {
    /// Whether the download limit, not the schedule, is what holds rows
    /// that wait by themselves, as the last refresh found it.
    static LIMIT_HOLDS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// What holds waiting rows now, for the statuses drawn next.
pub fn set_limit_holds(now: bool) {
    LIMIT_HOLDS.with(|l| l.set(now));
}

pub(crate) fn retrying(id: Id) -> Option<(u32, u32, String)> {
    RETRYING.with(|r| r.borrow().get(&id).cloned())
}

/// What the engine said of a failure, word for word, where the status
/// says it in plainer words: shown on hover.
pub fn failure_as_said(row: &Entry) -> Option<&str> {
    match &row.state {
        State::Failed(raw) if format::reason(raw).is_some() => Some(raw),
        _ => None,
    }
}

/// A running row that has every byte of a file of known length: it is
/// joining its parts, not fetching.
pub fn fetched_all(row: &Entry) -> bool {
    row.progress
        .total
        .is_some_and(|total| total > 0 && row.progress.downloaded >= total)
}

/// Whether a row was added for later and waits for its queue: stopped,
/// in a queue, nothing fetched and not held by the schedule.
pub fn waits_for_its_queue(row: &Entry) -> bool {
    row.state == State::Paused
        && !row.paused_by_schedule
        && !row.queue.is_empty()
        && row.progress.downloaded == 0
}

/// A fraction as a whole percent, rounded down.
pub fn whole_percent(fraction: f64) -> u32 {
    (fraction * 100.0).clamp(0.0, 100.0).floor() as u32
}

/// The folder a row's file is in, once finished, or the one it was asked
/// to go to; empty where that is the download folder's own choice.
pub fn folder_of(row: &Entry) -> String {
    if !row.path.is_empty() {
        return std::path::Path::new(&row.path)
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
    }
    row.save_dir.clone().unwrap_or_default()
}

/// What the Status column says, in English whatever the window's language: what the control
/// protocol and MCP report, so a script or an agent reads the same words on
/// every machine.
pub fn status_in_english(row: &Entry) -> String {
    status_in(row, |english| english)
}

pub(super) fn status_in(row: &Entry, words: fn(&'static str) -> &'static str) -> String {
    match &row.state {
        // Said while a range waits to be tried again: the bar drops back
        // and climbs, which read as a fault.
        // With why, where dm can say it plainly: "Retrying (attempt 5 of
        // 10)" alone read as dm stuck on nothing.
        State::Running if retrying(row.id).is_some() => match retrying(row.id) {
            Some((attempt, of, why)) => {
                let cause = format::retry_cause(&why).map(words);
                match (of > 0, cause) {
                    (true, Some(cause)) => {
                        crate::i18n::fill(words("{} (retry {} of {})"), &[&cause, &attempt, &of])
                    }
                    (true, None) => {
                        crate::i18n::fill(words("Retrying (attempt {} of {})"), &[&attempt, &of])
                    }
                    (false, Some(cause)) => {
                        crate::i18n::fill(words("{} (retry {})"), &[&cause, &attempt])
                    }
                    (false, None) => crate::i18n::fill(words("Retrying (attempt {})"), &[&attempt]),
                }
            }
            None => String::new(),
        },
        State::Running => match row.progress.fraction() {
            // A torrent with nothing arriving is waiting for peers, said:
            // "0%" stood there with nothing to say why.
            Some(f)
                if dm_core::is_torrent(&row.url)
                    && row.progress.speed < 1.0
                    && !fetched_all(row) =>
            {
                format!("{}% · {}", whole_percent(f), words("waiting for peers"))
            }
            // Every byte in, its parts being put together: said, not "100%"
            // for as long as that takes, tens of seconds for a large file
            // on a slow disk.
            Some(_) if fetched_all(row) => words("Joining parts…").into(),
            // Whole percents everywhere, rounded down so 99.9 is not 100:
            // the list said 28.88% beside a bar saying 28%.
            Some(f) => format!("{}%", whole_percent(f)),
            // A length the server did not say: what has arrived, and how
            // fast, rather than "Starting…" for the whole run.
            None if row.progress.downloaded > 0 => {
                if row.progress.speed > 1.0 {
                    format!(
                        "{} · {}",
                        format::bytes(row.progress.downloaded),
                        format::rate(row.progress.speed)
                    )
                } else {
                    format::bytes(row.progress.downloaded)
                }
            }
            // A torrent with nothing yet is looking for peers, for as long
            // as that takes; "Starting…" read as stuck.
            None if dm_core::is_torrent(&row.url) => words("Finding peers…").into(),
            // The first video waits while dm fetches what it needs for
            // videos, half a minute or so: said, so it does not read as
            // stuck.
            None if dm_core::tools::preparing() => {
                words("Getting ready for videos (first time only)…").into()
            }
            None => words("Starting…").into(),
        },
        State::Queued => words("Queued").into(),
        // Held by the schedule or the download limit, not by a person: it
        // goes on by itself, which "Stopped" would not say.
        // Saying what for: "Waiting" alone read as stuck.
        State::Paused if row.paused_by_schedule => {
            if LIMIT_HOLDS.with(std::cell::Cell::get) {
                words("Waiting for the download limit").into()
            } else {
                words("Waiting for the schedule").into()
            }
        }
        // Added for later: it waits for its queue to be started, and does
        // not start by itself, which "Queued" did not say.
        // Stopped by itself for want of room or of its folder's drive.
        State::Paused if !row.stalled.is_empty() => {
            crate::i18n::fill(words("Paused: {}"), &[&stalled_said(row)])
        }
        State::Paused if waits_for_its_queue(row) => words("Waiting for Start queue").into(),
        // How far it got, so a stopped row is not mistaken for one that
        // never started.
        State::Paused => match row.progress.fraction() {
            Some(f) if f > 0.0 => format!("{} · {}%", words("Stopped"), whole_percent(f)),
            _ => words("Stopped").into(),
        },
        // Its file moved or deleted since: said so in Properties and to the
        // control protocol too, as the list says it (Properties
        // said "Complete").
        State::Completed if crate::table::gone(row) => words("File missing").into(),
        State::Completed => words("Complete").into(),
        // A server that wants a sign-in says where to give one: the plain
        // sentences for 401 and 403 do (`format::REASONS`); anything else
        // with that status is said as the engine said it, with where. The
        // address and the server's own page were shown in the row.
        State::Failed(reason) if needs_sign_in(reason) && format::reason(reason).is_none() => {
            format!(
                "{} — {}",
                crate::i18n::fill(words("Error: {}"), &[reason]),
                words("it needs a sign-in: Properties › Login")
            )
        }
        // In plain words where dm knows them; the engine's own on hover.
        // A full disk says which folder's: "the disk is full" left a person
        // to guess which disk.
        State::Failed(reason)
            if format::reason(reason) == Some("the disk is full")
                && !going_into(row).is_empty() =>
        {
            crate::i18n::fill(words("Error: the disk is full in {}"), &[&going_into(row)])
        }
        // A video page yt-dlp failed on, with nothing more specific known: the
        // same sentence for every cause told a person nothing to act on or to
        // report, so yt-dlp's own words follow it.
        State::Failed(reason)
            if format::reason(reason) == Some(format::VIDEO_NOT_GOT)
                && format::ytdlp_said(reason).is_some() =>
        {
            let said = format::ytdlp_said(reason).unwrap_or_default();
            crate::i18n::fill(
                words("Error: {}"),
                &[&format!("{} ({said})", words(format::VIDEO_NOT_GOT))],
            )
        }
        State::Failed(reason) => crate::i18n::fill(
            words("Error: {}"),
            &[&format::reason(reason).map_or(reason.as_str(), words)],
        ),
        State::Cancelled => words("Cancelled").into(),
    }
}

/// The folder a row's file was going into: the one chosen for it, else the
/// download folder it began in; empty where neither is known yet.
pub(super) fn going_into(row: &Entry) -> String {
    row.save_dir
        .clone()
        .filter(|d| !d.trim().is_empty())
        .unwrap_or_else(|| row.began_in.clone())
}

/// Whether a failure is a server refusing without a sign-in (401, 403).
pub fn needs_sign_in(reason: &str) -> bool {
    [" 401 ", " 403 "]
        .iter()
        .any(|code| format!(" {reason} ").contains(code))
}

/// How two rows compare under a column, for sorting.
pub(super) fn compare(column: Column, a: &Entry, b: &Entry) -> std::cmp::Ordering {
    match column {
        // "part 2" before "part 10", as a person reads them.
        Column::Name => format::natural(&name_of(a), &name_of(b)),
        Column::Queue => a.queue.cmp(&b.queue),
        // By the size the column shows, which for a file of unknown length
        // is what has arrived of it.
        Column::Size => crate::table::shown_size(a).cmp(&crate::table::shown_size(b)),
        // By state, then how far along: the same order in every language,
        // and 9% before 45% rather than after it as text would put it.
        Column::Status => {
            let rank = |e: &Entry| match e.state {
                State::Running => 0,
                State::Queued => 1,
                State::Paused => 2,
                State::Failed(_) => 3,
                State::Cancelled => 4,
                State::Completed => 5,
            };
            rank(a).cmp(&rank(b)).then_with(|| {
                a.progress
                    .fraction()
                    .partial_cmp(&b.progress.fraction())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        }
        // Only a running row shows a time left or a speed; a stopped one's
        // last figures are stale, and sorted as the blank they are shown as.
        Column::TimeLeft => {
            let left = |e: &Entry| {
                (e.state == State::Running)
                    .then_some(e.progress.eta)
                    .flatten()
                    .filter(|d| !d.is_zero())
            };
            left(a).cmp(&left(b))
        }
        Column::Rate => {
            let speed = |e: &Entry| {
                (e.state == State::Running && e.progress.speed > 1.0).then_some(e.progress.speed)
            };
            speed(a)
                .partial_cmp(&speed(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        }
        Column::LastTry => a.finished_at.cmp(&b.finished_at),
        Column::Description => a.description.cmp(&b.description),
        Column::Referrer => a.referrer.cmp(&b.referrer),
        Column::Folder => folder_of(a).cmp(&folder_of(b)),
    }
}

#[cfg(test)]
pub(super) mod status_tests {
    use super::*;

    pub(in crate::app) fn paused(by_schedule: bool) -> Entry {
        Entry {
            id: 1,
            url: "https://e.com/f.bin".into(),
            state: State::Paused,
            progress: dm_core::Progress {
                downloaded: 0,
                total: None,
                speed: 0.0,
                eta: None,
            },
            filename: "f.bin".into(),
            finished_at: None,
            category: None,
            checksum: None,
            verified: false,
            mirrors: 0,
            path: String::new(),
            referrer: String::new(),
            paused_by_schedule: by_schedule,
            description: String::new(),
            queue: String::new(),
            save_dir: None,
            save_name: None,
            validator: None,
            quality: None,
            parts_dir: String::new(),
            began_in: String::new(),
            cookies: String::new(),
            cookies_once: false,
            replaces: None,
            speed_limit: 0,
            stalled: String::new(),
            sound: None,
        }
    }

    #[test]
    fn a_closed_schedule_says_when_it_next_opens() {
        let schedule = dm_core::Schedule {
            enabled: true,
            from: "01:00".into(),
            to: "07:00".into(),
            days: vec![1],
            wake: false,
        };
        // Monday 00:30: later today. Monday 08:00: next Monday.
        assert_eq!(opens_after(&schedule, 1, 30), Some((0, 60)));
        assert_eq!(opens_after(&schedule, 1, 480), Some((7, 60)));
        let never = dm_core::Schedule {
            days: vec![],
            from: "bad".into(),
            ..schedule
        };
        // A time that is not one is no schedule: open at once.
        assert_eq!(opens_after(&never, 1, 480), Some((0, 481)));
        assert!(needs_sign_in("the server answered 401 Unauthorized for x"));
        assert!(!needs_sign_in(
            "the server answered 404 Not Found for /4013"
        ));
    }

    #[test]
    fn a_row_held_by_the_schedule_or_the_limit_is_waiting_not_stopped() {
        // Saying what it waits for.
        set_limit_holds(false);
        assert_eq!(status_in_english(&paused(true)), "Waiting for the schedule");
        set_limit_holds(true);
        assert_eq!(
            status_in_english(&paused(true)),
            "Waiting for the download limit"
        );
        set_limit_holds(false);
        assert_eq!(status_in_english(&paused(false)), "Stopped");
        let mut part_way = paused(false);
        part_way.progress.downloaded = 45;
        part_way.progress.total = Some(100);
        assert_eq!(status_in_english(&part_way), "Stopped · 45%");
    }

    #[test]
    fn a_download_of_unknown_length_says_what_has_arrived_and_how_fast() {
        let mut row = paused(false);
        row.state = State::Running;
        assert_eq!(status_in_english(&row), "Starting…");
        row.progress.downloaded = 3 * 1024 * 1024;
        row.progress.speed = 2048.0;
        assert_eq!(status_in_english(&row), "3.0 MB · 2.0 KB/s");
        // A torrent with nothing yet is finding its peers.
        let mut magnet = paused(false);
        magnet.state = State::Running;
        magnet.url = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567".into();
        assert_eq!(status_in_english(&magnet), "Finding peers…");
        // Known, but nothing arriving: waiting for peers.
        magnet.progress.total = Some(100);
        magnet.progress.downloaded = 10;
        assert_eq!(status_in_english(&magnet), "10% · waiting for peers");
        magnet.progress.speed = 5000.0;
        assert_eq!(status_in_english(&magnet), "10%");
        // Every byte in, its parts being joined: said so, not "100%".
        let mut joining = paused(false);
        joining.state = State::Running;
        joining.progress.total = Some(100);
        joining.progress.downloaded = 100;
        assert_eq!(status_in_english(&joining), "Joining parts…");
    }

    #[test]
    fn columns_sort_by_what_they_show() {
        use std::cmp::Ordering::*;
        let named = |name: &str| {
            let mut row = paused(false);
            row.filename = name.into();
            row
        };
        // Names: numbers by value.
        assert_eq!(
            compare(Column::Name, &named("part 2"), &named("part 10")),
            Less
        );
        // Size: what has arrived of a file of unknown length is its size.
        let (mut big, mut small) = (named("a"), named("b"));
        big.progress.downloaded = 5000;
        small.progress.total = Some(10);
        assert_eq!(compare(Column::Size, &big, &small), Greater);
        // Speed and time left: a stopped row's last figures are not shown,
        // and do not sort it ahead of one that is running.
        let (mut stopped, mut running) = (named("a"), named("b"));
        stopped.progress.speed = 9e9;
        stopped.progress.eta = Some(std::time::Duration::from_secs(1));
        running.state = State::Running;
        running.progress.speed = 100.0;
        running.progress.eta = Some(std::time::Duration::from_secs(500));
        assert_eq!(compare(Column::Rate, &stopped, &running), Less);
        assert_eq!(compare(Column::TimeLeft, &stopped, &running), Less);
    }
}
