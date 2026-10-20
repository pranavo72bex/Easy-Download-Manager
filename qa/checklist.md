# QA checklist (Phases 3 and 4), 2026-10-20

Status words:
- **PASS:** shown by a test or a run named in the Evidence column.
- **PARTIAL:** shown for part of the item, with the part not shown said.
- **FAIL:** missing or wrong.
- **NOT VERIFIED:** nothing here could show it. It needs a person, a real
  Windows machine, a reboot, or hours. Never counted as a pass.

Evidence, by name:
- `window.rs`, `queue.rs`, `download.rs`, `integrity.rs`, `errors.rs`: test
  files in `desktop/crates/*/tests`.
- "Linux QA": the hands-on container runs of 2026-10-19 and 2026-10-20
  (Debian 12, Xvfb, real input). Their reports are summarised in `qa/bugs.md`.
- `chrome-check.py`: the extension in Chromium 155 against a real dm,
  57 of 57 on 2026-10-20.

dm has never run on Windows (gap 2), so every item that is about Windows
itself is NOT VERIFIED.

## Phase 3: a person using it

### 3.1 Install and first run

| ID | Item | Status | Evidence, or what is missing |
|---|---|---|---|
| 3.1.1 | Runs without admin | PARTIAL | macOS `.app` and the Linux AppImage run without it, and the `.deb`/`.rpm` install through the package manager; Windows not verified |
| 3.1.2 | Not flagged by SmartScreen or antivirus | FAIL | Unsigned (gap 5); SmartScreen and Gatekeeper warn |
| 3.1.3 | First window fast, sensible size, follows light/dark | PARTIAL | Follows the system's look (`the_default_look_is_modern_and_follows_the_computer`); size and place remembered. First frame 0.40–0.47 s on this Mac and 0.13–0.18 s on Linux, against a 300 ms target |
| 3.1.4 | Default folder exists, is writable, is said | PASS | The status bar names it; an unwritable one is said before fetching (`a_download_folder_that_cannot_be_written_is_said_before_fetching`) |
| 3.1.5 | Non-English user name, spaces or Unicode in paths | NOT VERIFIED | Unicode file names are fuzzed and held to `shared/naming-cases.json`; a Windows account with such a name has never been tried |

### 3.2 Adding downloads

| ID | Item | Status | Evidence |
|---|---|---|---|
| 3.2.1 | Paste a URL, add, it starts; name and size right | PASS | `add_url_downloads_the_file_announces_it_once_and_says_it_is_complete`; Linux QA |
| 3.2.2 | Redirects, query strings, `Content-Disposition` | PASS | The final URL is used; `the_name_the_server_offers_is_used`; zoo cases (`docs/zoo.md`) |
| 3.2.3 | Unicode, very long, or Windows-illegal names cleaned | PASS | `shared/naming-cases.json`, fuzzed in `cargo test` |
| 3.2.4 | Same name already there: renamed, never overwritten | PASS | `a_second_download_does_not_overwrite_the_first`; `copies_waiting_in_the_list_each_get_their_own_name` |
| 3.2.5 | No `Range`: one connection, and said not resumable | PASS | `a_server_without_ranges_still_produces_the_right_file`; the progress window's "Can resume: No" |
| 3.2.6 | No `Content-Length`: unknown size, still completes | PASS | `a_file_of_unknown_length_downloads`, `a_file_of_unknown_length_cut_short_is_not_reported_complete` |
| 3.2.7 | Files over 4 GB complete with the right hash | NOT VERIFIED | 2 GB measured today, not hashed; a test checks a large file never needs twice its size on disk. No run over 4 GB |
| 3.2.8 | Bad URL, 404, 403, expired link: a human message | PASS | `format::REASONS`; `a_refused_sign_in_says_where_to_give_one`; Linux QA |
| 3.2.9 | Clipboard watching catches links without spamming | PASS | `the_clipboard_watcher_offers_only_new_links_and_only_when_on` |

### 3.3 Browser extension

