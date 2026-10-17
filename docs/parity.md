# Behaviour checklist

What dm does, and how each claim is known. Behaviour follows public
protocols (HTTP ranges, FTP, Metalink, HLS, BitTorrent, native messaging).

**Wired is not working.** A button that sends a command proves the command
was sent. A row here is `PASS` only when a test or a recorded run checks the
outcome: bytes on disk compared with what was served, a state read back
after a restart, a server that was or was not contacted.

| Status | Meaning |
|---|---|
| `PASS` | Checked by a repeatable test or a recorded run, named in the row |
| `PARTIAL` | Works for the common case; the gap is named |
| `NOT VERIFIED` | Implemented, but nothing has checked the outcome |
| `BLOCKED` | Needs something code cannot supply |
| `N/A` | Out of scope, and why |

Last checked 2026-10-12: the desktop workspace's tests, the extension's
(`browser/`, Rust, 18 on the host), `scripts/app-check.py` against the built
binary, and `scripts/chrome-check.py` (14 checks in Chrome for Testing 155),
all passing. There is no JavaScript test suite any more: the extension is
Rust compiled to WebAssembly. Test paths are under
`desktop/crates/dm-core/tests/` unless they say otherwise.

## A. Adding downloads

| Behaviour | Status | Evidence |
|---|---|---|
| Paste a URL into the window | PASS | `dm-desktop/tests/window.rs` (byte-exact through the window); `app-check.py` |
| Several URLs at once | PASS | `queue.rs` `add_all`; window driver |
| Empty, malformed or unsupported URL refused | PASS | `queue::accepted`; host replies FAILED with the reason; a paste mixing links and other text queues the links and hands the rest back with a message (`window.rs`) |
| Redirect chains followed, loops bounded | PASS | `Policy::limited(10)`; audit run: loop fails in 5 ms |
| A name from the address with no extension takes its Content-Type's | PASS | shared naming table; zoo `type-only` (a PDF at `/get` was saved as "get") |
| Content-Disposition, RFC 5987 and URL names | PASS | `shared/naming-cases.json`, read by the engine's suite |
| Query string and fragment kept out of the name | PASS | same table |
| No name at all falls back safely | PASS | same table (`download`, MIME extension) |
| An existing file is never overwritten | PASS | `download.rs` `a_second_download_does_not_overwrite_the_first`; `claim_path` takes the name with `create_new` (`two_files_finishing_at_once_never_share_a_name`), and a torrent's folder with `create_dir` (`a_folder_already_there_is_not_moved_onto`) |
| Unsupported schemes fail clearly | PASS | queue refuses; host replies FAILED with the reason |
| Browser catches downloads, not other links | PASS | the app's catch rules (`dm-protocol` tests); `scripts/chrome-check.py`: a listed type caught, an unlisted one and one the app's settings leave out finished in the browser |
| Clipboard watching, opt-in | PASS | `window.rs`: nothing offered when off or for what was copied before turning it on; text and queued links ignored; a new link offered and queued; a dismissed one not offered again |
| Drag and drop | PARTIAL | dropped `.webloc`/`.url`/text files are read for links; winit delivers no bare dragged links; a real drop not watched |

## B. Correctness and integrity

| Behaviour | Status | Evidence |
|---|---|---|
| Text and binary files byte-exact | PASS | `download.rs` `a_segmented_download_is_byte_correct` |
| Zero-byte file, server with ranges (416 `*/0`) | PASS | `integrity.rs` `an_empty_file_on_a_server_with_ranges_downloads` |
| Zero-byte file, server without | PASS | `integrity.rs` `an_empty_file_without_ranges_does_not_wait_out_retries` |
| Large files without 2x disk | PASS | `a_download_never_needs_twice_its_own_size_on_disk`; `memory.rs` |
| Unknown length (chunked) | PASS | `integrity.rs` `a_file_of_unknown_length_downloads` |
| 4xx/5xx said with the status | PASS | `retry.rs` unit tests; `an_answer_is_not_retried_forever` |
| Connect failure, timeout, mid-body drop | PASS | `integrity.rs` `a_connection_dropped_mid_range_is_retried`; stalled-server pause test |
| Never Completed when truncated | PASS | `a_file_shorter_than_promised_is_not_saved` |
| Partial files never look finished | PASS | parts staged under dm's own directory; moved into place only when whole |
| A 206 for another range than asked is refused | PASS | [zoo](zoo.md) `ranges-other-range` — it was written at the asked offset until the zoo found it |
| A 206 with the right label and bytes from elsewhere is refused | PASS | [zoo](zoo.md) `ranges-lying-body`: each range after the first fetches one byte of overlap with the one before, compared at assembly |
| Thirty-one server behaviours from the wild, held to a floor that only rises | PASS | [docs/zoo.md](zoo.md); curl fetches the same addresses and agrees on every file |
| Cancel stops traffic and leaves nothing | PASS | `a_cancelled_download_leaves_nothing_saved`; queue cancel tests |

## C. Pause, resume, ranges

