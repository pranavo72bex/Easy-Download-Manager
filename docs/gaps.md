# Gaps

The work list: what is known to be missing from a production-ready download
manager and is both wanted and possible for free. A row says what is missing
and how it closes; when it closes, its row is deleted (what was done is in
`CHANGELOG.md`, `docs/parity.md` and git history).

| Status       | Meaning                                                      |
|--------------|--------------------------------------------------------------|
| OPEN         | Nothing done, or done but never run                          |
| PARTIAL      | Works and is tested, but not everywhere it must              |
| BLOCKED      | Needs something this project cannot get for free, or a person |

## Bottom line (the maintainer, 2026-11-10)

What every change is measured against. These two never close, and nothing below may be closed
in a way that goes against them:

1. **Better than reference A in every benchmark, in every parameter, on every system.** The
   benchmark is in [`bench/`](../bench/README.md) (`idle`, `fast`, `per_conn`, `flaky`,
   `outage`, `batch`, `big40`, and any case added later). Reference A's numbers of 2026-11-09
   and 10 are frozen in `bench/reference-a-baseline.json` as the floor. It never changes: a
   test and `bench/compare.mjs` pin its digest (maintainer, 2026-11-11). Every parameter is
   judged: time from the address and from the first request, the finish after the last byte,
   bytes fetched twice, memory, CPU and recovery after the outage. dm's median must beat
   reference A's best run, with every file intact. On macOS and Linux dm runs the same harness
   alone, and the machine-bound floors are scaled by the computer's measured ceiling
   (`bench/README.md`). A row closes only when `node bench/compare.mjs` says "better", not just
   close. Any parameter where dm is worse is a row in this list.
2. **Easier to use than reference A.** For the same task dm takes fewer steps, uses clearer
   words and has no dead ends. A gap found here is a row that proposes the change. A change to
   a behaviour in `docs/behaviour-contract.md` still needs the maintainer's approval in words
   before it is made (`CLAUDE.md`).

What reference A does comes from four folders of local notes. They are listed in
`docs/engine-local/sources.md`, which is git-ignored because the notes name it. Their findings
reach dm only as black-box items in `docs/engine-local/requirements.md`, and a run of both apps
must prove each one.

## State (2026-11-09)

The GPUI migration (rows G1 to G9, maintainer, 2026-11-09; `docs/migration/`) is the work that
can be done here: G6, the GPUI shell (G1 to G5, Phase A, closed 2026-11-10). The other rows need a Windows
computer, a device or a person.
On 2026-11-11, on the Windows computer: the GPUI window built, run, checked from its installer (63 of
64, B36's click timing once) and made Windows' default (G6 to G9; the bullet "Windows" under the GPUI migration below).
Checks on 2026-11-09 on the Mac: `cargo test --workspace` passes, line coverage 93.7% (CI's
floor 91%), the extension's 92 Edge checks, `video-sites-check.py` signed out (P0 7 of 7),
`human-qa.py` 64 of 64 rows. The 0.9.0 release's own files, downloaded from its draft (digests
as `latest.json` gives them), with extension 3.3.0's released zip: the macOS `.dmg` 64 of 64 rows;
the arm64 `.deb` installed by apt on Debian 12 with Chromium in Edge's place, 64 of 64 with
`DM_RELEASE=desktop-v0.9.0 sh scripts/linux-human-qa.sh` (B25 against the Linux baselines).

0.9.0 from its GitHub release, tested on Windows on 2026-11-09 (installed with its setup.exe, the
extension from dm-chrome-3.3.0.zip): `smoke.py`, `memory-check.py`, `launch-latency.py`,
`out-of-box-check.py` (30 of 30) and `human-qa.py` (64 of 64 rows) pass. Measured beside
reference A the same day, dm was slower in three cases (rows 166 to 168). On 2026-11-10 rows 167
and 168 closed (a reconnect no longer fetches its end again; past 8, connections only while they
add speed), and row 166 closed (across drives, a file a range as reference A stages one: 512 MiB in
2.1 to 2.9 s from the first request against reference A's 3.1 to 4.9). Row 169 (the extension's
JavaScript moved into Rust) closed on 2026-11-11 with the Firefox run on macOS; row 171 is what is left
of the flaky and outage cases, and row 172 the batch case, which the harness cannot yet compare.
On 2026-11-11 the benchmark moved into the repository (`bench/`). Reference A's 84 runs are
frozen there as the floor for every parameter, on every system. Judged that way, dm is ahead
end to end in `fast`, `per_conn`, `flaky` and `outage`, but worse in memory (row 175), in CPU
over capped connections (row 176), and in the finish after the last byte (row 177).

