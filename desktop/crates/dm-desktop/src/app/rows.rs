// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What a row says: `dm_desktop_base::rows` and `columns`, at their old
//! paths.

pub use dm_desktop_base::columns::compare;
pub use dm_desktop_base::rows::*;

// The tests below read the window's items too.
#[cfg(test)]
use super::*;

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
            accept_page: false,
            validator: None,
            quality: None,
            parts_dir: String::new(),
            began_in: String::new(),
            cookies: String::new(),
            cookies_once: false,
            headers: Vec::new(),
            replaces: None,
            speed_limit: 0,
            stalled: String::new(),
            fresh_link: false,
            title_hint: None,
            torrent_files: Vec::new(),
            post: None,
            posted: false,
            sound: None,
            span: None,
            shape: None,
            mp3: false,
            gif: false,
            delete_after: None,
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
            when_done: String::new(),
            retries: 0,
            run_when_done: String::new(),
            hang_up: false,
            once_at: None,
            force: false,
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
        // Nothing arriving just now: what has arrived alone.
        row.progress.speed = 0.0;
        assert_eq!(status_in_english(&row), "3.0 MB");
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