| Behaviour | Status | Evidence |
|---|---|---|
| Pause stops promptly, even on a stalled connection | PASS | `a_pause_on_a_stalled_connection_is_honoured_within_the_read_timeout` |
| Resume continues from staged bytes | PASS | `a_download_resumes_from_what_is_already_staged` |
| Resume after an app restart | PASS | `queue.rs` `the_queue_survives_the_app_closing` and the running-row case |
| Server ignores Range mid-run (200 for 206) | PASS | `Error::Changed`, nothing appended |
| Server without ranges, retried | PASS | `integrity.rs` `a_server_without_ranges_is_fetched_again_from_the_start` |
| File republished between runs (ETag/Last-Modified) | PASS | `integrity.rs` `a_file_republished_between_runs_is_not_spliced_into_the_old_parts` |
| Connection setting changed before a resume | PASS | `a_changed_connection_setting_does_not_corrupt_a_resumed_download` |
| Parts with no plan beside them | PASS | `parts_staged_under_no_known_plan_are_not_trusted` |
| A power cut that lost more than the end checked at resume | PASS | `download.rs` `a_power_cut_that_lost_more_than_the_checked_end_is_repaired_whole` (256 KiB of zeros: byte-exact, not fetched from the start); a repair steps back at most 4 MiB at a time without spending a retry, so a file's own zeros before a torn end are not all fetched again: `a_torn_end_after_a_files_own_zeros_is_not_followed_back_through_them` |
| Pause or cancel of a magnet whose metadata never comes | PASS | `torrent.rs` `a_magnet_with_no_peers_stops_when_paused` |
| Pause of yt-dlp ends what it started (its ffmpeg) | PASS (unix) | `ytdlp_tree.rs` `a_pause_ends_what_yt_dlp_started_not_only_yt_dlp`, `a_pause_ends_the_tree_under_a_path_with_no_kill_on_it` (`killpg`, no `kill` program); Windows uses `taskkill /T`, started and not waited for, not run |

## D. Multiple connections

| Behaviour | Status | Evidence |
|---|---|---|
| Ranges cover the file exactly, no overlap | PASS | `plan.rs` unit tests; byte-exact downloads |
| A range longer than asked is refused, not trimmed | PASS | `a_range_longer_than_asked_for_is_refused_rather_than_trimmed` |
| One failed range recovers alone | PASS | `a_connection_dropped_mid_range_is_retried` |
| One connection when ranges are not served | PASS | `a_server_without_ranges_still_produces_the_right_file` |
| Small files not split | PASS | `MIN_SEGMENT`, `plan.rs` tests |
| Mirrors (Metalink) each carry ranges | PASS | Metalink tests in `download.rs` and `metalink.rs` |
| Metalink: the document's size is the reference, a stale mirror is dropped; a piece's hash is not the file's | PASS | `tests/metalink.rs`; `metalink.rs` `a_digest_of_one_piece_is_not_taken_for_the_whole_file` |
| A site's own connection limit (Options › Network), by host or suffix, never above the setting | PASS | `limits.rs` `a_site_with_a_limit_of_its_own_is_fetched_on_that_many_connections` (a counting server sees at most the limit); `window.rs` `site_limits_attempts_and_the_timeout_are_set_in_options_and_reach_the_queue` |

## E. Retry

| Behaviour | Status | Evidence |
|---|---|---|
| Bounded, with jittered backoff; the bound a setting (Options › Network), zero keeps trying | PASS | `retry.rs`; `limits.rs` `attempts_are_as_many_as_asked_and_zero_keeps_trying` |
| The read timeout a setting, held to | PASS | `limits.rs` `a_connection_that_sends_nothing_is_given_up_on_within_the_timeout_set` |
| Retry-After (seconds), capped at 60 s | PASS | `retry.rs` tests |
| Retry-After as an HTTP date | PASS | measured against the response's own `Date`, so a wrong local clock cannot skew it; ignored without one (`retry_after_reads_only_what_it_can_trust`) |
| 404/403/401 not retried | PASS | `retry.rs` tests |
| Manual Retry of a failed row | PASS | queue test fails, retries and completes a row; window driver |
| A pause or cancel cuts the backoff short | PASS | `stopping_is_not_something_to_retry` |

## F. Queue and schedule

| Behaviour | Status | Evidence |
|---|---|---|
| Order and concurrency limit | PASS | `queue.rs` limit tests; a paused row still letting go holds its slot (`a_paused_row_still_stopping_holds_its_slot`) |
| A failure does not block the queue | PASS | `queue.rs` |
| Queue survives restart; a corrupt file is kept aside | PASS | `queue.rs` |
| A hand-off arriving before the saved queue loads | PASS | `a_link_added_before_the_saved_queue_loads_is_kept`; the app now loads the queue before anything can add |
| Saves at the same time | PASS | `saves_at_the_same_time_do_not_report_a_failure_that_did_not_happen`; `lib.rs` `writes_to_one_file_at_once_all_succeed` |
| Schedule window; a person's pause survives it | PASS | `queue.rs` schedule tests; `app-check.py` |
| Schedule's pauses survive a restart | PASS | `what_the_schedule_paused_is_started_after_a_restart` |
| Schedule with no day ticked runs every day; a time that is not HH:MM is refused in the Scheduler with the reason | PASS | `no_day_ticked_means_every_day`, `the_scheduler_refuses_a_time_that_is_not_one_and_says_why` |
| A settings change is in force whole or not at all; a missing cookies file is a problem shown, not a refused change; the cookies file is read at launch | PASS | `dm-shell/tests/apply.rs`, `an_option_that_cannot_be_applied_is_said_and_what_was_typed_is_kept` |
| One odd entry in the settings file costs that entry only; an unknown proxy type is turned off | PASS | `one_odd_entry_costs_that_entry_and_not_every_setting` |
| Removing rows never deletes finished files | PASS | audit reading; `clear_finished` test |
| The Main queue starts as dm opens, when asked to (Options › Downloads) | PASS | `dm-shell/tests/launch.rs` `the_main_queue_starts_at_launch_when_the_setting_says_so`: a row added for later is no longer waiting after `shell::start` |
| A named queue runs fewer files at once than the whole (Options › Downloads); the rest of the room goes to other queues | PASS | `queue.rs` `a_queue_with_a_limit_of_its_own_runs_that_many_while_the_rest_of_the_room_goes_on`; `window.rs` `a_queues_own_file_limit_and_the_parts_folder_are_set_in_options` |
| Recheck queue on its own timer (Options › Downloads), as well as when asked | PASS | `queue.rs` `the_recheck_queue_asks_again_on_its_own_timer_and_only_then`; `shell::watch_rules` drives it; `window.rs` `the_proxys_bypass_list_and_the_recheck_timer_are_set_in_options` |
| Recheck compares the file, not the document: a Metalink row by the file's length (the document's or a mirror's), a yt-dlp or stream row not at all (not counted as checked); an `HTTP://` row is asked about too | PASS | `queue.rs` `a_recheck_of_a_metalink_asks_about_the_file_not_the_document`, `a_recheck_leaves_alone_a_row_made_from_a_web_page`, `a_recheck_asks_about_a_row_whose_scheme_is_written_in_capitals` |
| Recheck keeps the old file until the new version is whole; a failed fetch says so and keeps it, Retry replaces it in place | PASS | `queue.rs` `a_recheck_that_cannot_fetch_the_new_version_keeps_the_old_file` |
| Pause all and Stop queue while the schedule holds rows make them a person's pauses, not restarted when the window opens | PASS | `queue.rs` `pausing_everything_while_the_schedule_holds_it_is_not_undone_by_the_window_opening` |
| A row deleted while running holds its slot until its run lets go | PASS | `queue.rs` `a_deleted_row_holds_its_slot_until_its_run_lets_go` (measured at the server) |
| Priorities | N/A | dm orders work by queues and their order, not by priorities |