Removed from the list on 2026-11-09 at the maintainer's word (`docs/decisions.md`, "The work
list keeps only what can be done"): every check that needs a site sign-in (Facebook, Instagram,
Reddit, YouTube signed in, rows 63, 71, 75, 110, 122, 142, 152), one that needs reference A's
dialogs set by hand (row 65's proxies) or a person's hands or eyes on the window (rows 60, 66,
72f), the blocked proof-of-origin token (row 37), a reporter's confirmation (row 30), the site
rules that could only be checked signed in (rows 68 and 69), and the notes that were not work
(their findings are in `docs/reference-behavior.md`).

## Open work

| #   | Gap | Status | How it closes |
|-----|-----|--------|---------------|
| 5   | 89 strings (the 71 reworded in 0.7.0, the 14 of 2026-10-30 for the quality list, the part fields and the Playlist dialog, and Take out's 4) were put into the other twelve languages by a machine pass and have not been read by a native speaker | BLOCKED: native readers | One reader per language corrects them (`git log -p --follow -- desktop/crates/dm-desktop-base/i18n`; before 2026-11-10 they were under `dm-desktop/i18n`) |
| 20  | Stress phases not run on Windows: `fulldisk` (needs a small volume) and `torrent` (needs a sandbox off the public network); the clipboard offer is covered on Linux only, as on macOS and Windows the only clipboard is a person's | PARTIAL | On Windows: `fulldisk` with a VHD as the download folder, `torrent` in a sandbox; the clipboard phase on a machine whose clipboard nobody is using |
| 39  | Secrets sealed under a key in the system's store: proved on Windows, macOS and Linux, except a copy of dm's home started under another Windows account, which must not open them and must say so once | PARTIAL | On a Windows computer with a second account |
| 47  | The second network path (Options › Advanced › Network) is tested with a second local address only | PARTIAL | One run with a phone's connection beside the computer's own, recording the speeds and each path's share |
| G6  | GPUI migration, Phase B: no GPUI shell yet to decide go or no-go (screen readers in every dialog window first; software rendering under Xvfb and GPU-less Windows; B42 placement; tray; drag and drop; IME; size) | PARTIAL: macOS check 1 pass; Windows (2026-11-11) checks 1, 3 and 4 pass, 2 by reading, 5 drop-in coded, 6 by hand, 7 measured; Linux (2026-11-11) checks 1 to 4 pass on X11, 2 and 4 on Wayland, link drops pass on both; owed on every system: drag out (G10); on Linux the 50-window churn, IME by hand and the package's size | `desktop/crates/dm-gpui` on the proposed pin, each check of `docs/migration/GPUI_MIGRATION_PLAN.md` Phase B pass or fail on macOS, Windows, X11 and Wayland; the maintainer's D1 (go) and D2 (pin) |
| G7  | GPUI migration, Phase C: the window in eight slices (main window; table; row commands; Add URL and the questions; queues and Scheduler; progress and failure windows; Options; dialogs, tray, shortcuts, accessibility, B25 pictures) | PARTIAL: every slice holds on macOS (64/64, 2026-11-10) and on Linux (64/64 from a build of the checkout, B25 compared with `docs/behaviour-baseline/gpui/linux`, and `linux-gui-check.sh` 122/122 with real input, 2026-11-11); on Windows 63 of 64 from its installer (2026-11-11; B36 missed only its click-to-first-request time once, held at 0.71 s on the second pass); owed on Windows: a whole `human-qa.py` run on an installer built after the review fixes of 2026-11-11; the empty list's prompt (inventory 54: "Nothing queued", its line, Add a download, what may be dropped) is built (2026-11-11, `table::empty`, the words both windows' from `rows::empty_words`), checked on macOS over the control port (its words, Add a download opening Add URL, gone with a row, "No download here matches the filter." for a search that finds nothing) and the macOS B25 picture drawn again; Linux: owed to the final Linux pass (`human-qa.py` with the Linux B25 picture drawn again by `--save-baseline` and looked at); Windows: its picture on the laptop. `human-qa.py` on the GPUI window on macOS, 2026-11-11: 65 of 66 rows, B25 failing for size alone on all three pictures (this Mac's display now centres the first window at 901 points and the dialogs at 520, 2 pixels off each baseline; the empty window's new picture was taken of a 900-point window, as the baselines are). View › Text size is built (B66, issue #32: the scale slider of 60/80/100/120%, 2026-11-11), checked on macOS over the control port; Linux: owed to the final Linux pass (`app-check.py` and `human-qa.py` B66 under Xvfb, `cargo check` of the Linux paths); Windows: owed on its laptop (a 150% display, the caption buttons and title bar at 120%); not checked anywhere: a real drag of the slider's thumb; the list scrolls sideways where its columns are wider than the window (2026-11-11: at 120% in a 760-point window, Last Try Date out of sight, then reached by a sideways scroll over the port, `ui.scroll {sideways}`; the wheel moves only the rows, a sideways swipe only the columns), checked on macOS; Linux: owed to the final Linux pass (the same under Xvfb) | Each slice's contract rows (`docs/migration/GPUI_MIGRATION_PLAN.md` Phase C) hold in GPUI on every system, run side by side with egui by `human-qa.py`; D3 for the new B25 pictures |
| G8  | GPUI migration, Phase D: platform integrations to adapt or redo for GPUI (`docs/migration/PLATFORM_GAP_ANALYSIS.md`) and packaging | PARTIAL: macOS's .dmg held 64/64 (2026-11-10); Windows' installer (`cargo packager -p dm-gpui --formats nsis`) held 63 of 64 installed (2026-11-11), with the tray, the login launch, the window put away, moved and pictured through Win32, the jump list, taskbar progress and High Contrast; Linux's .deb, AppImage and .rpm built from `dm-gpui` (2026-11-11: `/usr/bin/dm-desktop` and a hidden `dm-desktop.desktop` kept for earlier installs; the .deb installed by apt passes `app-check.py` and the Secret Service check; the AppImage answers as a native host), with the tray and its host (`dm_desktop_base::tray`, shared with the egui window), dialogs placed and moved on X11, links dropped on X11 and Wayland. The release profile's fat-LTO link of `dm-gpui` ran out of Docker's 7.6 GB here (the local check used thin LTO); CI's 16 GB runners are expected to hold it. Owed: the release workflow's packages from a draft on every system | The release workflow's packages, downloaded from a draft, hold 64/64 on macOS, Windows and Linux |
| G9  | GPUI migration, Phase E: parity report, the default switched, egui removed | PARTIAL: the GPUI window is the one built and shipped on macOS (2026-11-10, after its .dmg held 64/64), Windows (2026-11-11, after its installer held 63 of 64) and Linux (2026-11-11, after 64/64 and 122/122); egui, its fonts (Inter, Selawik, the Fluent icon font: the GPUI window uses the system's) and its vendored patches to be removed in a change of their own (the maintainer, 2026-11-11: "remove after Windows"), once Windows' whole run on the reviewed installer holds | `docs/migration/EGUI_PARITY_REPORT.md` row by row; the maintainer's D5; egui, its three vendored patches and kittest removed in a change of their own |
| G10 | A finished download cannot be dragged out of the GPUI window into a folder, a mail or a chat (it can from the egui window: `drag` on macOS and Windows, dm's own XDND on X11, winit's on Wayland; `docs/parity.md` "Dragging finished files out of the window"). Not a contract row | PARTIAL (2026-11-11): written for macOS, X11 and Wayland, not yet watched landing. A finished row (or the chosen rows, from one of them) is a GPUI drag (`dm-gpui` `dragout.rs`, `table.rs`) whose files GPUI hands to the system as the pointer leaves the window (`external_drag_payload`): AppKit's dragging session on macOS and the data device on Wayland, both GPUI's own (Wayland now offers a copy only), and on X11 an XDND source added to the vendored GPUI (`vendor/gpui-pre-linux` `x11/drag_out.rs`, the egui window's `xdnd.rs` moved there). No `drag` crate: the system's drag runs beside GPUI's loop, never inside an update. Built, clippy clean, `dragout.rs` unit tests pass; a row still takes a click and a double-click through the control port, as before. macOS: `drag-out-check.py` now drives `dm-gpui` (DM_BINARY for the egui window) but could not move the pointer here (the terminal is not allowed under Accessibility; `AXIsProcessTrusted` false, so the egui window failed it the same way): owed, by hand or with that permission. Linux: owed to the final Linux pass (`linux-drag-check.sh`, `linux-wayland-drag-check.sh` with DM_WINDOW=gpui; the X11 code is not compiled on this Mac). Windows: GPUI's Windows platform has no `start_external_drag`, so a row's drag stays inside the window there; owed to the Windows laptop (an OLE drag source of dm's own on `windows-sys`, as `drops.rs` builds a drop target, or the `drag` crate) | `drag-out-check.py` passes with dm-gpui on macOS; `linux-drag-check.sh` and `linux-wayland-drag-check.sh` pass with dm-gpui; a drag out on Windows |
| G11 | Options › General's "Use the Mica backdrop" (Windows 11) is the egui window's only: the GPUI window draws Fluent's opaque colours and offers no switch. Off by default, not a contract row | OPEN | The switch on Windows 11 (`chrome::mica_possible`), GPUI's `WindowBackgroundAppearance::MicaBackdrop` with the window's and side panel's colours see-through where it is on, its words read as they read now |
| 121 | An expired signed stream address is renewed: from the open page (`decide::Handed`, `Start::renews`) and by the browser's next hand-over of the same file (`Entry::fresh_link`, row 57). Tested offline only | OPEN | A real video site's CDN address that runs out mid-download, tab left open in Microsoft Edge, goes on in the same row |
| 163 | GitGuardian fails pull request #33 on two made-up test passwords (`s3cret`, `p4ss`) in commit 62baeca's history; nothing like them is in the tree, and gitleaks finds no leak | OPEN: the maintainer | In GitGuardian's dashboard, resolve incidents 39386940 and 39386941 as test credentials |
| 170 | Dailymotion: the panel on a public video lists no qualities, so nothing can be chosen (`video-sites-check.py` PUB-03, https://www.dailymotion.com/video/xbuzu66, signed out, Windows, Microsoft Edge, 2026-11-10). Not dm's reading: the check's picture of the page (`SITES_SHOT`) shows a "Join Dailymotion" sign-up box over the player for a new profile signed in nowhere, and the check's consent words took that box's own "Continue" for a consent button (it then said "Check that's the right email"). Behind it the player never asked for the video: DevTools on every frame and worker saw no metadata, playlist or media request in 60 s, and the extension's own listener none either, so the panel was on a 335×188 video that is not the page's. Set aside at the maintainer's word of 2026-11-10: Dailymotion does not let a visitor who is not signed in download ("ignore it for now") | BLOCKED: a sign-in | The check tells a sign-up box from a consent one (a box with an address or password field is a wall: BLOCKED), and `python scripts/video-sites-check.py --phases public-panel --panel-sites dailymotion` passes: the panel lists the video's qualities and a whole video with sound lands in dm's folder |
| 171 | Beside reference A, measured from the first request, dm ends 0.3 to 0.5 s behind on a server that keeps cutting connections and through a 15 s outage, though level or ahead from the moment each app is given the address (2026-11-10, `bench.mjs`, dm's home on C:, both saving to the hard disk D:: `flaky` 8.61 and 8.76 s against 8.31 and 8.24 from the first request, 8.63 and 8.78 against 8.82 and 8.76 from the address; `outage` 23.64 against 23.33, and 23.65 against 23.82). The difference is in dm's finish after the last byte, 0.37 to 0.56 s against 0.19 to 0.30, and in what it fetches twice, 1.8 and 2.3% against 0.8 to 1.1% | OPEN | `node bench/bench.mjs flaky outage`, a fresh download folder, then `node bench/compare.mjs`: `transferSeconds` better than reference A's floor (flaky 8.236 s, outage 23.312 s), as are `refetchedPct` (0.8% in both) and every other parameter in those two, every file whole |
| 172 | The `batch` case has no comparison yet. It ran to its end for the first time on 2026-11-10, once `bench.mjs` stopped blocking its own server while it handed reference A the files (`spawnSync`): dm fetched all 24 files of 2 MiB whole in 5.66 and 5.74 s, and reference A, its queue filled and started from the command line (`/d … /a`, then `/s`), finished 3 and 4 of the 24 within the 300 s allowed, which says more about how the harness starts its queue than about reference A | OPEN | `node bench/bench.mjs batch` gives reference A its files as a person would and both apps finish all 24 whole; that run of reference A is frozen in a baseline file of its own beside `bench/reference-a-baseline.json` (never in it), and `compare.mjs` says dm is better in every parameter |
| 175 | Memory: dm uses about four times reference A's floor while downloading and about twice at rest. `bench/compare.mjs` on the Windows laptop, dm's release build of 2026-11-10 (the egui window), on that day's side-by-side runs and again on 2026-11-11: peak working set 80 to 91 MiB in `fast`, `per_conn`, `flaky` and `outage` against reference A's 20.9 to 21.6, and private bytes 42 to 53 MiB against 10.6 to 11.6. At rest the working set was 65.7 and 77 MiB against 39.2, and private bytes 31.3 and 31.4 against 12.3. On macOS (Apple M1, GPUI release build, 2026-11-11) the working set is 108 MiB at rest and 119 to 136 MiB downloading; a CPU sample in `per_conn` shows no hot spot, the process mostly waiting | OPEN | `node bench/bench.mjs` with the shipped window's release build on Windows, macOS and Linux, then `node bench/compare.mjs`: `peakWorkingSetMB` and `peakPrivateMB` better in every scenario |
| 176 | CPU where the server holds the speed down: dm uses about twice reference A's floor. In `per_conn` it used 2,016 to 2,594 ms against 1,281, and in `flaky` 2,828 to 3,344 against 1,406 (the same runs as row 175). In `fast` and `outage` dm already uses less: 2,297 to 2,453 ms against 2,953, and 6,188 against 14,203 | OPEN | `node bench/compare.mjs`: `cpuMs` better in every scenario |
| 177 | dm's finish after the last byte is slower than reference A's in every scenario (`lastByteToDoneMs`, the same runs as row 175): `fast` 855 ms against 795, `per_conn` 381 and 542 against 159, `flaky` 500 and 605 against 71, `outage` 369 against 218. After the outage dm's next byte comes 70 ms after the server returns, where reference A had a request waiting (0 ms, `recoveryAfterOutageMs`). On macOS (GPUI, 2026-11-11) all eight connections asked again in the same instant every two seconds and the next byte came 1,966 ms late; each run of failing now asks in its own quarter-second (`retry::quick_nap`'s lane), and the outage run is 23.32 s against the 23.8 floor, recovery 232 ms. Recovery at 0 would take reference A's hundreds of requests a second, not copied (`retry::Pace`) In `per_conn` the bytes fetched twice tie at 0.1%, and a tie is not better | OPEN | `node bench/compare.mjs`: `lastByteToDoneMs`, `recoveryAfterOutageMs` and `refetchedPct` better in every scenario |
| 178 | YouTube's panel on a live page now and then missed its own steps, though dm's part held: in `human-qa.py` on macOS on 2026-11-11, two of four runs failed B33 to B35 and B59 (once the player did not start when the check pressed play; once the panel's first click handed the page to dm instead of listing the qualities). **macOS: fixed and proved, 2026-11-11** (docs/decisions.md, same date): a click no longer takes an answer of fewer than two lines as final while YouTube's player may still list its own (asked again every 0.5 s for up to 15 s), the background no longer cuts dm's reading short for a single stream, and `youtube-check.py` presses play as a person does (`press_play`). `youtube-check.py` passed 10 of 10 runs in a row on macOS (68 of 68 checks each, no page refused by YouTube); with dm's reading made slow, the old extension handed the page over 10.1 s after the click and the new one kept the list. Still owed: Windows saw the page come without its player data twice on the evening of 2026-11-11 (G7), not seen on macOS. Linux: owed to the final Linux pass (`scripts/linux-chromium-human.sh` with `youtube-check.py` ten runs in the Debian container, if its browser can reach youtube.com; there Chromium's autoplay policy holds the player, so `press_play` is used on every run). Windows: ten runs on the Windows laptop | OPEN | `youtube-check.py` passes ten runs in a row on Windows and Linux (macOS done) |
| T5  | dm is listed in Windows' Settings › Default apps for magnet links and `.torrent` files (`register.rs` `list_in_default_apps`, tested with a stand-in `reg`) but has not been seen there | OPEN | A person on Windows with an installed build sees dm there and chooses it for a magnet link |
| 180 | (optimization spec §3.1, §4.1; [audit](optimization-audit.md#31-the-render-path)) The main window's render does blocking and whole-list work on every frame. Each frame: about 12 deep clones of the 80-field `Settings` under its mutex (`Backend::settings`; 6 of them in the row list's processor, which GPUI calls 3 times a frame, `dm-gpui/src/table.rs:324-325`); 2 `block_on` calls into the engine (`dm-gpui/src/toolbar.rs:82`, the queues of Start Queue and Stop Queue); 2 `stat()` calls for each finished row on screen (`status_of` → `dm-shell/src/app/rules.rs:89`); about 12 passes over the chosen rows (`chosen()` once per toolbar button) and full scans in the bars and the status bar; and every label wrapped in a `canvas` for the control port's `ui.texts`, even with no port open (`dm-gpui/src/screen.rs:42-70`). The progress windows `block_on` and read the plan and ledger files at every wake (`dm-gpui/src/progress.rs:136`, `:151`). Download details scans every row and the disk at every render (`dm-gpui/src/details.rs:510`). Its cost is part of row 181's numbers | OPEN | A main-window frame with 10,000 rows (half finished) makes no `block_on`, no file-system call and at most one `Settings` read. A test proves it by counting them. What render needs is read once a wake and kept. The progress and details windows read the disk off the window's thread. The labels are recorded only while a control port is open, or measured to cost nothing. Then `ui-bench.py --rows 100,10000` and `stress-check.py window` are run again and are no worse than the audit's §9 anywhere, and `human-qa.py` holds 64 of 64 |
| 181 | (optimization spec §3.2; [audit](optimization-audit.md#32-progress-coalescing)) Each wake of the main window reads everything and draws everything again. A wake is a queue change (coalesced over 250 ms), or the 1 s clock while anything runs. It calls `Downloads::read` (`dm-gpui/src/list.rs:1600`). That read is 3 `block_on` snapshots, a name built for every row, and every row placed, filtered and sorted again. It also runs `SavedSizes::apply`, which costs (finished rows × all rows) (`dm-shell/src/app/snapshot.rs:49-52`; about 10⁸ comparisons at 10,000 finished rows). Then `cx.notify()` runs with no condition (`dm-gpui/src/list.rs:475`). So the title bar, toolbar, sidebar, bars, rows and status bar are built again, and so is every window observing `Downloads` (progress, Complete, drop target, Properties). `changes::take()` says whether anything changed, and nothing looks at its answer (`dm-gpui/src/list.rs:1601`). The wake channel is unbounded (`dm-gpui/src/main.rs:199`). Measured on macOS (Apple M1, release, 2026-11-11, with other work running on the computer; `ui-bench.py --rows 100,10000 --runs 3 --idle 30`): one scroll is answered in 0.03 s at 100 rows and 0.17 s at 10,000 rows, over the spec's 50 ms. Idle CPU is 0.2% and 1.4%. CPU with 20 downloads is 11.2% and 17.2%. `stress-check.py window` (5,060 rows, 60 downloads) answers in a median of 43 ms, worst 127 ms | OPEN | A read where k of 10,000 rows changed costs O(k) beyond pointer comparisons. `SavedSizes::apply` is linear, with a unit test of 10,000 finished rows held to a time bound. A wake that changed nothing on screen draws nothing (a test counts renders). The parts of the window that did not change are not built again (views of their own, or cached). The wake channel holds at most one pending wake of each kind. `ui-bench.py --rows 10000` (CPU with 20 downloads) and `stress-check.py window` (CPU, and answer times with 5,000 rows and 60 downloads) are each better than the audit's §9. A scroll at 10,000 rows is answered in under 50 ms (the spec's §9) |
| 182 | (optimization spec §3.3; [audit](optimization-audit.md#33-large-lists-10000-records)) The download list keys its rows by position, not by download. Each row's id is `.id(i)` (`dm-gpui/src/table.rs:341`). The click, double-click and menu handlers capture the position and look the row up only when they run (`dm-gpui/src/list.rs:1868`), so a re-sort or a new row between a frame and a click acts on another row. The scroll offset is kept in pixels with no anchor, so a row added above the view moves what is shown. Nothing tests fast scrolling while downloads update: `ui-bench.py` scrolls once, before its downloads start | OPEN | Rows are keyed by `dm_core::Id`, and the handlers carry the id. A test re-sorts the list between a frame and a click, and the click acts on the row that was clicked. A test holds the top visible row in place while rows are added or removed above it. `ui-bench.py`, or a `stress-check.py` phase, pages through 10,000 rows while 60 downloads update, and each page is answered no slower than one scroll is with nothing running (the audit's §9) |
| 183 | (optimization spec §5; [audit](optimization-audit.md#5-async-runtime-and-threads)) The window's thread waits on slow work in event handlers. `Backend` makes 52 `block_on` calls on it (`dm-shell/src/app/backend.rs:67-702`). The worst cases: Move/Rename to another disk copies the whole file while the window waits (`dm-gpui/src/rowdialogs.rs:426` → `dm-shell/src/app/backend.rs:602` → `dm-core/src/queue/queues.rs:120`). Pause, Resume or Retry on many rows makes one `block_on`, and possibly one queue write, per row (`dm-shell/src/app/commands.rs:605-609`). The folder and file choosers are synchronous `rfd` (`dm-gpui/src/services.rs:33-50`, 9 callers). Only Download details and Download links use GPUI's async prompt. A column or toolbar drag writes the settings file (`dm-shell/src/app/backend.rs:354`, `:371`). Export, Import and Delete file's trash are synchronous (`dm-gpui/src/actions.rs:196`, `dm-gpui/src/list.rs:1787`). While the clipboard is watched, it is read every second. | OPEN | Each of these runs off the window's thread, and its answer is sent back. A check in `stress-check.py` holds the control port's `ui.texts` to an answer in under 100 ms throughout two cases: a 1 GB file moved to another volume (a disk image on macOS), and Pause on 10,000 chosen rows. The choosers keep the system's own panels and stop holding the thread (`rfd`'s async dialogs, or GPUI's prompt where it has a panel on that system). `human-qa.py` holds 64 of 64 |
| 184 | (optimization spec §4.2, §5.2; [audit](optimization-audit.md#42-locks)) Engine locks are held across slow work, and the order they are taken in is written nowhere. tokio guards alive across I/O: `store`, across the queue file's write. Edition 2021 keeps an `if let` temporary alive to the end of its block (`dm-core/src/queue/persist.rs:212`, `:346`). `slots`, across `remove_file` (`dm-core/src/queue/runner.rs:478-512`) and across a synchronous `create_dir_all` (`runner.rs:612-624` → `dm-core/src/queue/mod.rs:791`). A job's `segments`, across `spawn_blocking` (`dm-core/src/download/ranged.rs:454-494`). Two global locks, each across a network fetch: the JavaScript engine's (`dm-core/src/tools.rs:377`) and a proxy script's (`dm-core/src/pac.rs:341`). `queue.note()`, which calls the change callback, runs under `slots` (`runner.rs:500`) and under `saving`. `Inner` (`dm-core/src/queue/mod.rs:476-557`) does not say in what order its locks are taken. A few `std::fs` calls run on the async workers (`persist.rs:24-31`, `:336`) | OPEN | No queue lock is alive across I/O, a network wait or `spawn_blocking`. Each is reworked, or kept with a comment saying why it must be. The lock order is written above `Inner`, and `note()` runs after the guards are dropped. A test holds one queue write for 2 s: `list_shared` still answers in under 10 ms, and other downloads' progress keeps moving |
| 185 | (optimization spec §5, §5.2; [audit](optimization-audit.md#52-download-manager-safeguards)) CPU and disk work has no bounded pool and no limit of its own, and Cancel cannot stop it. The runtime is tokio's default (`dm-desktop-base/src/boot.rs:101`): one worker per core, and up to 512 blocking threads shared by every file write, assembly and copy. A digest runs on the async workers between its reads (`dm-core/src/checksum.rs:109-127`). Each ZIP extraction gets an OS thread of its own (`dm-shell/src/app/files.rs:154`), and so does each JavaScript evaluation (`dm-core/src/javascript.rs:434`). Assemblies, digests and extractions have no limit apart from the network's connections. After Cancel, an assembly copy (`dm-core/src/download/assemble.rs:43`) or a digest still runs to its end. A slow disk should slow the network read and not grow memory (the write is awaited before the next read, with 16 KiB per connection), but that is by design only, and untested | OPEN | Digests, extraction, assembly and other CPU work run on a bounded pool whose size is set apart from the network's limits. The blocking pool is sized on purpose. Cancel stops a digest or an assembly of a 2 GB file within 1 s (a test). A test with the parts' writes slowed to 1 MB/s, under 8 connections offering 50 MB/s, shows resident memory flat (within 10 MB) and the download slowed. `node bench/bench.mjs fast per_conn big40`, then `compare.mjs`: no worse |
| 186 | (optimization spec §5, §8 scenario 12, §9; [audit](optimization-audit.md#5-async-runtime-and-threads)) Quitting while downloads run is not a graceful, tested shutdown. Exit, a logout and SIGTERM all flush the queue (`dm-gpui/src/list/questions.rs:314` → `dm-shell/src/app/backend.rs:226`). The downloads are neither stopped nor awaited: their tasks go down with the runtime, mid-write. The next start puts them back as Queued (`dm-core/src/queue/persist.rs:105`) and trusts each range's ledger. Nothing tests a quit in the middle of transfers, or how long it takes | OPEN | On Exit or SIGTERM with 20 downloads transferring, each one is stopped as a pause stops it (buffers flushed, ledgers written). Its task is awaited within a deadline of 2 s, and dm exits 0 within that time. A test measures the exit time (a `stress-check.py` phase, or one in `dm-core`). After a restart, the rows come back as they do today, every file finishes byte-exact, and no connection fetches more than one buffer (16 KiB) twice |
| 187 | (optimization spec §8; [audit](optimization-audit.md#8-profiling-and-benchmarking)) Six of the spec's twelve benchmark scenarios are missing or partial: Cold startup: `ui-bench.py` never starts with the system's file cache emptied. Scrolling a large list: one 10-line scroll. Sorting and filtering a large list: `ui-bench.py`'s Find waits for a Find next button that only the egui window has, so with the GPUI window it times out (50 s, 2026-11-11), and neither the search box's filter nor a sort is timed. Hundreds of progress updates at once: `stress-check.py window` runs 60. High throughput with the window in use: the window's answer times and `bench fast` are never measured together. A removable drive pulled mid-download: by hand only (ENG-07b). Two of the spec's targets (§9) have no measurement at all: the frame time (16.7 ms at 60 Hz) and the engine's throughput with the window against without it. Shutdown with downloads running is row 186 | OPEN | `ui-bench.py` or `stress-check.py` measures each scenario over three runs, with the median and spread, and the audit's §9 has the numbers: Start after the file cache is emptied (`purge` on macOS, `drop_caches` on Linux, a restart noted on Windows). A scroll through 10,000 rows. A sort by each column, and the search box's filter, at 10,000 rows (Find's step made to work with the GPUI window). 300 downloads updating beside 10,000 rows. The window's answer times during a download at full speed. A disk image detached mid-download (`hdiutil detach -force` on macOS, a loop device on Linux): the download ends paused with its reason, and dm still answers. Frame times at 10,000 rows, scrolling with 60 downloads running, taken from Instruments, or from GPUI's own timing where it has one. `bench fast` and `big40` run with the window and with `--headless`, their transfer seconds equal within the runs' spread. |
| 188 | (optimization spec §7, §8, §9, §12 phase 5; [audit](optimization-audit.md#8-profiling-and-benchmarking)) Measurements do not record what produced them, and only `bench/` has a baseline. `meta()` in `bench/bench.mjs:386-395` records the OS, CPU, memory, and the binary's size and date. It does not record the commit, rustc, the target, the profile and features, the GPU or the display scale. `ui-bench.py` prints a table with no stored baseline and nothing to compare it with. `memory-check.py`'s budget is checked only when someone runs it by hand. The release binary's size (19,868,544 bytes on macOS, 2026-11-11) is tracked nowhere. CI's toolchain floats (`stable`) | OPEN | Every results file of `bench.mjs` (its `meta()` only: never the server, the scenarios or the judging), `ui-bench.py` and `memory-check.py` records the commit, `rustc -V`, the target, the profile, the GPU and display scale, and the dataset's size. `ui-bench.py` writes JSON and compares it with a stored baseline for each system, printing the median, the spread and the change. CI records the release binary's size on each system, and fails only on a jump of more than 10% |
| 189 | (optimization spec §7; [audit](optimization-audit.md#7-cargo-build-and-release)) The release profile was measured for the egui window and never again for `dm-gpui`. `desktop/Cargo.toml:16-45` has `opt-level = "z"`, with `dm-core` and the CPU-bound crates at 3; that list still names `egui` and `epaint`, not GPUI's renderer. It also has fat LTO, `codegen-units = 1` and `strip = true`. Thin LTO has never been measured; fat LTO's link ran out of Docker's 7.6 GB (G8). No debug symbols are kept for a release, so a backtrace from a person's build cannot be read. `panic = "abort"` was turned down for the egui tray's `catch_unwind`, which `dm-gpui` does not have. There is no profiling profile | OPEN | Measured on `dm-gpui` (macOS first, then Linux and Windows): fat against thin LTO, `opt-level` for GPUI's crates, and `panic = "abort"`. dm has no crash reporting, so abort is weighed against tokio's per-task panic isolation. Each is measured for binary size, link time and peak memory, cold and warm start, idle memory, and CPU in `bench` `fast` and `per_conn`. The choice and its numbers go in `docs/decisions.md`. A release build's symbols are kept apart (dSYM, PDB, `.debug`) and attached to the release. A `[profile.profiling]` (release with debug info) exists for flamegraphs |
| 190 | (optimization spec §8, §10; [audit](optimization-audit.md#8-profiling-and-benchmarking)) dm lacks some of the spec's tools: `cargo deny`: there is no `deny.toml`. BUGLOG.md:461 left it to the maintainer, whose spec now asks for it. Profiling: `docs/testing.md` says nothing of `cargo flamegraph` or samply, Instruments, WPR/WPA or perf. `criterion`: dm has no benches; the engine's timing tests only pass or fail. `cargo bloat`: the binary's size by crate has never been recorded. Linux arm64 ships, and its packages are smoke-tested, but its tests are never run. | OPEN | A `deny.toml`: licences compatible with GPL-3.0-or-later, crates.io as the only source, duplicates warned, advisories as in `audit.toml`. `cargo deny check` runs in CI. A Profiling section in `docs/testing.md`, with the command for each system. `criterion` benches for at least range planning, a 10,000-row read of the application (`Session::read`, `SavedSizes`, `places`) and naming. They are run by hand and never gate CI. `cargo bloat --release -p dm-gpui --crates` recorded with row 189. CI's test job also runs on `ubuntu-22.04-arm`. |
| 191 | (optimization spec §6; [audit](optimization-audit.md#6-cross-platform)) Windows: the window has not been tried across monitors of different scale, nor has dm been used in a path longer than 260 characters. GPUI declares PerMonitorV2 in the manifest it links, and handles `WM_DPICHANGED`. Still, nobody has moved the window between a 100% display and a 150% one. That manifest has no `longPathAware`. Rust's `std::fs` reaches long paths by itself, but Open, Show in folder, the trash and the installer go through the shell. Unicode names are tested (`shared/naming-cases.json`). Owed to the Windows laptop | OPEN | On Windows with two displays, at 100% and 150%: the main window and a dialog are dragged from one display to the other. Each redraws at the new scale, clicks land where things show, and there is a picture of each. `human-qa.py` passes whole with the window on the second display. A download completes into a folder whose full path is 300 characters long, and into one with CJK and emoji in its name, and Open, Show in folder and Delete file each work on it. A `dm-core` test of a 300-character path runs on Windows CI |
| 192 | (optimization spec §6; [audit](optimization-audit.md#6-cross-platform)) Linux: the GPUI window is unchecked across desktops, GPUs and disks. None of these has been run: GNOME (mutter) and KDE Plasma (KWin) on Wayland. G6 has X11, and Wayland only for its checks 2 and 4. Niri and Hyprland: only unit tests of their minimize rule (`dm-desktop-base/src/tray.rs:223`). Real NVIDIA, AMD and Intel drivers: only Mesa in Docker. The portal file chooser, and its zenity fallback. The tray and notifications on each desktop. A full disk: `stress-protocols.py fulldisk` runs on macOS only. A mount removed mid-download. Owed to the final Linux pass | OPEN | On real Linux computers: `human-qa.py` passes on GNOME Wayland, Plasma Wayland, Niri and Hyprland. On each, the tray (or its fallback), a notification and the folder chooser are checked, the chooser both with the portal and with zenity and no portal. The window draws correctly on at least two of NVIDIA, AMD and Intel. `fulldisk` runs on a small tmpfs or loop device, and a mount is pulled mid-download. Both end in a paused row with its reason, and dm still answers. |
| 193 | Links on the clipboard's Save to (B67, issue #32: "choose or create a new folder") is built and checked on macOS only: the shared Save to (`dm-desktop-base` `save_to.rs`, `dm-gpui` `savefolder.rs`) in Links on the clipboard and Download links, a typed folder made when the links are added, one that cannot be said. Checked on macOS over the control port (2026-11-11): `app-check.py` (109 ok, 0 failed; its new clipboard checks all hold) and screenshots at 60, 100 and 120%. Owed on macOS too: a whole `human-qa.py` run with B67 (written, not run: the extension is not built in this worktree, the disk had 2 to 5 GB free with the other track building, and the benchmark was running). Not checked anywhere: the system's folder panel itself, opened by Browse… (it waits for a person; New Folder in it is read from GPUI's source, `NSOpenPanel` `canCreateDirectories`). Linux: owed to the final Linux pass (`app-check.py` and `human-qa.py` B67 under Xvfb with xclip; the portal's or GTK's folder chooser offering a new folder; `cargo check` of the Linux paths, none of which is new). Windows: owed on its laptop (the folder picker's New folder; `human-qa.py` B67) | PARTIAL | `human-qa.py` B67 holds on Linux and Windows, and Browse…'s panel is seen to make a folder on each system |

## GPUI migration: picking it up (rows G1 to G9)

Written so that a new session can start from here alone. The maintainer asked on 2026-11-09
for dm's window to move from egui to GPUI, following a specification they attached (its
substance is in `docs/migration/GPUI_MIGRATION_PLAN.md`). The maintainer also said, twice:
**take the recommended option at every decision and keep going; do not stop to ask.** Record
each decision so taken in `docs/decisions.md` ("at the maintainer's word of 2026-11-09").

### Where and how

- **Branch `gpui-migration`**, not `main`: `git switch gpui-migration`. `main` receives it only
  by fast-forward after the full gate below, at the end of a phase.
- **Read first**, in this order: `docs/migration/GPUI_MIGRATION_PLAN.md` (phases, the decisions
  D1 to D5 and the recommended option for each), `docs/migration/GPUI_UI_ARCHITECTURE.md` (the
  design every step builds towards: crates, tick, state, views, `WindowProbe`),
  `docs/migration/GPUI_TEST_PLAN.md` (the gate and what each row adds to it),
  `docs/migration/CURRENT_UI_ARCHITECTURE.md`
  (sections 3.3 the real tick order, 4.2 the rules still in `DmApp`, 5.2 the control port,
  8 what a GPUI window must provide, 9 docs against code), then for the step at hand
  `EGUI_FEATURE_INVENTORY.md`, `PLATFORM_GAP_ANALYSIS.md`, `GPUI_DEPENDENCY_DECISION.md`, and
  `docs/architecture/MODULE_BOUNDARIES.md` (what may depend on what; `cargo xtask layers`
  enforces it from `desktop/xtask/src/main.rs`, `fn layers`, the `GUI` and `SOCKETS` lists).
- **Rules that still hold** (CLAUDE.md): no behaviour in `docs/behaviour-contract.md` changes
  (Phase A moves code, it changes nothing a person sees); name no other download manager
  ("reference A", "reference B"); no GPUI dependency in any crate before G6.
- **The gate, after every step that touches `desktop/`, `browser/` or `extension/`**, before
  its commit (all four, in this order; report every row they print):
  1. `cd desktop && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
  2. `sh scripts/windows-check.sh clippy` (Docker running; the Windows-only code G2 and G3 move
     is never built on the Mac otherwise; a plain `cargo check --target x86_64-pc-windows-msvc`
     stops at `ring`'s C, which needs the Windows headers the container carries)
  3. `cd desktop && cargo build -p dm-desktop && cd .. && sh scripts/package.sh && python3 scripts/human-qa.py`
     (macOS, a visible window, about 20 minutes): 64 of 64
  4. `sh scripts/linux-human-qa.sh` (Docker running; builds the checkout in Debian): 64 of 64
  Record each row's progress in `docs/migration/EGUI_PARITY_REPORT.md` once GPUI slices start.
  A row that fails once and passes on a second whole run (B40 did on 2026-11-09) is reported
  as such; a row that fails twice is a defect to fix before the commit.
- **One commit per step** (or per module moved, in G2 and G3), message naming the row.
  When a row closes, delete it here and say what was done in `CHANGELOG.md` (Unreleased,
  as "Inside, no change to what a person sees: ...") and in the plan's status line.

### State of each row (2026-11-09)

- **G1, docs and code agree: done** (2026-11-10). `refresh()` calls `changes::take()`, `logic()`
  reads `changes::pending()`; the architecture docs give the tick as the code runs it; stale
  comments fixed. Decided, not changed: `DmApp::logic` keeps running from both `Window::logic`
  and `DmApp::ui` (the window tests drive `DmApp::ui` alone).
- **G2, a toolkit-free crate: done** (2026-11-10). `desktop/crates/dm-desktop-base` holds
  what the desktop program does that is not drawing (`MODULE_BOUNDARIES.md`), with no window
  crate (`cargo xtask layers`); dm-desktop re-exports each module at its old path. `Desktop`
  takes the clipboard, file choosers and non-Windows notifications from the window through
  `desktop::WindowServices` (`docs/decisions.md`, 2026-11-10). `--help` and MCP's
  `initialize`/`tools/list` byte for byte as before; `launch-latency.py` unchanged;
  `handover-check.py` 67 of 68 (LibreOffice, row 173, failing before G2 too); both QA runs 64/64.
- **G3, rules into `dm_shell::app`: done** (2026-11-10). Moved, each with a dm-shell test:
  `handoff::Cancels`, `hints::Hint`, `commands::remembered_limit` and `when_done_command`,
  `session::new_download_asked`, `WhenDone::possible`, `schedule::spent_once_put_away`,
  `names::PendingRenames`, `snapshot::SavedSizes`, `questions::Questions` (B39 to B41),
  `search::due_now`/`by_site_folder`/`seen_saved`, `update::weekly`, `control_api::ScriptGuard`,
  `dialup::hang_up_later`, `Session::add`, `backend::Undo`, the links dialog's rules
  (`links::looks_like_page` and the rest), `names::category_folder`, `rules::replacing`. Left in
  the window by decision (`docs/decisions.md`, 2026-11-10): `apply_settings`' words and view
  updates, the sounds and notifications' words, dialogs' drafts. Both QA runs 64/64 at 0b0a699.
- **G4, `ui.*` as a contract: done** (2026-11-10). `dm_desktop_base::probe`: typed requests and
  answers for every `ui.*` method and `commands.list`/`command.run`, the `WindowProbe` trait, and
  `probe::input`/`probe::view` (the two doors; a batch reaches `view` only); nine tests fix the
  JSON. The egui window implements it (`app/human.rs`'s `Probe`); `ui.screenshot` stays in
  `answer_control` (its answer comes when the frame's pixels do). `human-qa.py` unchanged, both QA
  runs 64/64 at 7cf34db.
- **G5, egui's own costs: done** (2026-11-10). `scripts/ui-bench.py`; egui's numbers at 100,
  1,000 and 10,000 rows are in `GPUI_TEST_PLAN.md` section 4 (release build, M1, macOS 27).
  Linux and Windows numbers come with G6, measured the same way beside GPUI's.
- **G6, the GPUI shell: in progress.** `desktop/crates/dm-gpui` (binary `dm-gpui`) on the pins
  opens its main window on macOS, type-checks for Windows and builds on Linux (2026-11-10;
  `GPUI_TEST_PLAN.md` section 5's findings). Check 1 (screen readers in a dialog's own window,
  and 50 windows opened and closed) passes on macOS; Windows (NVDA, a Windows computer) and
  Linux (Orca over AT-SPI, `linux-gui-check.sh`'s image) are to run. Since: both windows start
  dm through `dm_desktop_base::boot` (one engine); the shell reads the queue once per wake from one
  foreground task (a burst of changes coalesced over 250 ms; a clock every second while something
  runs, else 30 s), draws the rows with `uniform_list` (only those on screen laid out) in the
  system's own light or dark colours and font, and quits on SIGTERM as Exit does. Seen with
  10,000 rows; it answers the control port (`ui.texts`, `ui.scroll`, `ui.inspect`, the
  application's methods; the rest refused by name), so `ui-bench.py` measures it. **Owed:**
  `linux-human-qa.sh` for 43a565d (the shared startup; macOS 64/64): the run of 2026-11-10 failed
  only for a full Docker disk ("0 bytes free"), and the maintainer deferred Linux QA for now; it
  runs before anything merges into `main`. Since: the shell's release numbers are beside egui's
  (`GPUI_TEST_PLAN.md` section 4: at 10,000 rows a third of egui's CPU with 20 downloads, 28 % less
  footprint, an 11.6 MB binary against 18.0 MB); dm's menus are in macOS's menu bar, built from the
  command registry, each command's keys bound (⌘Q quits through the menu's action); the words on
  screen are recorded where each frame lays them out, and `ui.click`, `ui.scroll` and `ui.key` go
  through GPUI's own input path, so `human-qa.py` can drive the GPUI window. Phase C slice 1 under
  way: the toolbar in a unified title bar (macOS's traffic lights beside it, a picture with its word
  on each button, the person's order and hiding from `commands::toolbar_shown`, moved out of the egui
  crate; the bar's space drags the window, a double-click on it does the system's setting), and the
  side panel as a macOS source list (places, their counts and the filter's matching moved out of the
  egui crate to `dm_shell::app::places`, with `Place::from_word` for `ui.place`), and the status
  bar (its counts from `rules::tally`, moved out of the egui crate with `fetched_all` and
  `waits_for_its_queue`; the egui window's own sentences). `human-qa.py` 64/64 on macOS for the egui
  window after these moves (f02e438). The file-kind pictures in rows (B45: told apart by shape, each
  in its kind's colour, the row's height unchanged; seen with one file of each kind). Next: slice 1's
  rows checked through `human-qa.py` with `DM_BINARY` naming dm-gpui (the GPUI window still lacks
  Add URL and the progress windows B63 needs: slices 4 and 6). Slice 2 under way: the table (a row's
  words and the columns moved to `dm_desktop_base::rows` and `columns`; the GPUI table shows the
  person's columns with the egui window's headings and words, sorts by a heading's click, cuts long
  words with an ellipsis, a progress bar under a running row's status) and its selection (click,
  ⌘-click, ⇧-click, ↑ and ↓; chosen rows in the selection colour; `ui.select`, `ui.inspect`).
  Slice 3 under way: every command the application carries out by itself (`commands::verb`: Resume,
  Stop, Download again, the queue moves, Resume all, Stop all, Delete completed, Start and Stop
  Queue, When done) runs from the toolbar, the menu bar, its keys and `command.run`, enabled as
  `commands::applies` says for the rows chosen; and the row menu (right-click or Control-click, the
  egui row menu's items and words, drawn as a macOS context menu); Copy link and Copy details through
  GPUI's clipboard (the system pasteboard), said in the status bar as the egui window says them.
  Questions are asked as macOS sheets (`question.rs`; `docs/decisions.md` 2026-11-10: B42 for the
  GPUI window's questions changes at D5), with dm's words (`dm_desktop_base::asks`, the Remove
  question's rules moved there from the egui dialog); Remove and Delete file ask, finished rows leave
  with Undo; every window's words are recorded for the port, so `ui.texts` and `ui.click` reach a
  sheet as they reach egui's dialogs. Slice 4 under way: Add URL is a window of its own (B01,
  B42): the address (a link on the clipboard put in and selected), Recent with Clear history, the
  sign-in, the warning line; what OK does is dm's for both windows (`dm_desktop_base::add_url`).
  Download details is a window of its own (B02 for a plain file): Category, Save to folder with
  Browse… (the system's folder panel), Save as (the server's name, once asked), Size, Note, Delete
  file after, the name's clash (numbered, or Replace once whole), Start now and Add for later to a
  queue, the row begun while it asks where Options says so; what it offers and adds moved to
  `dm_desktop_base::file_info` for both windows. "Already in the list" is a sheet with its four
  answers and Remember my choice (`add_url::carry`, both windows). Pickers are pop-up buttons
  opening the system's own menu (gpui-component's `NativeMenu`, an `NSMenu` on macOS), and a
  dialog's window is as tall as what it holds (`fit.rs`). The port types (`ui.type`) and says
  where the GPUI windows are (`ui.windows`). Found and fixed on the way: the main window's wake
  loop ended for good once a dialog that read the main view closed (GPUI's
  `WeakEntity::update_in` runs in whichever window last drew the view); it updates in its own
  window now. The questions a download raises (B39 to B41) are sheets, one at a time, with dm's
  words for both windows (`dm_desktop_base::asks`); running them found B39 naming a query
  address's file by the list's label (`127.0.0.1/dl?id=7`, the contract says `dl`) and asking
  about a name the server never sent, fixed for both windows (`names::address_file_name`). Quit
  is the application's (⌘Q from a dialog in front too, as on every Mac), and Exit closes the
  dialogs first: macOS held its terminate while a sheet asked, so a logout or `kill` waited on an
  unanswered question. The dialogs, sheets and main window take AppKit's measures, taken from the
  system's own dialog (`native.rs`: 13 and 11-point text, 24-point controls, 20-point margins,
  forms with a label column, the default button rightmost); a question is asked on dm's own
  window once it is in front (it went with Add URL as Add URL closed, and was never shown while
  dm was in the background). `human-qa.py` 64/64 on macOS for the egui window at 1711002.
  "Exit dm?" (Exit while downloads run: its words follow the list while it is open, its
  buttons never move) and "Start outside the schedule?" (Resume or Start now on downloads the
  schedule or the download limit holds) are sheets, with dm's words for both windows
  (`asks::exit`, `asks::outside`, `dm_desktop_base::held`). The Playlist window: a YouTube
  playlist read by dm, each video ticked, Select all, one Video quality (`file_info::playlist_adds`,
  both windows). Slice 4 is done in the GPUI window: the browser's hand-offs reach it (it had no
  `--native-host` at all: every way dm runs with no window, the host, the warm page reader,
  `--version`, `--help`, the command-line client, MCP, `--register`, a typo, now
  `dm_desktop_base::modes` for both windows, and macOS's "open with" Apple events
  `dm_desktop_base::opened`); Download details for a hand-off (the page, the sign-in, the panel's
  quality and Part, "From …", "Always save here without asking", a site's second cancel offering
  to leave it to the browser), a video's Quality chips, Part, Shape and GIF, a ZIP's Contents with
  Take out (B44), a page that lists several videos opening Playlist; "Download links" for a page's
  links (`dm_desktop_base::links_import`, both windows); and the tick each wake (`tick.rs`):
  `--new-download` and `--when-done` from the command line, notifications and sounds for
  finished and failed downloads (the window's own notifier, `notify.rs`), an address handed
  over again said, the schedules' hooks, When done with its countdown as a sheet (Cancel the
  default), "Delete file after", the watched folder, the big download hint, problems above the
  list with Dismiss. The port chooses in a pop-up as a person does (a click on the pop-up holds
  its choices instead of opening the system's modal menu; a click on a choice's words picks it),
  and reads only the words in view, so `human-qa.py` can drive the GPUI window. The window's top bar holds the search
  box, as a code editor's title bar does (the maintainer's word, "take reference from vscode"):
  the list narrowed as one types, ⌘F, Find next and previous, Escape; the toolbar a row under it.
  Slice 5 under way: the Scheduler (B49, B62, B65: the queues at its left with Start now and Stop,
  each queue's hours or the Main queue's, weekly or once, When done, tries again, a program to
  run; `dm_desktop_base::scheduler::SchedulerDraft`, both windows), the Speed limiter (its
  presets, on and off, the download limit, slower hours), Start Queue and Stop Queue saying what
  they did, and their queue lists in Downloads' menu and beside the toolbar's buttons (B64).
  The Grabber (B60, B61: a site's pages searched for files, its templates, other sites, the
  site's folders kept, sizes and paths, the files found ticked and added for later) and its
  saved searches, run by themselves on the tick (`dm_desktop_base::grabber`, both windows).
  Slice 5 is done but B52, which comes with the tray (slice 8). Slice 6 under way: each
  download's progress window (B53, B54, B63: its status and next step, size, speed, time left,
  whether it can resume, a bar of its pieces, its own limit, Connections with each piece's bar
  and speed, a torrent's swarm; opened on Start now where Options says, for a download started
  elsewhere, and by File › Show progress; closed as it finishes), "Download complete" (each
  file's Open, Open with…, Open folder; "Don't show this window again"), and "Cancel this
  download?" as a sheet; `dm_desktop_base::progress` for both windows. The GPUI window's
  services are the system's own (the clipboard; the open and save panels). The file and list
  commands: Open, Open with…, Preview, Show in Finder, Extract here and Extract to…, Select all,
  Import (asked first where some of a list is listed already) and Export, Help, the categories
  and toolbar shown or hidden, and Sort by (`actions.rs`). The row dialogs: Update download
  link (a fresh address, or "Get it from the browser" with the bar above the list until the
  next download clicked there is taken; offered by itself when a link runs out part-way,
  where Options says), Move to queue, Move/Rename and File properties (status and next step,
  size, category, where it is saved, the page it came from, address, sign-in and note, a
  torrent's files and their priorities, a finished file's SHA-256 and Check against, a
  finished ZIP's Contents: B44's Properties half), each a window of its own, their OK
  `dm_desktop_base::row_edits` for both windows; a double-click on a row (B46). Add numbered
  series (every address listed before anything is fetched), Links on the clipboard (one
  already listed offered unticked), About dm with Check for updates, What's new and Download
  the update, the update offered above the list once verified (Install now, Later), the
  weekly check, and Help's topics, Tip of the day, home page, support and Tell a friend
  (`dm_desktop_base::project`, both windows). A control client's click a moment ago runs no
  file and installs no update (the script guard, gaps row 159 f). Slice 7 is done: Options, its
  six pages (B43, B51, B56, B57) with the egui window's words in macOS's settings layout, OK and
  Restore defaults on `dm_desktop_base::options` for both windows, the look it asks for, and the
  clipboard watched as it says. Slice 8 under way: the tray (B52; the status item, its menu and
  tooltip), the close button as Options says with "Keep running in the tray?", the window back
  from the tray, the Dock, a second launch and the browser, a login launch into the tray, File ›
  Close (⌘W), and every dialog's close button doing what its Cancel does (B42). The bars above
  the list the egui window has (a download paused for want of room, downloads that did not
  finish, the way to add dm to a browser) and a new install's "Add dm to your browser". The
  control port answers every method in the GPUI window: `ui.inspect` with the dialogs open, the
  status line and problems, `ui.texts` with what fields hold (not a password's),
  `ui.screenshot` (the system's own picture of the window) and `ui.move_window`. Links dragged
  from a browser and files dropped on the window are taken (`drops.rs`), and the floating drop
  target (View › Drop target, the last of the menus' commands) is a window of its own
  (`droptarget.rs`). `human-qa.py` on the GPUI window: 63 of 64 on its second run (B48 then failed
  only because Escape did not close the progress window it found open; put right), and B25's
  pictures saved as the GPUI window's own baseline (`docs/behaviour-baseline/gpui/macos/`, D3,
  looked at). Since: macOS's Edit and Window menus; Downloads › When done carried out (it was
  offered and did nothing) and the menus' ticks (When done, the speed limiter, View's toggles) and
  "…" as the egui window's; `control.port` forgotten as the window quits; and `--headless` in the
  GPUI binary (`dm_desktop_base::headless`, no GPUI at all, so it needs no display):
  `app-check.py` 98 of 98 and `smoke.py` pass with dm-gpui. View's Columns, Customize toolbar
  (its three styles drawn), Language and Sort by as menu items, ticked as they stand; a column
  moved by dragging its heading, a toolbar button by dragging it onto another; Resume says
  Retry or Start now for the rows chosen, in the File menu and the row menu, as the egui
  window's does (`rows::resume_word`). The main window comes back where and as large as it was
  left (`frame.rs`); on Linux as large and zoomed as it was, where the desktop places it. Synced with `main` at a6faac1 (the engine's speed work, the extension's
  background and popup in Rust); `human-qa.py` 64 of 64 on the GPUI window on that tree.
  The release bundle from its .dmg: `smoke.py` and `human-qa.py` 64 of 64. The menus' titles in the
  top bar beside the search box, on every system (the maintainer's words); only the system's fonts.
  Windows and Linux coded here (`syswindow.rs`, the title bar, the tray's icon, window names): Linux's
  GPUI binary passes `smoke.py` and `app-check.py` under Xvfb; Windows builds and lints, and on
  2026-11-11 was run and checked on a Windows computer (the bullet "Windows" below). Each system's own colours (Fluent on Windows, libadwaita on Linux); a
  browser's link dropped on the window on Windows through the OLE target both windows now share.
  **Still owed in G7:** (View › Text size built 2026-11-11, `zoom.rs`, B66). Then
  (`changes::subscribe`
  waking one foreground task), `WindowProbe` for what it draws, a dialog window, then the
  checks one by one, check 1 (screen readers) first. As planned: Recommended pin (D2):
  `gpui = { package = "gpui-pre", version = "=0.3.12" }`,
  `gpui_platform = { package = "gpui-pre-platform", version = "=0.3.12" }` (macOS features
  `font-kit`, `runtime_shaders`; Linux `wayland`, `x11`; Windows none),
  `gpui-component = "=0.9.1"`; fallback `gpui-ce` 0.3.0 if the pair will not build. A new
  crate `desktop/crates/dm-gpui` (binary `dm-gpui`), depending on dm-core, dm-shell and
  dm-desktop-base only; add it to `cargo xtask layers` as a window crate. It opens the main
  window and one dialog window, runs the tick once per wake from one foreground waker
  (`changes::subscribe`), keeps the tokio runtime for the engine, and implements `WindowProbe`
  for what it draws. Judge it on `GPUI_MIGRATION_PLAN.md` Phase B's seven checks, each pass or
  fail per system, written to `GPUI_TEST_PLAN.md`. D1 (go): recommended go if check 1
  (screen readers reach OK and Cancel in the dialog's own window on macOS, Windows and Linux)
  passes, or can be made to pass by a patch to GPUI kept in `desktop/vendor/` as eframe's is;
  no-go otherwise, and then the plan stops at Phase A, which stands on its own.
- **macOS first, every step (maintainer, 2026-11-10).** "First migrate on macos, all steps, then
  we will move for linux. Windows part will be handled on windows so no need to do here. Focus on
  macos and coding part here." So: Phases B to E are carried out and proved on macOS here (taken
  as D1 go for macOS, with D2 the recommended pin, `gpui-pre` 0.3.12); Linux follows once macOS is
  done; Windows (its builds, runs and checks) is done on a Windows computer, not here. The code
  stays cross-platform. Owed before anything merges into `main`: Windows' whole run on the
  reviewed installer (G7); the Linux runs held on 2026-11-11.
- **Windows (maintainer, 2026-11-11: "complete the windows part"): done on a Windows computer.**
  The GPUI window builds there (`dm-gpui/build.rs`: icon and version; GPUI's own manifest), runs,
  and was checked against the contract: `human-qa.py` on the debug build (63 of 64 twice, B36's
  click timing), then from the release installer (`cargo packager -p dm-gpui --formats nsis`,
  63 of 64: B36's click timing, 1.76 s on its first pass, 0.71 s on its second), with B25's pictures drawn for Windows
  (`docs/behaviour-baseline/gpui/windows/`, D3, looked at). Phase B on Windows: check 1 pass
  (`windows-a11y-check.py` 5 of 5, after 50 windows opened and closed too), 3 and 4 pass, 2 by
  reading (WARP; CI's GPU-less runner is the run), 5 drop-in coded and drag-out missing (G10),
  6 by hand, 7 measured (`GPUI_TEST_PLAN.md` section 5). Put right on the way: the port's
  window titles and frames (GPUI's Windows windows say neither), pictures without the invisible
  resizing borders, the caption glyphs, High Contrast, the jump list and taskbar progress,
  Options' magnet/.torrent owners and Dial Up / VPN (B43), a greyed control that stopped the
  wheel and still took clicks, the chosen language put in force only once Options was saved (every
  system), a hand-off that waited out the window's quarter-second coalescing (B36's click-to-first-
  request), Download details asking the server only once its window was made, and
  `cargo packager` making both windows' installers. Windows' default is the GPUI window
  (Makefile, CI, the scripts), as macOS's (`docs/decisions.md`, 2026-11-11). Left on Windows:
  a whole `human-qa.py` run, B36's click timing included, on an installer built after the review
  fixes (G7); G10, Options' Mica switch (G11), IME by hand (check 6), CI's draft run of the release
  workflow (G8).
- **Linux (maintainer, 2026-11-11: "only focus on linux part. i am working on windows part on
  windows laptop"): done on the Mac, in Docker.** Linux ships the GPUI window (`docs/decisions.md`,
  2026-11-11): `linux-human-qa.sh` 64/64 (B25 compared), `linux-gui-check.sh` 122/122 with real
  input, the Wayland close check, links dropped on X11 and Wayland, the .deb, AppImage and .rpm
  built and the .deb installed and driven. Every Linux check builds `dm-gpui` unless
  `DM_WINDOW=egui` (`scripts/linux/window.sh`). Left on Linux: drag out (G10), IME by hand.
- **Native feel and speed (maintainer, 2026-11-10).** Every GPUI slice (G6 to G8) is built as
  each system's own programs behave and measured against G5's egui numbers: the plan's section
  "Native feel and speed". A contract row a native change alters is updated with its baseline in
  the same commit, naming that word.
- **G7 to G9: after D1.** The plan's Phases C, D and E. D3 (new B25 pictures): recommended
  approve once every other row holds in GPUI, drawing them with `--save-baseline` and looking
  at them. D4: for a row GPUI cannot hold as written, recommended the fallback the gap
  analysis names (for example `tray-icon`, `rfd`, dm's own drag-out code). D5: recommended
  switch the default only after 64/64 on macOS, Windows and Linux from release packages,
  and remove egui in a change of its own.

## Periodic

- Search GitHub for copies of dm that drop `NOTICE` (`dm-engine`, `com.dm.engine.json`,
  `easy-download-manager@pranavo72bex`) and file a DMCA notice for one. Last run 2026-11-09:
  no copy (the one `dm-engine` crate found, `dionkadi/rDM`, is a separate Tauri and SQLite
  program).