| ID | Item | Status | Evidence |
|---|---|---|---|
| 3.3.1 | Chrome takes the download, the browser does not too | PASS | `chrome-check.py` 57/57: the file is byte-exact in dm's folder and the browser's list is empty |
| 3.3.2 | Edge | PARTIAL | The store listing's id is allowed (`a_manifest_without_the_edge_stores_id_is_rewritten`); the extension from the Edge store has not been run against dm |
| 3.3.3 | Firefox | PARTIAL | `firefox-check.py native` passed before the last page-script change; not run since |
| 3.3.4 | Cookies, referrer, sign-ins passed on | PASS | `tests/cookies.rs`, `host.rs`; `chrome-check.py` (a cookie-gated file, the referrer on dm's requests) |
| 3.3.5 | App closed: started, or the browser keeps it, with a message | PASS | Linux QA case 8 (a click starts dm); `chrome-check.py` with the host unregistered (the browser keeps the download) |
| 3.3.6 | Video button appears and works | PASS | 22/22 cases in the container on 2026-10-20, including late players, nested shadow roots, autoplay and `blob:` players |
| 3.3.7 | Extension and app versions mismatched: a clear message | PASS (unit) | `decide.rs` tests of the mismatch wording; not seen in a browser |
| 3.3.8 | Private windows | PASS (unit) | Nothing is handed over from one (unit-tested); not seen in a browser |

### 3.4 Download control

| ID | Item | Status | Evidence |
|---|---|---|---|
| 3.4.1 | Pause, resume, cancel, restart; buttons update | PASS | `window.rs` (each button on its own row); `a_paused_download_stops_and_keeps_what_it_had`, `a_cancelled_download_leaves_nothing_saved` |
| 3.4.2 | Resume after close and after a crash | PASS | Linux QA: Exit and `kill -9` mid-download, relaunch resumes from the parts; zeroed and cut part tails repaired; sha256 matched |
| 3.4.3 | Resume after a reboot | NOT VERIFIED | Not tried; the power-cut repair is tested (`a_power_cut_that_lost_*`) |
| 3.4.4 | Resume after a network drop | PARTIAL | Simulated drops are retried (`a_connection_dropped_mid_range_is_retried`); Wi-Fi turned off for real not tried |
| 3.4.5 | Hash equals the server's in all of the above | PARTIAL | Close and crash: yes (Linux QA). Reboot and Wi-Fi: not tried |
| 3.4.6 | Connections changed mid-download | PASS | `a_changed_connection_setting_does_not_corrupt_a_resumed_download` |
| 3.4.7 | Speed limit within ±10 %, changes live | PASS | `the_shown_speed_keeps_to_the_limit_from_the_first_second`, `lifting_the_limit_takes_effect_on_what_is_already_running`, `tests/speed.rs` |
| 3.4.8 | Disk full mid-download: paused with a clear error, no corruption | NOT VERIFIED | A file larger than the room is refused before fetching (`a_file_larger_than_the_disk_is_refused_before_it_is_fetched`). A disk that fills mid-download under the new in-place writes has not been tried |
| 3.4.9 | Folder deleted, or no permission: a clear error | PASS | `a_download_folder_that_cannot_be_written_is_said_before_fetching` |

### 3.5 Queue, scheduler, categories

| ID | Item | Status | Evidence |
|---|---|---|---|
| 3.5.1 | Max simultaneous downloads | PASS | `downloads_run_together_but_not_all_at_once`, `a_queue_with_a_limit_of_its_own_runs_that_many_*` |
| 3.5.2 | Reorder; start and stop a queue | PASS | `rows_move_up_and_down_the_order`, `download_later_waits_in_the_main_queue_until_start_queue` |
| 3.5.3 | Schedule; sleep or shut down when done, cancellable | PARTIAL | `nothing_starts_while_the_schedule_is_closed`; the countdown and Cancel are tested against a fake; a real sleep or shutdown was run once in Linux QA against a stand-in `systemctl` |
| 3.5.4 | Categories route files by type | PASS | `a_finished_file_is_sorted_into_its_category_folder`, `a_category_made_in_options_takes_its_files_into_its_own_folder` |

### 3.6 List and window

| ID | Item | Status | Evidence |
|---|---|---|---|
| 3.6.1 | 500+ rows: smooth, find, sort | PARTIAL | `five_hundred_rows_draw_inside_a_frame_budget` (debug build); find and sort tested; release-build frame times not measured |
| 3.6.2 | Multi-select and keyboard (Ctrl/Shift, Delete, Enter, Ctrl+A) | PASS | `window.rs` selection and keyboard tests |
| 3.6.3 | Row menu: Open, Open folder, Copy link, Redownload, Delete with confirm | FAIL | All of it is there (`every_row_menu_item_clicked_does_what_it_says`) except **Copy link**, which the menu does not have |
| 3.6.4 | Progress, speed, time left readable, not jittery | PASS | Digits of one width; smoothed speed; the list and the progress window agree |
| 3.6.5 | Completion notification, click opens the file or folder | NOT VERIFIED | The notification is sent; what clicking it does is the desktop's, not checked |

### 3.7 Settings, tray, lifecycle

| ID | Item | Status | Evidence |
|---|---|---|---|
| 3.7.1 | Every setting persists across a restart | PASS | `settings_are_saved_to_a_file_without_passwords_and_loaded_back`; `app-check.py` |
| 3.7.2 | Tray; single instance (a second launch shows the first) | PASS | `app-check.py`; Linux QA (`gio launch` and the command line hand to the running dm) |
| 3.7.3 | Start with Windows | NOT VERIFIED | Linux's login entry is checked (`linux-gui-check.sh`); macOS and Windows never watched at a real login |
| 3.7.4 | Update check does not block, fails politely | PARTIAL | Off the window's thread; unit-tested; never run against a real release (gap 10) |
| 3.7.5 | Uninstall removes it, optionally keeps data | PARTIAL | `--unregister` removes the manifests and the login item and ends a running dm; the packages run nothing on removal |

### 3.8 Over time

| ID | Item | Status | Evidence |
|---|---|---|---|
| 3.8.1 | Two hours, 10 downloads: no memory growth, no crash | NOT VERIFIED | Longest run: 11 minutes, 10 downloads (Linux QA): file handles and sockets steady, memory 139 → 164 MB at a batch change. The memory test runs six downloads |
| 3.8.2 | Idle an hour: CPU near 0 | PARTIAL | 0.17 % over 60 s with the window open (`qa/after.md`); an hour not measured |
| 3.8.3 | Logs rotate and hold no tokens or passwords | PASS | dm keeps no log files. Addresses are shown without their sign-ins (`format::shown_url`), and `cargo xtask privacy` passes |

## Phase 4: Fluent, against the WinUI 3 Gallery

| ID | Item | Status | Evidence, or what is missing |
|---|---|---|---|
| 4.1 | Mica behind the window (Windows 11), solid on Windows 10 | FAIL | Solid base colour everywhere; no Mica. Only Windows has Mica, and dm has never run there |
| 4.2 | Acrylic for flyouts | FAIL | Menus and drop-downs are solid |
| 4.3 | Custom title bar | FAIL | The system's own title bar; the Gallery draws its own |
| 4.4 | NavigationView pane, collapsible, Settings at the bottom | PARTIAL | Pane, expandable groups with the chevron at the right, pills, own icons (2026-10-20); it gives way to a drop-down when narrow. Options is in the menu and toolbar, not at the pane's foot |
| 4.5 | Spacing on a 4 px grid | PASS | `theme.rs` (compact density, 8 between controls, 24 in dialogs) |
| 4.6 | No clipped text at the smallest size | PARTIAL | Large-text tests pass (`every_dialog_keeps_its_title_and_buttons_on_screen_at_large_text`); the last column's heading still clips at 960 wide (gap 71) |
| 4.7 | Type ramp | PASS | Body 14, Caption 12, Body Strong 14 semibold, Subtitle 20 semibold; Segoe UI on Windows, Selawik elsewhere |
| 4.8 | Sentence case | PASS | Labels, buttons and menus |
| 4.9 | Corners 4 px controls, 8 px overlays | PASS | `the_modern_looks_are_winui_3s_brushes` |
| 4.10 | System accent colour | FAIL | dm uses Windows' default accent (#0078d4 family), not the person's own |
| 4.11 | Light and dark follow the system live; high contrast | PARTIAL | Light and dark follow; there is no high-contrast look |
| 4.12 | Subtle borders and shadows | PASS | Hairlines, shadow 28 under dialogs, 16 under menus |
| 4.13 | Text contrast at least 4.5:1 | PASS | `status_words_outlines_and_progress_can_be_read_in_every_look` |
| 4.14 | Fluent icons, 16 px in lists and commands | PASS | Fluent UI System Icons (MIT), 16 px in the pane, 18 px on the toolbar |
| 4.15 | Controls match the Gallery's | PARTIAL | Button, accent button, ContentDialog, CommandBar, drop-down, check box and radio follow WinUI. On/off settings are check boxes, not ToggleSwitches; problems are a list, not InfoBars; no indeterminate progress bar |
| 4.16 | Rest, hover, pressed, disabled, focused states | PASS | `hover_and_press_can_be_seen_on_every_surface`, the focus-ring tests |
| 4.17 | Settings grouped in cards | FAIL | Options groups settings under headings, not in cards |
| 4.18 | Short, purposeful motion; respects "animations off" | PARTIAL | Only egui's short open and close animations; the system's setting is not read |
| 4.19 | Keyboard, Tab order, focus ring | PASS | `window.rs` keyboard and focus tests |
| 4.20 | Screen reader | PARTIAL | `every_control_has_a_name_a_screen_reader_can_say`; Orca tried once (gap 14); Narrator never |
| 4.21 | 100–200 % and per-monitor scaling | PARTIAL | 150 % and 200 % tested (`options_fits_the_window_at_large_text` and others); per-monitor scaling on Windows not verified |
