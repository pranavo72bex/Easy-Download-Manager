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

1. **Faster than reference A in every benchmark.** Every case of the side-by-side harness
   (`D:\compare\release-0.9.0\bench.mjs`: `fast`, `per_conn`, `flaky`, `outage`, `batch` and
   `inet`, and `idle`, where dm must use no more CPU and memory at rest), and any case added to
   it later. Both apps run on the same computer, in the same run, and the file must come out
   intact. A speed row closes only when dm is faster, not just close. Any case where dm is
   slower is a row in this list.
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
JavaScript moved into Rust) is done but for a Firefox run on macOS or Linux; row 171 is what is left
of the flaky and outage cases, and row 172 the batch case, which the harness cannot yet compare.

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
| G6  | GPUI migration, Phase B: no GPUI shell yet to decide go or no-go (screen readers in every dialog window first; software rendering under Xvfb and GPU-less Windows; B42 placement; tray; drag and drop; IME; size) | OPEN | `desktop/crates/dm-gpui` on the proposed pin, each check of `docs/migration/GPUI_MIGRATION_PLAN.md` Phase B pass or fail on macOS, Windows, X11 and Wayland; the maintainer's D1 (go) and D2 (pin) |
| G7  | GPUI migration, Phase C: the window in eight slices (main window; table; row commands; Add URL and the questions; queues and Scheduler; progress and failure windows; Options; dialogs, tray, shortcuts, accessibility, B25 pictures) | OPEN (macOS first; D1 go for macOS, 2026-11-10) | Each slice's contract rows (`docs/migration/GPUI_MIGRATION_PLAN.md` Phase C) hold in GPUI on every system, run side by side with egui by `human-qa.py`; D3 for the new B25 pictures |
| G8  | GPUI migration, Phase D: platform integrations to adapt or redo for GPUI (`docs/migration/PLATFORM_GAP_ANALYSIS.md`) and packaging | OPEN (macOS under way: the tray, the Dock's reopen, the login launch, the window put away, moved and pictured through AppKit, links dropped from a browser, `--headless` with no GPUI; next, the release bundle and its smoke test) | The release workflow's packages, downloaded from a draft, hold 64/64 on macOS, Windows and Linux |
| G9  | GPUI migration, Phase E: parity report, the default switched, egui removed | PARTIAL: on macOS the GPUI window is the one built and shipped (2026-11-10, after its .dmg held 64/64); Linux and Windows keep egui until theirs hold the contract in GPUI; egui removed after | `docs/migration/EGUI_PARITY_REPORT.md` row by row; the maintainer's D5; egui, its three vendored patches and kittest removed in a change of their own |
| 173 | `handover-check.py libreoffice` fails: "nothing started". On 2026-11-10 the page's Mac link (`a.os_macos`, the first match) is visible and topmost when measured, yet after the check's click the tab is at `libreoffice.org/download/` and no download starts, in dm or in Edge; the same with the build before G2, so not that change. The link itself is a plain 302 to a mirror's `.dmg`. Tried and dropped: choosing the first link a click reaches, an instant scroll, waiting for the link to hold still | OPEN | Find what the page does with the click (its script, or where the click lands) and make the check click as a person does; the other 18 sites hold |
| 174 | `window::options::site_limits_attempts_and_the_timeout_are_set_in_options_and_reach_the_queue` failed once in a whole `cargo test --workspace` (2026-11-10: the settings file read back said `read_timeout` 30, not the 10 just saved) and passed in the next two whole runs of the window tests and three alone; again later that day, the same way, then passing alone and in a whole run of its binary (291 of 291): another test in that binary writes the shared settings file without holding `support::settings_file()` | OPEN | Find the test that saves settings without the guard (a hint, Options' OK, a speed command) and have it take the guard; the window binary passes ten whole runs |
| 121 | An expired signed stream address is renewed: from the open page (`decide::Handed`, `Start::renews`) and by the browser's next hand-over of the same file (`Entry::fresh_link`, row 57). Tested offline only | OPEN | A real video site's CDN address that runs out mid-download, tab left open in Microsoft Edge, goes on in the same row |
| 163 | GitGuardian fails pull request #33 on two made-up test passwords (`s3cret`, `p4ss`) in commit 62baeca's history; nothing like them is in the tree, and gitleaks finds no leak | OPEN: the maintainer | In GitGuardian's dashboard, resolve incidents 39386940 and 39386941 as test credentials |
| 169 | The extension is not all Rust yet (asked by the maintainer, 2026-11-10). About 1,200 lines of JavaScript decide things on their own: `extension/content.js` (616: key-held clicks and magnet links, the page's links for "Download all/selected links", the media check that loads the panel, the answers kept for the quality list, and a second, plain-JavaScript "Download video" panel for pages whose policy forbids WebAssembly, Reddit's), `extension/document.js` (453, in the page's own world: the player's requests and JSON answers, the property paths and selector chains of the site rules, the feed's player ids), `extension/background.js` (128) and `extension/popup.js` (11). Some of it was kept JavaScript on purpose (`docs/decisions.md`: 2026-10-12, a Manifest V3 worker registers its listeners on its first turn; 2026-10-17, Alt-click and the links work where a page forbids WebAssembly, which Firefox applies to content scripts; 2026-10-20, no WebAssembly compiled in a frame that shows no media; and a page-world script runs under the page's own policy) Done on 2026-11-10: the background and the popup have no JavaScript of their own. `scripts/package.sh` writes the WebAssembly into the background's script (`pkg/background.js`), which starts it at once (`initSync`), so Rust registers the listeners on the worker's first turn itself (`web/listen.rs`; `chrome-check.py` "every listener is registered on the first turn", 7 of 7); `pkg/popup.js` only starts the Rust; `extension/background.js` and `extension/popup.js` are gone, and a test holds the two written scripts to starting the Rust. Which form bodies are kept and how they are written (`decide::posted_body`, tested against `URLSearchParams` and `btoa` output) and the panel's file put into the asking frame (`web/page.rs`) were the old loader's and are Rust's; the rules `content.js` and `document.js` keep (the panel's words, five of them shared with the full panel, which now takes them from there; the click-as-drawn limits; the kinds shown in the tab; the gesture window; the caps on kept answers; the address, stream and piece patterns; the early answers; the steps of a path or chain; the magnet's pass) are named once in `decide/scripts.rs` and each script is held to them by a test. Checked: the extension's 154 host tests, clippy for the host and WebAssembly, `chrome-check.py` 92 of 92 in Microsoft Edge (the form's answer sent again as the page sent it, the panel put in, the plain panel where WebAssembly is forbidden, Alt-click, all links). Kept JavaScript at the maintainer's word of 2026-11-10 (`docs/decisions.md`): `content.js` and `document.js` (1,069 lines), whose rules are Rust's and held to them by tests. They must work where a page forbids WebAssembly, so Rust could run there only compiled to plain JavaScript (binaryen's `wasm2js`); measured that day, a prototype of one hook came to 110 KB of JavaScript (19 KB gzipped) and 1.1 MB of memory in every page and frame before any of their logic, against 22 and 30 KB for the two scripts. Left: Firefox, where the background now starts its WebAssembly at once on the background page's main thread (none here: `firefox-check.py` fetches Firefox on macOS and Linux only; branded Chrome no longer loads an unpacked extension). | PARTIAL | On macOS or Linux: `python3 scripts/firefox-check.py native` passes with the background of `a9e891ab` (its listeners registered on the first turn, a download handed to dm), and the extension's Edge checks and `human-qa.py` still pass. Not to be done without the maintainer's word: the two plain panels' wording made one ("dm did not answer…" against "dm did not answer in time…"), or Reddit's player moved from `content.js` into the app's site rules |
| 170 | Dailymotion: the panel on a public video lists no qualities, so nothing can be chosen (`video-sites-check.py` PUB-03, https://www.dailymotion.com/video/xbuzu66, signed out, Windows, Microsoft Edge, 2026-11-10). Not dm's reading: the check's picture of the page (`SITES_SHOT`) shows a "Join Dailymotion" sign-up box over the player for a new profile signed in nowhere, and the check's consent words took that box's own "Continue" for a consent button (it then said "Check that's the right email"). Behind it the player never asked for the video: DevTools on every frame and worker saw no metadata, playlist or media request in 60 s, and the extension's own listener none either, so the panel was on a 335×188 video that is not the page's. Set aside at the maintainer's word of 2026-11-10: Dailymotion does not let a visitor who is not signed in download ("ignore it for now") | BLOCKED: a sign-in | The check tells a sign-up box from a consent one (a box with an address or password field is a wall: BLOCKED), and `python scripts/video-sites-check.py --phases public-panel --panel-sites dailymotion` passes: the panel lists the video's qualities and a whole video with sound lands in dm's folder |
| 171 | Beside reference A, measured from the first request, dm ends 0.3 to 0.5 s behind on a server that keeps cutting connections and through a 15 s outage, though level or ahead from the moment each app is given the address (2026-11-10, `bench.mjs`, dm's home on C:, both saving to the hard disk D:: `flaky` 8.61 and 8.76 s against 8.31 and 8.24 from the first request, 8.63 and 8.78 against 8.82 and 8.76 from the address; `outage` 23.64 against 23.33, and 23.65 against 23.82). The difference is in dm's finish after the last byte, 0.37 to 0.56 s against 0.19 to 0.30, and in what it fetches twice, 1.8 and 2.3% against 0.8 to 1.1% | OPEN | `bench.mjs flaky outage`, a fresh download folder: dm ahead from the first request too, every file whole |
| 172 | The `batch` case has no comparison yet. It ran to its end for the first time on 2026-11-10, once `bench.mjs` stopped blocking its own server while it handed reference A the files (`spawnSync`): dm fetched all 24 files of 2 MiB whole in 5.66 and 5.74 s, and reference A, its queue filled and started from the command line (`/d … /a`, then `/s`), finished 3 and 4 of the 24 within the 300 s allowed, which says more about how the harness starts its queue than about reference A | OPEN | `bench.mjs batch` gives reference A its files as a person would, both apps finish all 24 whole, and dm is the faster |
| T5  | dm is listed in Windows' Settings › Default apps for magnet links and `.torrent` files (`register.rs` `list_in_default_apps`, tested with a stand-in `reg`) but has not been seen there | OPEN | A person on Windows with an installed build sees dm there and chooses it for a magnet link |

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
  left (`frame.rs`). Synced with `main` at a6faac1 (the engine's speed work, the extension's
  background and popup in Rust); `human-qa.py` 64 of 64 on the GPUI window on that tree.
  The release bundle from its .dmg: `smoke.py` and `human-qa.py` 64 of 64. The menus' titles in the
  top bar beside the search box, on every system (the maintainer's words); only the system's fonts.
  Windows and Linux coded here (`syswindow.rs`, the title bar, the tray's icon, window names): Linux's
  GPUI binary passes `smoke.py` and `app-check.py` under Xvfb; Windows builds and lints. Owed on
  Windows: running it. Each system's own colours (Fluent on Windows, libadwaita on Linux); a
  browser's link dropped on the window on Windows through the OLE target both windows now share.
  **Still owed in G7:** View › Text size (GPUI has no zoom:
  every size in points; it needs sizes in rems and `Window::set_rem_size`). Then
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
  stays cross-platform. Owed before anything merges into `main`: the Linux runs, and Windows'.
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
