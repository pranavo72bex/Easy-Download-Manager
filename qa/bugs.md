# Bugs found in this round, 2026-10-19 to 2026-10-20

Each bug was found by a hands-on run (the Linux container with real input,
or the measurements in `qa/baseline.md` and `qa/after.md`) and fixed with a
test where one could hold it. Earlier rounds' bugs are in `docs/gaps.md`
(Closed).

| # | Severity | What happened | Root cause | Fix | Test |
|---|---|---|---|---|---|
| 1 | Major | A multi-connection download spent 7–9 s of 11–14 s for 2 GB copying parts into the file | Ranges went into part files, which were copied into the finished file | Written in place into one staged file; finishing is a rename (b1a46de) | `ranges_are_written_in_place_and_finishing_copies_nothing` |
| 2 | Major | A row deleted after a pause could leave its folder behind (seen under load while making 1) | A connection still stopping wrote its progress by path, making the file again in the folder being removed | The progress file is held open by the attempt (b1a46de) | `deleting_a_row_removes_the_parts_where_it_began_after_the_folder_changed` |
| 3 | Major | A download larger than the disk said "File too large" instead of "not enough room" (seen while making 1) | The staged file was made full length before the room was checked | Made full length only once there is room (b1a46de) | `a_file_larger_than_the_disk_is_refused_before_it_is_fetched` |
| 4 | Moderate | With 10 downloads running, the window took 14.6 % of a core against 9.7 % | Every queue change asked for an immediate repaint | A repaint within 250 ms, so the requests merge (48d3a13) | Measured (`qa/after.md`) |
| 5 | Moderate | The panel never showed when the pointer rested after one move over a player | The move that loaded the panel's code came before its listener | The point is handed over once loaded (9a96809) | Container run, case (c) |
| 6 | Moderate | A pointer entering a player within half a second of a check elsewhere loaded nothing | The check was throttled on the leading edge only | It also runs at the end of each half second (9a96809) | Container run, case (c) |
| 7 | Moderate | No panel for a player two shadow roots deep | Only the outer shadow root was searched | Searched four deep (9a96809) | Container run, case (c) |
| 8 | Moderate | No panel for a video that played as it was added | The play that loaded the code was used up | The element that played is handed over (9a96809) | Container run, case (g) |
| 9 | Moderate | The list's in-row progress bar was never drawn | A text line, a 4 px gap and 8 px spacing pushed it past the 32 px row | Tighter spacing inside the cell (576ae61) | Container run |
| 10 | Moderate | The footer band covered the last field of most dialogs | The gap above the band was measured from the wrong edge | 24 px below the body (576ae61, c84913d) | `a_category_made_in_options_takes_its_files_into_its_own_folder` and others |
| 11 | Moderate | At large text, dialog buttons scrolled away with the body | The scrolled part included the footer | The footer is kept below the scrolled part (576ae61) | `properties_fit_a_small_window_and_close_from_the_title_bar` |
| 12 | Moderate | Options at 200 % stood wider than the window | A radio row shrank a choice's words to a letter a line; a fixed dialog's body took the width it had grown to | Radio groups stacked; a fixed dialog's body held to its frame (c0b450c) | `options_fits_the_window_at_large_text` |
| 13 | Moderate | No tray: closing the window quit at once while downloads ran | The close went through when no tray was up | Asked first, as Exit does (398a34a) | Container run |
| 14 | Moderate | Hidden in the tray, the window still drew | A repaint was asked for on every refresh | Not drawn while hidden; slower wakes (398a34a) | Container run: 9 % → 1.9 % |
| 15 | Minor | When done never fired for a 1 MB file | A row first seen finished was not counted | Counted after the first look (398a34a) | `a_download_that_came_and_went_between_two_looks_fires` |
| 16 | Minor | `--unregister` left a running dm, which wrote back what it removed | Nothing told the running dm | It is asked to end; the login setting is turned off (398a34a) | `a_quit_is_asked_only_of_a_running_dm` |
| 17 | Minor | One Japanese name cost 36 MB | The whole CJK font file was read into memory | Mapped instead (a5700d9) | Measured (`qa/after.md`) |
| 18 | Minor | Options' unpicked tabs were in the primary colour | The colour given when laid out won over the one given when drawn | Laid out in their own colour (c84913d) | Screenshot |
| 19 | Moderate | After ten downloads, 40 threads (Linux: 100, and 200+ MB) stayed for the life of dm | The inbox watcher's file calls went through tokio's blocking pool twice a second, waking its parked threads in turn so none was idle long enough to end | Plain file calls on the watcher's own thread | Measured: 40 → 13 threads a minute after (macOS, `qa/after.md`) |
| 20 | Moderate | A login launch made and hid a whole window: 43 MB in the tray, and a one-frame flash | eframe makes its window when it starts | No window until one is asked for (`run_hidden`) | `app-check.py` (tray: control answers, a download finishes, a second launch shows a drawn window); `linux-gui-check.sh` 71/71 |
| 21 | Minor | A failed row said only "Error"; the disk being full was in a tooltip | The status cell drew a fixed word | The plain reason in the cell, truncated to the column; all of it on hover | `a_cut_connection_is_said_plainly` |
| 22 | Minor | Time left read "Time le" at the window's first size | The name kept 220 points while only Status was fitted beside it | The name gives way so Speed, else Time left, fits; the empty list's headings use the same width | `the_name_gives_way_first_in_a_narrow_window`, `the_columns_start_in_one_place_whatever_the_rows_queues` |
| 23 | Minor | "Not enough room" named the row's staging folder (`/dl/parts/2`) | The folder checked was named | The download folder is named where the two share a disk | `errors.rs` (the huge-file case) |
| 24 | Moderate | A cut error showed two tooltips, one over the other | egui's own tooltip for elided words and dm's | egui's turned off for that label | QA loop 2 (screenshots) |
| 25 | Minor | "Error: the server has no file …" lost the 404 at the column's edge | The cell repeated "Error:" beside the red mark | The reason alone in the cell; "Error:" and all on hover | QA loop 2 |
| 26 | Minor | A scroll bar under Options' tabs at 150% that scrolled nothing | The tabs were fitted to the exact width | Fitted 4 points short | `options_tabs_fit_at_150_percent` |
| 27 | Minor | A download given a name, running beside one of the same file, pushed the other to "same (1).bin" | The named one was placed under the server's name and renamed after | Placed under the chosen name in the first place, cleaned as any outside name is | `a_chosen_name_does_not_take_the_servers_name_from_another_row`, `a_chosen_name_cannot_leave_its_folder` |

Not dm's: my first throughput run counted 404s as downloads (my test
server kept the query string in the file name), and later runs slowed as
the disk filled with finished 2 GB files. Both measurements were taken
again; `qa/after.md` holds the clean ones.
