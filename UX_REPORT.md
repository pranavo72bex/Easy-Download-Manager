# UX report — the ten tasks, and the ten heuristics

Run on macOS with Microsoft Edge 156 and a local build of dm 0.7.1, on
2026-10-26, against the "make it effortless" standards. **How, and what was
not possible:** real downloads in the real Edge (a local server, with the
server's log and the saved files as the evidence), the repo's browser check in
a throwaway Edge profile, the window drawn to pictures by `scripts/ui-shot.py`,
and the test suites. Nobody sat at the screen, so *times are measured, not
felt*, and a screen that was not drawn is **not scored** below. Not run here:
a Windows install, a real cable pulled, a real reboot (the engine's own tests
for them are cited).

## The ten tasks

| # | Task | Result | Steps / clicks | Friction found | What was done |
|---|---|---|---|---|---|
| 1 | Install, then download a file from the browser | **Met once installed.** Clicking a link: Edge's own download is cancelled and dm is fetching it in the same second (server log: both at 14:47:45), no file of Edge's, 0 downloads in Edge's history. | 1 click on the link; the install itself is not zero (app + extension, then the extension must be loaded) | Two stale saved settings kept some types in the browser and made every catch wait for a click | Saved catch lists brought up to the defaults once; "ask first" off by default and the window now comes forward when it does ask; PDFs, videos and songs (which the browser *shows*, so never "downloads") are taken on a plain click |
| 2 | Video from a page, in another quality | **Met** (browser check, Edge): a "Download video" button appears on the player, the panel lists qualities behind its arrow, the default is taken at once | hover, 1 click (2 for a quality) | Needs the yt-dlp helper for most sites | none needed |
| 3 | Paste 5 URLs, download as a batch | **Was not met; fixed.** Add URL answered "That looks like two links. Add one at a time…". Now five pasted links open one *Download details* for all five | paste, OK, Start now | the refusal | Add URL starts a batch when links are separated; links run together with nothing between still ask (where one ends is a guess). Test: `five_links_pasted_into_add_url_are_one_batch` |
| 4 | Network gone mid-download, then back | **Met by tests** (`a_link_that_drops_every_half_megabyte_still_delivers_the_file`, `a_connection_dropped_mid_range_is_retried`). Row says "Connection dropped (retry n of m)". Not run with a real cable | 0 | the retry wording says nothing about *when* it will try again | open (below) |
| 5 | Restart mid-download | **Met by tests** (`the_queue_survives_the_app_closing`, `a_power_cut_that_lost_a_block…_is_repaired_not_restarted`, `what_the_schedule_paused_is_started_after_a_restart`) | 0 | none found | — |
| 6 | Recover after an expired link | **Met.** Update download link has "Get it from the browser": it opens the page the file came from and takes the next download clicked there as the new link (`an_expired_link_is_refreshed_and_the_download_completes`) | menu or row, 1 click, then click the file on the page | needs the person to click the file again | — |
| 7 | Find a file from last week and open its folder | **Met.** Search filters instantly (name, address, note); Done and the categories narrow it; "Open folder" is on every row; a finished row now says "Complete" and when | type, right-click, Open folder | there was no date on a finished row | the date beside "Complete" (`a_finished_row_says_when_it_finished`) |
| 8 | Schedule a big download for the night, shut down when done | **Met.** Scheduler: "The hours when downloads may run, in every queue", plain times and days; Downloads › When done: Shut down; a 5 GB download now says once that the Scheduler can run it at night | 3 dialogs | the hint was not offered | said once (`a_very_big_download_is_told_about_the_scheduler_once`) |
| 9 | Remove a download by mistake, then undo | **Was not met; fixed for finished downloads.** Remove asked a question first and had no Undo. A finished one now leaves the list at once and the status bar offers **Undo** for 5 seconds; Undo puts it back where it was. Delete file is its own command with a question, and goes to the Recycle Bin or the Trash, not erased | 1 click, 1 click to undo | the question for a routine action; a delete that could not be got back | engine `finished_rows`/`put_back`, tests in `dm-core` and the window; unfinished rows still ask (their fetched bytes go with them) |
| 10 | Change the folder for videos only | **Met.** Options › Folders: categories with their own folders | 2 dialogs | no description under it | Folders already explains its categories; Restore defaults added |

## Also fixed from the standards

- Double-click opens a finished file (it showed Properties); a download that is
  not finished still shows Properties.
- A running download of known size with no rate yet says **Calculating…** in
  Time left, not a blank. Time left is smoothed already (`dm-core/src/progress.rs`).
- Dialogs' fields and buttons are 32 points tall (Fluent 2).
- The close button asks once whether dm keeps running in the tray, and keeps the
  answer; the welcome now asks where downloads go and ends with Done.
- "Pause capturing on this site" in the extension's popup (checked in Edge:
  a paused site's download stays with the browser, and goes to dm again once
  it is not paused).
- A dead link reads "this link no longer works".

## The ten heuristics, screen by screen

Scores 1–5, only for screens that were drawn and looked at. Below 4 are listed
after the table with what was done.

| Screen | 1 Status | 2 Real world | 3 Control | 4 Consistent | 5 Prevent errors | 6 Recognise | 7 Efficient | 8 Minimal | 9 Recover | 10 Help |
|---|---|---|---|---|---|---|---|---|---|---|
| Main window | 4 | 4 | **4** (was 3) | 4 | 4 | 4 | 4 | 3 | 4 | 3 |
| Add URL | 4 | 4 | 4 | 4 | 4 | 4 | **4** (was 3) | 5 | 4 | 4 |
| Properties | 4 | 3 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 3 |
| Options › General | 4 | 3 | 3 | 4 | 4 | 3 | 4 | 4 | 3 | 2 |
| Scheduler | 4 | 5 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 |

Not scored because not drawn and looked at: Download details, the Delete file
question, the other Options pages, the extension's popup, the video panel, the
completion window.

### Below 4, and what was done since
Scores above are as drawn at the time; what has changed since is listed, and
a screen is re-scored only where it was drawn again.

- **Main window · 8 Minimal (3):** the Queue column is now drawn only while a
  download has a queue, and finished rows say when they finished (drawn
  again). The sixth column is now Last try, still cut by the sideways scroll:
  the extra columns are reached by scrolling, by design. Stays 3.
- **Main window · 10 Help (3):** a very big download now says once that the
  Scheduler can run it at night, and the welcome asks where downloads go and
  ends with Done. Not drawn again; not re-scored.
- **Properties · 2 Real world (3), 10 Help (3):** Referrer and SHA-256 now
  explain themselves on hover. Not drawn again; not re-scored.
- **Options › General:** a line under 13 of the settings with the program's
  own names (including the command line), Restore defaults on each tab, and
  "When I close the window". Only the top of General was drawn again, which
  shows no line yet; not re-scored. The theme's four icons have names on hover
  and for a screen reader (checked in the code).

## What still falls short of the standards (all in `docs/gaps.md`)

- Settings: 13 have a line under them and each tab has Restore defaults; the
  rest have no line and there is no separate Advanced page (row 16).
- Nothing holds notifications back while a fullscreen app or Focus Assist is
  on. Blocked: the Windows code cannot be compiled on this machine (row 14).
- A retry does not say when it will try again (row 17).
- The window's own controls are 24 tall, not 32 (row 10); dialogs are 32.