## G. Speed and progress

| Behaviour | Status | Evidence |
|---|---|---|
| Percentage within 0–100; speed and ETA finite | PASS | `progress.rs` tests; UI clamps |
| Speed limit held at the transfer | PASS | `app-check.py`: 256 KB/s holds 1 MiB to seconds |
| One ceiling across every download | PASS | torrent and yt-dlp runs each hold a share while they run; the ceiling is split n+1 ways (`limit.rs` tests). A yt-dlp run keeps the part it started with |
| UI with hundreds of rows | PASS | `window.rs`: 500 rows, each frame within budget (layout, not paint) |

## H. Filesystem and naming

| Behaviour | Status | Evidence |
|---|---|---|
| Traversal, separators, control characters | PASS | `shared/naming-cases.json`; `a_name_cannot_become_a_path` |
| Windows reserved names, including `CON.tar.gz` | PASS | shared table |
| Long names cut to 240 bytes, extension kept | PASS | shared table (CJK case) |
| Download folder and categories respected | PASS | `app-check.py` |
| Disk full during assembly | PASS | truncated file removed, row failed; Retry test |
| Unwritable download folder | PASS | checked before anything is fetched, by every route, and the folder is named (`a_download_folder_that_cannot_be_written_is_said_before_fetching`) |

## I. Persistence

| Behaviour | Status | Evidence |
|---|---|---|
| The folder for unfinished parts is a setting (Options › Folders); a row keeps the folder it began in | PASS | `queue.rs` `a_download_keeps_the_parts_folder_it_began_in_when_the_setting_changes` (paused, the folder moved, resumed from the old parts, a new row under the new folder); `window.rs` |
| Running rows come back queued and resume | PASS | `queue.rs`; without a rate or ETA from before the restart (`a_restored_row_does_not_claim_a_rate_from_before_the_restart`) |
| A finished row whose file was deleted | PASS | kept as history; its status says "File missing", Open and Open with are off, Open folder says the file is gone, Redownload fetches it again. `window.rs` deletes a finished file and checks each |
| Corrupt settings file | PASS | kept as `settings.json.unreadable` and said in the window (`a_settings_file_that_cannot_be_read_is_kept_and_said`) |

## J. Browser and native messaging

The extension is Rust (`browser/`) compiled to WebAssembly; its only
hand-written JavaScript is a 20-line loader. It catches and hands over, and
downloads nothing itself. The protocol is one crate, `dm-protocol`, used by
the host and the extension.

| Behaviour | Status | Evidence |
|---|---|---|
| Framing, 64 MiB cap | PASS | `host.rs` tests |
| A malformed frame does not end the host | PASS | `a_frame_that_is_not_json_does_not_end_the_conversation` |
| Request ids cannot escape the inbox | PASS | `a_browser_does_not_get_to_choose_where_a_request_is_written` |
| Stdout carries frames only | PASS | no print macros in the host; the app it starts has stdout to null; `app-check.py` |
| One vocabulary for both sides | PASS | `dm-protocol` tests: wire names, versions, an older extension's extra keys |
| Catch rules are the app's (`settings.get`) | PASS | `host.rs` `the_extension_is_sent_the_catch_rules_in_the_apps_settings`; `chrome-check.py`: a .zip left in the browser when the app's settings list only .bin |
| A clicked download caught, handed over with its referrer, erased from the browser | PASS | `chrome-check.py` (Chrome for Testing 155, macOS arm64): file byte-exact in dm's folder, dm's requests carry the page as `Referer`, the browser's list and folder empty |
| Host absent: the browser keeps the download | PASS | `chrome-check.py`: manifest removed, the download finishes in the browser |
| A link only the browser can fetch is not taken | PASS | the host probes before filing (`a_link_dm_cannot_fetch_itself_is_left_with_the_browser`); `chrome-check.py`: a cookie-gated file refused by dm, resumed and finished by the browser |
| A site that answers dm with a sign-in page (a redirect to a 200 of HTML) in the file's place: the browser keeps its download | PASS | the extension sends the browser's own type (`mime`) with a caught download, and the host refuses when its probe ends at HTML while the browser's type was not HTML, or, untyped, the address names a file (`dm-protocol` `a_page_served_to_dm_for_a_file_served_to_the_browser_is_told_apart`; `host.rs` `a_sign_in_page_served_in_place_of_the_file_leaves_it_with_the_browser`). A refusal resumes the browser's download, as for any refusal. Not yet driven in a browser |
| A slow host does not have a file downloaded twice | PASS | the extension sends when it stops waiting (`deadline`); the host's probe gets at most 10 s and never past the deadline less a 2 s margin, and a hand-off that would be answered late is refused, not filed (`dm-protocol` `a_hand_off_is_given_only_the_time_the_extension_still_waits`; `host.rs` `a_hand_off_the_extension_has_stopped_waiting_for_is_not_filed`; `decide.rs` `the_host_answers_well_before_the_extension_stops_waiting`) |
| Nothing from a private window reaches dm | PASS | its downloads are left with the browser (`finished_private_and_non_web_downloads_stay_with_the_browser`); so are its "Download … with dm" menus and the video panel, and no cookies are read for it (`decide.rs` `nothing_from_a_private_window_is_handed_over_from_a_menu_or_the_panel`). Not yet driven in a browser |
| Never re-catch what the extension restarted | PASS | `decide.rs` `what_dm_restarted_itself_is_never_caught_again`, `a_url_given_back_is_let_through_once_and_not_for_ever`; the restart path itself is NOT VERIFIED in a browser (Chrome resumed every paused download in the runs) |
| Context menu: link, media, all and selected links | PARTIAL | `decide.rs` tests for each target; `chrome-check.py` calls the link handler from the worker and dm fetches the file; no real menu click is driven |
| "Take every download from this site" (toolbar popup): a type or size the app's rules leave is taken from that site all the same — a download asked for on the site, served from it or started from its pages, so `github.com` covers its redirect to `objects.githubusercontent.com`; the app's master switch and its sites to skip still hold, and it still refuses what only the browser can fetch | PASS | `decide.rs` `a_site_whose_every_download_is_taken_covers_its_parts_and_nothing_else`, `a_site_whose_every_download_is_taken_covers_its_redirects_and_its_pages`, `a_forced_site_still_answers_to_the_apps_switch_and_skip_list`; `dm-protocol` `taking_every_download_from_a_site_overrides_type_and_size_only`, `a_site_on_the_skip_list_keeps_downloads_served_from_elsewhere_and_its_pages`; `chrome-check.py` (a `.txt` the rules leave lands in dm's folder with the site listed, and the popup has the switch) |
| Two cancelled hand-offs from one site offer to leave it to the browser | PASS | `window.rs` `two_cancelled_hand_offs_from_a_site_offer_to_leave_it_to_the_browser`: a typed-in cancel does not count; "Keep handing them over" leaves the rules; "Leave them to the browser" puts the site on Options › Browser's list the extension reads |
| Toolbar popup | PASS | Clicking dm's icon opens a popup: whether the app answers (or the fix, naming `dm-desktop --register`), "Catch downloads" (off: the browser keeps every download), the video panel on this site, and "Open dm", which brings dm's window up and downloads nothing. `chrome-check.py` opens it and drives each (2026-10-14) |
| Registering with the browsers (`--register`) | PASS | `register.rs` tests against a temporary home: both manifest families, unregistering, ids, a protected folder refused; the CLI run against a temporary home |
| The unpacked Chrome id is the one registered | PASS | `dm-protocol` `the_manifest_key_yields_the_id_the_host_is_registered_for`; `chrome-check.py` reads the id Chrome assigned |
| Used by hand in Chromium, with dm's real window: real pointer and keys only | PASS | `scripts/linux-chromium-human.sh` (Debian 12 Chromium 155, arm64, Xvfb + openbox, xdotool; `scripts/chromium-human.py`): dm registered with `--register` and opened with its window; a clicked .zip lands byte-exact in dm with the browser's list empty and the row Complete; a real right-click and the browser's own "Download link with dm" item; the pointer resting on a playing video raises "Download video", its click and dm's Start now land the video; an Alt-clicked link finishes in the browser; dm's row clicked, Stop, Resume (the file byte-exact) and Delete; the Extensions button, dm's popup and its "Catch downloads" switched off, after which a clicked link stays in the browser. DevTools and the control protocol only read and take screenshots. 26/26 (2026-10-17) |
| The same by hand on other Linux distributions, in each one's own image | PARTIAL | dm built once in the Debian image and copied into the distribution's own (`scripts/linux/Dockerfile.human-*`, only its browser, dm's libraries, Xvfb, openbox, xdotool), arm64, 2026-10-17, each run twice: `DM_DISTRO=fedora sh scripts/linux-chromium-human.sh` (Fedora 41's Chromium 142) 26/26 both runs; `DM_DISTRO=ubuntu2204 sh scripts/linux-firefox-check.sh` (Mozilla's Firefox 158; Ubuntu's own is a snap) and `DM_DISTRO=fedora` (Fedora's Firefox 145) 12/12 both runs, the real right-click and the panel click included. `DM_DISTRO=arch` (Arch Linux ARM, Chromium 153) 26/26 since egui 0.37 (it was 20/26: at-spi2-core 2.62 dropped `org.a11y.Status.ScreenReaderEnabled`, which accesskit_unix 0.21 waited on, so dm's window never reached the accessibility bus and no screen reader could read it; accesskit_unix 0.24 listens to `IsEnabled`; decisions.md 2026-10-17). Not on Ubuntu with Chromium: a snap does not run in a container |
| Alt-click a link: the browser keeps it | PASS | `decide.rs` `an_alt_click_leaves_a_web_address_with_the_browser_and_nothing_else`; `chrome-check.py` Alt-clicks a link of a type the app catches and the file finishes in the browser, not in dm |
| The panel lists a player page's qualities with sizes | PASS | `ytdlp.rs` `a_pages_formats_become_one_choice_per_height_and_the_sound_alone`; `host.rs` formats tests; `chrome-check.py` (32/32) lists two lines from a stand-in yt-dlp and the picked one reaches dm as `720/mp4`; `youtube-check.py` (27/27) lists the real video's three qualities and the picked MP4 arrives as .mp4 |
| A panel video opens "Download details" in the app | PASS | `window.rs` (the panel video test); on by default, Options › General turns it off |
| "Download video" panel over players | PASS | `decide.rs` `the_panel_is_for_real_players_only`, `the_panel_sits_in_the_players_corner_and_stays_on_screen`, `a_panel_click_hands_the_file_or_the_page`; `chrome-check.py` hovers real players. A file `<video>` beneath a transparent overlay gets the panel, its click lands the file in dm's folder, and the panel hides while playing. A MediaSource player hands dm the page, not the `blob:` address. A 96px clip gets no panel. On 2026-10-17, `CHROME_PATH=/usr/bin/chromium DM_BINARY=~/.local/bin/dm python3 scripts/live-panel-check.py` visited W3Schools: the panel appeared and handed `mov_bbb.mp4` to dm; it completed and played (10 s, 176p), with no dm page errors. The combined run was 31/33 because two later popup checks failed; the standalone `chrome-check.py` passed 53/53. |
| The panel on YouTube | PASS (by hand) | `python3 scripts/youtube-check.py`, 2026-10-12. On "Me at the zoo" the panel shows over the playing player in its corner and stays while the pointer rests for five seconds. Its click says "Sent to dm", and the `.webm` lands in dm's folder through yt-dlp. Not in CI: it leaves loopback |
| Panel on a site whose policy forbids WebAssembly | PARTIAL | Chrome applies the page's CSP to the page script's WebAssembly, so the panel cannot load there; `chrome-check.py` checks it fails quietly and that the right-click menu still hands that video |
| Chrome end to end | PASS | `python3 scripts/chrome-check.py` (30 checks) |
| Firefox end to end | PASS | `scripts/firefox-check.py native`, Firefox 157 fetched into `~/.cache/dm-firefox` (not installed), 2026-10-12: installs with no warnings; a caught download and "Download link with dm" land in dm's folder; the panel shows over a video and its click hands the file. The run found the panel's click broken in Firefox, now fixed |
| Windows registration | PARTIAL | `register.rs` writes both manifests under `%LOCALAPPDATA%\dm` and a key per browser (Chrome, Chromium, Edge, Brave, Vivaldi, Firefox) through `reg.exe`; the arguments are unit-tested on every OS and the code compiles for Windows (`scripts/windows-check.sh`). Never run on Windows |
| Cookies | PASS | the browser's stay with it unless "Hand over this site's sign-in" is on for the site (toolbar popup; the `cookies` permission is optional and asked for on that switch): then its cookies go with the hand-off, the host probes with them and the row keeps them (`dm-protocol` round trip; `host.rs` `a_sites_cookies_handed_over_reach_the_app_and_the_probe`; `tests/cookies.rs` `cookies_the_browser_handed_over_sign_that_sites_requests_in_and_no_others`, including yt-dlp's `--add-headers`). A `cookies.txt` chosen in Options › Network is sent on dm's requests and handed to yt-dlp; without either, a cookie-gated link is refused by dm and finishes in the browser (`chrome-check.py`). The popup switch itself is not browser-driven here (gap 32) |

## O. Network and proxy

| Behaviour | Status | Evidence |
|---|---|---|
| HTTP, HTTPS, SOCKS4 and SOCKS5 proxies with credentials, on every request of dm's own; SOCKS5 resolves names at the proxy | PASS | `proxy.rs` tests, including `a_socks5_proxy_carries_the_download_and_resolves_its_name` (a loopback SOCKS5 proxy sees only names); `app-check.py` (a dead proxy is not gone around) |
| The computer's own proxy settings as a kind (environment variables; macOS network settings) | PASS | `tests/proxy.rs` `the_computers_proxy_settings_are_the_environments`; off or a proxy of dm's own ignores the environment (`builder_with`) |
| A bypass list: those sites fetched directly, the rest through the proxy | PASS | `tests/proxy.rs` `a_site_on_the_bypass_list_is_fetched_directly_and_the_rest_through_the_proxy`; `proxy.rs` `the_bypass_list_covers_names_their_subdomains_addresses_and_everything`; the FTP client and the tools read the same list (`Options::proxy_for`) |
| FTP through a SOCKS5, SOCKS4 or HTTP (CONNECT) proxy, control and data connections both | PASS | `tests/ftp.rs` `an_ftp_download_goes_through_a_socks5_proxy_and_not_around_it` (a loopback SOCKS5 proxy with a login sees every connection; a wrong proxy password is its refusal), `an_ftp_download_goes_through_an_http_proxy_by_connect`, `a_bypassed_site_is_fetched_directly_and_an_https_proxy_is_refused` |
| A proxy a route cannot go through is a refusal, never a direct connection | PASS | FTP under an HTTPS proxy (`tests/ftp.rs`); a torrent under anything but SOCKS5 (`torrent.rs` `only_a_socks5_proxy_can_carry_a_swarm`); a dead proxy fails the download (`app-check.py`) |
| PAC scripts | N/A | a proxy auto-config file is JavaScript, which dm does not run; the computer's settings or a proxy of its own cover the cases dm has met |

## K. Media

| Behaviour | Status | Evidence |
|---|---|---|
| A stream that arrives as a transport stream is saved as an MP4 where an ffmpeg can remux it (Options › Downloads, on by default); the `.ts` where not asked or not possible | PASS | `hls.rs` `a_transport_stream_is_saved_as_an_mp4_where_ffmpeg_can_and_as_ts_where_not_asked`: a real stream made by ffmpeg lands as `show.mp4` that ffmpeg decodes to the end, nothing left staged; off, the segments in order as `show.ts` |
| Subtitles fetched and embedded by yt-dlp when asked (Options › Downloads), only where an ffmpeg can embed them | PASS | `dash.rs` `subtitles_are_fetched_and_embedded_only_where_an_ffmpeg_can_embed_them` (the stand-in records `--write-subs --sub-langs all --embed-subs`; nothing without ffmpeg or when off) |
| Direct media files | PASS | ordinary downloads |
| HLS, including AES-128 and resume | PASS | `hls.rs`; `app-check.py` |
| HLS behind redirects: entries joined against where each playlist landed | PASS | `hls.rs` `relative_entries_are_joined_against_where_a_redirect_landed` |
| Sound alone named for what its segments are: by their first bytes (TS, fMP4, ADTS bare or behind ID3), else by their addresses (`.m4a`, `.ts`, `.aac`); a second, different initialisation segment (compared as resolved addresses) goes to yt-dlp | PASS | `stream.rs` `sound_alone_is_named_for_what_its_segments_are`, `segments_are_told_apart_by_their_first_bytes`, `a_second_initialisation_segment_is_refused_not_swapped_in`; `hls.rs` `packed_audio_from_tokenised_addresses_is_saved_as_aac` |
| A refusal's body is read no further than 64 KiB and for no longer than 5 s | PASS | `errors.rs` `a_refusal_whose_body_never_ends_is_answered_promptly` (a 500 whose body never ends) |
| yt-dlp is given the proxy in a 0600 config file of the run's own, never on its command line | PASS | `dash.rs` `with_ytdlp_a_dash_manifest_is_handed_over_with_the_proxy_and_the_page` (no password in argv; the file gone after the run); a real yt-dlp 2026.08.19 read the file's quoting (`--verbose` proxy map) |
| A torrent's own name is cleaned before its folder is made | PASS | `torrent.rs` `a_torrents_own_name_cannot_lead_out_of_the_download_folder` (`../.config/autostart`) |
| A cleared row's run still winding down counts against the limit | PASS | `queue.rs` `a_cleared_row_holds_its_slot_until_its_run_lets_go` |
| yt-dlp progress over picture then sound only goes forward | PASS | `ytdlp_tree.rs` `progress_across_picture_and_sound_only_goes_forward`; `ytdlp.rs` unit tests (float estimates) |
| A link to a page named as a document (`notes.html`) is fetched by dm's own client, with the row's referrer and cookies, whether or not yt-dlp is there; a quality picked in the browser's panel still goes to yt-dlp | PASS | `pages.rs` (`a_page_named_as_a_document_is_fetched_by_dm_even_where_yt_dlp_is_refused`: a referrer-gated `report.html`, the stand-in yt-dlp never run) |
| DASH | PASS | handed to yt-dlp; refused with the reason without it (`dash.rs`) |
| Player pages | PASS | handed to the user's yt-dlp; `ytdlp.rs` against a loopback server |
| DRM or site-specific extraction | N/A | not done, by design |

## L. Window (`crates/dm-desktop`, egui)

Drawn in Rust, no web view: menu bar (Program, File, Downloads, View, Help),
toolbar, category tree, a sortable table, the row menu, and dm's dialogs.
Every menu item, toolbar button and row-menu item is clicked by its name and
checked for what it does (`every_menu_item_clicked_does_what_it_says`,
`every_toolbar_button_clicked_does_what_it_says`,
`every_row_menu_item_clicked_does_what_it_says`); this table is what the
main ones are checked to *do*.
Evidence is `dm-desktop/tests/window.rs` unless it says otherwise.

| Behaviour | Status | Evidence |
|---|---|---|
| Add URL → Download details → Start now: byte-exact, announced once, Download complete with Open | PASS | `add_url_downloads_the_file_announces_it_once_and_says_it_is_complete` |
| Folder, Save as and Note from the Download details dialog | PASS | `the_file_info_dialog_puts_the_file_where_and_as_what_it_says`; engine side `dm-core/tests/rows.rs` |
| Add for later into the Main queue; Start queue | PASS | `download_later_waits_in_the_main_queue_until_start_queue`; `rows.rs` |
| Toolbar acts on the selection: Resume, Stop, Stop all, Delete | PASS | `the_toolbar_acts_on_the_selected_row` |
| Delete only for complete or stopped downloads, asking first for an unfinished one | PASS | `delete_waits_for_a_download_to_stop`, `the_toolbar_acts_on_the_selected_row` |
| Properties edit the username and password, kept out of the address box | PASS | `properties_keep_the_login_out_of_the_address_box` |
| dm's command line: `/d URL /p folder /f name /a`, `/s` | PASS | `scripts/app-check.py` against the real binary; `dm-shell` command-line tests |
| Category tree: Everything with its kinds, In progress, Done, queues | PASS | `the_category_tree_filters_the_one_list` |
| Sorting by column heading, and back | PASS | `clicking_a_heading_sorts_and_clicking_again_reverses` |
| Properties: note, Update download link | PASS | `properties_change_the_description_and_refresh_an_expired_address`; `rows.rs` |
| Redownload, Move/Rename, move up/down, stop a queue | PASS | `dm-core/tests/rows.rs` |
| Add numbered series (`*` with numbers or letters) | PASS | `a_batch_adds_every_address_the_star_makes`; `dm-shell` `lists` tests |
| Add links from clipboard | PASS | `window.rs` `links_on_the_clipboard_are_picked_and_put_in_the_main_queue`: only the links are offered, and Add for later puts both in the main queue, paused |
| Find files on a site: by file type, pages deep on the same site (at most 100 pages), paths kept or skipped by what they contain; the site is also where the start page redirected to | PASS | `dm-shell/tests/crawl.rs` (depth, filters, the page limit, a file or a missing page as the start, `a_start_page_that_sends_everyone_to_another_host_is_searched_there`); `window.rs` `the_site_file_finder_finds_the_files_a_page_links_to`, `a_site_search_reads_pages_deep_and_a_saved_one_runs_on_its_own` |
| A site search saved under a name, run again by hand or every so many hours; what is new is added for later | PASS | `window.rs` `a_site_search_reads_pages_deep_and_a_saved_one_runs_on_its_own`: saved to run hourly it runs at once and adds three files for later; run again it says it found nothing new; deleted |
| Export and Import as dm's `.dmlist` (JSON), keeping each row's queue, note, chosen name and digest (older lists without them still read); a plain list of links imports too, without the punctuation around them | PASS | `window.rs` (the export and import test); `backend.rs` `an_exported_list_brings_back_queue_note_name_and_digest`; `dm-shell` `lists` tests (`a_link_in_prose_is_read_without_the_punctuation_around_it`, `a_list_written_before_rows_kept_more_than_an_address_still_reads`) |
| Find / Find next | PASS | `find_next_selects_the_next_row_with_the_name` |
| Options › Browser: which downloads the browser hands over | PASS | `the_file_types_the_browser_hands_over_are_set_in_options`; the extension reading them: `scripts/chrome-check.py` ("the app's settings decide") |
| Options on OK, saved; a failure puts it back | PASS | `options_are_put_into_force_on_ok_and_written_down`, `an_option_that_cannot_be_applied_is_put_back_and_said` |
| Speed limiter on/off remembers its value | PASS | `the_speed_limiter_turns_off_and_back_on_to_its_last_value` |
| Sign-ins / Needs a sign-in reach the server | PASS | `dm-core` `a_login_in_the_address_is_sent_as_basic_auth` (reqwest dropped the login after the probe until fixed); `dm-shell` `a_site_login_goes_on_the_hosts_it_names_and_no_others`; the password is never shown (`format` test) |
| A server that wants a Digest sign-in (MD5, SHA-256, `-sess`, `qop=auth`, a nonce that expires) | PASS | `tests/auth.rs`: the file downloads with one challenge and every range signed in from the start; SHA-256 and a stale nonce; a wrong password refused with the 401 and not retried; Negotiate and NTLM said rather than tried; Basic unchanged |
| An address already in the list is asked about: download again, show the row, or leave it | PASS | `an_address_already_in_the_list_is_asked_about_before_it_is_added_again` |
| Referrer and Folder columns, sortable, hideable, movable like the rest | PASS | `Column::ALL`; `a_column_turned_off_is_gone_and_comes_back` drives the column menu; `every_control_has_a_name_a_screen_reader_can_say` |
| A sound on failure, apart from the one on finishing | PASS | `a_failed_download_plays_the_failure_sound_when_asked` (once per failure) |
| When done: hibernate, where the computer has the command; greyed where not (macOS) | PASS | `power.rs` tests; `every_menu_item_clicked_does_what_it_says` presses it where available |
| Progress window with each connection and the real Can resume | PASS | `a_progress_window_shows_each_connection` (it used to say "Yes" for a server without ranges) |
| Every menu item, toolbar button and row-menu item, clicked by its name, does what it says | PASS | `every_menu_item_clicked_does_what_it_says`, `every_toolbar_button_clicked_does_what_it_says`, `every_row_menu_item_clicked_does_what_it_says` |
| Every registry command runs, with rows in every state | PASS | `every_command_runs_against_rows_in_every_state` |
| Control protocol: token, commands, inspect, screenshot | PASS | `the_control_protocol_runs_commands_and_reads_the_window_back`; `app-check.py` against the real binary, including a PNG of the running window |
| Clipboard watcher | PASS | `the_clipboard_watcher_offers_only_new_links_and_only_when_on` |
| Every control has a name a screen reader says | PASS | `every_control_has_a_name_a_screen_reader_can_say` |
| 500 rows | PASS | `five_hundred_rows_draw_inside_a_frame_budget` |
| Lock, second launch, browser hand-off, HLS, yt-dlp, proxy, speed limit, schedule | PASS | `scripts/app-check.py` on macOS and Linux |
| `--hidden` opens no window | PASS | counted under Xvfb in `linux-check.sh`; a one-frame flash is possible |
| Tray: Show, Quit, close hides into it | PASS (Linux) | `scripts/linux-gui-check.sh` drives the StatusNotifierItem as a panel does: the item registers with a watcher, closing hides into it, Show dm, a left click (Activate), Speed limiter and Quit act; a watcher that is missing, late or restarted is handled. Physical clicks on a real panel, and macOS and Windows, not driven |
| Dock click brings a hidden window back (macOS) | NOT VERIFIED | wired through winit's own delegate class; not clicked |
| Dragging a link from a browser onto the window | PARTIAL | winit delivers only files: dropped `.webloc`, `.url` and text files are read for links |
| Dragging finished files out of the window | PARTIAL | macOS and Windows through the `drag` crate, driven in `window.rs`, never watched landing; Linux: not available (the crate needs a GTK window), said in the README |
| Start on login (Linux) | PASS | `scripts/linux-gui-check.sh`: the setting writes an XDG autostart entry, `dex` starts it as a session does, dm waits in the tray with no window, and turning it off removes the entry |
| Dark theme | PASS | rendered both ways for the README |

## N. Measured

| Behaviour | Status | Evidence |
|---|---|---|
| Never crash on outside input: no unwrap, expect or panic in shipped code; checked indexing where outside input is read | PASS | clippy denies them in every crate, the WebAssembly build included |
| Every reader of outside input fuzzed | PASS | 21 property tests in `tests/fuzz.rs` of five crates; a weekly deep run (`.github/workflows/fuzz.yml`). It found ". A" cleaning to " A" |
| The crates keep their layers | PASS | `cargo xtask layers` (a test too): the engine has no window, the extension no sockets |
| Nothing phones home: no analytics or crash reporting, no address but the release page | PASS | `cargo xtask privacy` (a test too) |
| A generated scorecard | PASS | [scorecard.md](scorecard.md), `cargo xtask scorecard`, held up to date by a test |
| Headless, for a server or a script | PASS | `dm-desktop --headless --control 0`, driven in `app-check.py` |
| A batch of up to 256 control requests | PASS | `a_batch_runs_its_steps_in_order` |
| An MCP server for AI clients | PASS | `dm-desktop mcp`, driven in `the_mcp_bridge_gives_an_agent_dm_as_tools` and, as a real subprocess, in `app-check.py`; a dm that restarts on another port is connected to again (`mcp.rs` `a_dm_that_restarts_is_connected_to_again`) |
| The control port cannot be held without the token | PASS | `control.rs`: idle connections wait in their own list of 16, the oldest closed for a newcomer (`idle_connections_without_the_token_do_not_keep_the_owner_out`); 5 s to authenticate, however slowly sent (`a_connection_that_does_not_authenticate_in_time_is_closed`); a non-JSON first line ends it (`a_line_that_is_not_json_before_the_token_ends_the_connection`); a request pending as dm quits is told "dm is closing" (`a_request_waiting_as_dm_quits_is_told_dm_is_closing`) |
| Control requests that ask for nothing are refused | PASS | `window.rs` `the_control_protocol_refuses_nothing_to_add_and_a_queue_that_is_not_there`: an empty `urls`, a `queue:` never made |
| SIGTERM and SIGINT quit as Program › Exit does | PASS | `tests/quit.rs` spawns the real binary headless, signals it, and sees exit status 0 and `control.port` gone (both fail without the handler). Windows' console events are listened for, never run |
| One dm at a time, told truly | PASS | `instance.rs`: a lock that cannot be opened is an error, not "running" (`a_lock_that_cannot_be_opened_is_an_error_not_a_running_dm`); a claim waits out the host's probe (`a_dm_starting_while_the_host_probes_waits_the_probe_out`). dm then exits saying why, on stderr and in a dialog; the host refuses the hand-off so the browser keeps it |
| A browser launch told from a person's by its whole shape | PASS | `shell.rs` `a_link_or_a_name_never_makes_a_launch_the_browsers` |
| Two hand-offs never share one inbox file | PASS | `host.rs` `two_hand_offs_never_share_one_file` |
| A link with no site as typed, or a magnet with no torrent hash, refused at add | PASS | `shell.rs` `a_link_with_no_site_as_typed_is_refused_though_it_parses`, `a_magnet_without_a_torrent_hash_is_refused_at_add` |

## M. Platforms and packaging

| Behaviour | Status | Evidence |
|---|---|---|
| macOS app and dmg | PASS | `cargo packager` builds a 7.9 MB dmg; the binary inside `dm.app` passes `app-check.py` |
| Linux client | PASS | `scripts/linux-check.sh` (Debian container, 36 checks; no desktop tray host under Xvfb) |
| Linux deb and AppImage | PARTIAL | `scripts/linux-package.sh` builds both with `cargo packager` in Debian 12 arm64 (no WebKitGTK installed), installs the 10 MB `.deb` through apt and drives the installed `/usr/bin/dm-desktop` through `app-check.py` (all pass); the 19 MB AppImage (95 MB under Tauri) answers the browser's handshake. Not built on x86_64 or on CI's Ubuntu 22.04 |
| Arch (PKGBUILD) | PASS | `scripts/arch-check.sh`: the release PKGBUILD through makepkg in Arch Linux ARM (build, its `check()` of the engine, package), installed with pacman, the installed `dm` answers a browser hand-off. The first run found the memory test warming too little for glibc's per-thread arenas; it now warms as much as it measures |
| Windows | NOT VERIFIED | CI written, never run |
| Code signing and notarisation | BLOCKED | needs bought certificates |
| Login item at a real login | NOT VERIFIED | the `--hidden` launch is checked; a real login is not |
