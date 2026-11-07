# BUGLOG — the Docker QA loop

Bugs found by `qa/` (the Docker test stack), how each was reproduced, its
root cause and its fix; and the places where a test, not dm, was wrong.

## Summary

The loop ended on 2026-10-29 after five full runs and three in a row with
every scenario passing (37 of 37) and every browser check passing (69 of 69),
with no flaky result in those three.

- **Found and fixed: 7.** BUG-001 a range whose server overstates its length
  was fetched for ever; BUG-002 an untrusted certificate was retried for two
  minutes; BUG-003 Brave refused the panel's WebAssembly on YouTube; BUG-004 a
  silent the external video tool kept its row running for ever; BUG-005 the new panel loader
  logged an error where WebAssembly is forbidden; BUG-006 the panel stayed
  put when the player changed size; BUG-007 the panel stayed put when the
  player moved without changing size (found by `scripts/human-qa.py` on real
  YouTube, 2026-10-30). Each has a regression test that failed before its
  fix.
- **Open: 1.** BUG-003 is not yet confirmed on real YouTube
  (Google blocks the container's browser for now), and the Firefox live-page
  panel check has not been compared with the build before it.
- **Tests that were wrong: 8** (T-001 to T-008), each explained below before
  it was changed. Most were the test expecting a failure where dm, by design,
  pauses with a reason, or a test environment that was not what it claimed.
- **Flaky: none** in the last three runs. Run 1's network and folder
  failures were the stack running two suites at once (T-006), not flakiness.
- **Not done, and why**: `cargo fuzz` (needs nightly; the repository fuzzes
  with proptest on stable, weekly in depth) and `cargo deny` (a licence and
  source policy is the maintainer's to write).

## Architecture (read before changing anything)

- **`desktop/`** is the product: a Cargo workspace.
  - `dm-core`: the download engine, no window. Probes a URL, plans ranges, runs
    up to 32 connections into one staged file (`parts/<id>/staged`) with a
    ledger per range (`done-NNNN`, written every second or 4 MB), verifies a
    resumed range's tail against the server before trusting it, retries what
    waiting can fix (`retry.rs`: 10 tries by default, backoff to 30 s), and
    moves the file into the download folder at the end. Also HLS/DASH, FTP,
    Metalink, torrents, and the external video tool (run as a child process) for video pages.
  - `dm-shell`: settings, the queue file, the browser host and `--register`,
    the one-instance lock.
  - `dm-desktop`: the program. The window (egui), and `--headless` with no
    window; a local **control port** (JSON lines on 127.0.0.1, token in
    `DM_HOME/control.token`: `downloads.add`, `queue.list`, `downloads.pause`,
    `.resume`, `.cancel`, ...), the command line and an MCP server on it.
  - `dm-protocol`: the messages between extension and app.
- **`browser/`** is the extension's logic in Rust, compiled to WebAssembly;
  **`extension/`** the manifests and three small JavaScript loaders.
- **Extension → app is native messaging.** The browser starts `dm-desktop`
  with its extension origin as the first argument; that process (the host)
  answers `engine.hello` / `settings.get` / `download.start`, writes the
  request into `DM_HOME/inbox`, rings the running app's doorbell (or starts
  it) and exits. A download the browser held is resumed in the browser if
  the app does not take it.
- **There is no server of dm's own**; nothing leaves the computer but the
  downloads themselves.

## How the QA stack runs

`sh qa/run.sh` (README, "How to run the test suite"): `origin` (every HTTP
condition, `qa/origin/origin.py`), `toxiproxy`, `mockvideo` (pages and HLS made
by ffmpeg at build time), `app` (dm built from this checkout, headless, the
scenarios in `qa/tests/scenarios.py`, downloads on a 200 MB tmpfs, as a
non-root user) and `browser` (Chromium with the unpacked extension,
`scripts/chrome-check.py`), on a Docker network with no way out.

Deviations from the brief, and why:

- The extension check is `scripts/chrome-check.py`, the repository's own,
  driven over DevTools, not Playwright: it already covers the click-to-dm
  hand-off, dm not running (the browser keeps the download), and the catch
  rules (types and sizes), and a second copy in Playwright would drift from it.
- Chromium is Debian's package, not Chrome for Testing, which has no Linux
  arm64 build.
- No HTTPS case can succeed: dm uses rustls with the built-in webpki roots, so
  no certificate the stack can make is trusted. Every HTTPS case (private CA,
  expired, wrong host, self-signed) must be refused, with a reason.

## Bugs

### BUG-001 — a range whose server overstates its length is fetched for ever

- **Scenario**: `length_wrong` (origin `len=`: `Content-Length` 1 MB more than
  is sent, then the connection is closed).
- **Symptom**: the row stays "running" at 100 % for good; the origin sees the
  last 64 KB asked for again every few seconds (`[983040, 1048576]` repeated).
- **Root cause**: `download/ranged.rs` read on after a range had every byte it
  asked for; the server's early close then came back as a body error, which
  was returned as a network failure. The retry checked the tail (64 KB),
  completed the range, hit the same error, and the progress it made reset the
  retry count, so it never ended.
- **Fix**: a body error once the range has all its bytes ends the range
  (`Err(_) if written >= span_now => break`). An unknown length plans
  `u64::MAX`, so a short stream still fails.
- **Regression test**: `dm-core/tests/errors.rs`
  `a_range_that_came_whole_under_a_longer_stated_length_is_finished` (failed
  before: `Network(... IncompleteBody)`; passes after).

### BUG-002 — an untrusted certificate is retried for over two minutes

- **Scenario**: `bad_certificates` (HTTPS with a private CA, expired, wrong
  host name, self-signed).
- **Symptom**: the row "running" at 0 bytes for 2+ minutes before it fails.
- **Root cause**: reqwest reports a refused certificate as a connect error;
  `retry::transient` retries every connect error, so all ten tries were spent
  on a certificate that cannot change.
- **Fix**: `retry::untrusted_certificate` — an error whose chain mentions the
  certificate is not transient. The reason shown already names it.
- **Regression test**: `dm-core/tests/errors.rs`
  `an_untrusted_certificate_is_said_at_once_not_retried` (before: still trying
  after 60 s; after: refused in milliseconds, one connection).

### BUG-003 — Brave refuses the panel's WebAssembly on YouTube (docs/gaps.md row 30)

- **Scenario**: `scripts/linux/cachyos-brave/stress.sh` (real YouTube; not part
  of the offline suite, which must not reach real sites).
- **Symptom**: "Unsafe attempt to load URL chrome-extension://…/pkg/dm_catcher.js
  … Domains, protocols and ports must match" in the extension's errors.
- **Root cause**: `content.js` imported the panel's module from the page's
  frame; the fetch was checked against the page's origin.
- **Fix**: the background injects `pkg/panel.js` (wasm-bindgen `no-modules`
  glue with the WebAssembly in it) with `scripting.executeScript`; the page
  fetches nothing of the extension's; `web_accessible_resources` is gone.
- **Regression test**: `browser/src/decide/tests.rs`
  `the_panel_is_put_in_by_the_browser_and_fetched_from_no_page`;
  `ext-check.sh` shows no `chrome-extension://` request from the page.
- **Open**: not yet confirmed on real YouTube (Google blocks the container's
  browser for now); the Firefox live-page check (W3Schools) did not show the
  panel and has not been compared with the build before this change.

### BUG-004 — an external video tool that never says anything keeps the row running for ever

- **Scenario**: `videopage_crash_and_hang` (a fake the external video tool that is `sleep 100000`,
  on an extension-less watch address, which is the external video tool's alone).
- **Symptom**: after 300 s the row is still "running" at 0 bytes; nothing is
  said. The page reading gives up after 120 s (`READ_WITHIN`), but the
  fallback, the external video tool's own download (`videopage.rs`), waited on it with no limit.
- **Root cause**: the download loop read the external video tool's output until it closed and
  checked only pause and cancel while it was quiet.
- **Fix**: `Heard`, a watchdog on both streams: no word for 120 s before the
  first (as long as a reading may take; its environment switch sets it for tests),
  or for ten minutes once it has spoken, ends the external video tool's process tree and says
  "the external video tool stopped answering".
- **Regression test**: `dm-core/tests/videopage_quiet.rs`
  `a_helper_that_says_nothing_is_ended` (before: still waiting after 60 s;
  after: ended in 2 s).

### BUG-005 — on a page that forbids WebAssembly, the new panel loader logged an error

- **Scenario**: the browser check, "on a site whose policy forbids
  WebAssembly the panel stays away, quietly" (found in the BUG-003 change
  before it was committed).
- **Root cause**: `pkg/panel.js` started compiling at once; the promise
  rejected before `content.js` awaited it, and Chrome logged it as uncaught.
- **Fix**: the loader handles the promise as it makes it
  (`__dmPanelCode.catch(() => {})`); `content.js` still sees the rejection.
- **Regression test**: `browser/src/decide/tests.rs` (the loader is built so),
  and the browser check itself.

### BUG-006 — the panel stayed in the player's old corner when the page re-laid the player

- **Scenario**: `scripts/human-qa.py`, row B33 on a real YouTube page in
  Edge, with the machine loaded (a Docker image building): YouTube's page
  settled after the panel was up, the player went from 809 to 742 pixels
  wide and down 12, and the button sat 51 pixels past the player's new
  right edge until the pointer moved.
- **Root cause**: the panel followed a scroll and a window `resize`
  (`browser/src/content/handoff.rs`), and nothing else; a player that
  changes size with the window unchanged (a page laying itself out late, a
  theatre mode) moved out from under it.
- **Fix**: a `ResizeObserver` on the player the panel sits over
  (`show.rs` `follow_size_of`), re-anchoring as a scroll does; dropped when
  the panel hides.
- **Regression test**: `scripts/chrome-check.py`, "when the player changes
  size, the panel follows into its new corner, with no pointer move" (the
  player shrunk by script, no pointer event); failed before, passes after.

### BUG-007 — the panel stayed put when the player moved without changing size

- **Scenario**: `scripts/human-qa.py`, row B33 on a real YouTube page in
  Edge (2026-10-30, twice in the day's runs it held, once it failed). A
  timeline of the page (`getBoundingClientRect` of the player and the panel
  every 100 ms from the hover): the player lays out at (24, 80) 742 × 557,
  the panel is placed on it; about 0.8 s later the player is at (16, 68),
  the same size, and the panel has not moved; at 1.1 s the player widens to
  809 × 607 and the panel follows (BUG-006's observer). A page that moves
  its player without resizing it (something above it folding) leaves the
  panel where the player was, until the pointer moves.
- **Root cause**: the panel followed a scroll, a window resize and a size
  change (`ResizeObserver`), and nothing else. No observer reports a plain
  move. The check was also judging the panel against a box measured before
  the hover, so a panel that had followed the second layout was called
  wrong against the first (that is what failed in the QA run; the 51 px
  BUG-006 recorded as "past the new right edge" was the same stale box).
- **Fix**: while the panel is shown, a timer every 200 ms compares the
  player's box with the one the panel was placed for and places it again
  when they differ (`show.rs` `follow_if_moved`, `FOLLOW_EVERY_MS`);
  cleared when the panel hides. `youtube-check.py` judges the corner
  against the player's box as it is now, with up to four seconds for the
  panel to follow.
- **Regression test**: `scripts/chrome-check.py`, "when the player moves
  without changing size, the panel follows, with no pointer move" (a 50 px
  block put in above the player by script, no pointer event); failed before,
  passes after.

### BUG-008 — FTP and torrents refused on a computer with no proxy

- **Scenario**: `scripts/linux-stress.sh` (2026-11-06, the Debian container,
  no network): every FTP download failed with "dm cannot send FTP through
  the computer's proxy settings, and does not go around them". The same on
  macOS with no proxy set.
- **Root cause**: a new install follows the computer's proxy settings (gaps
  row 80, decided 2026-11-06). For dm's own connections (FTP, a torrent's
  swarm, RTMP/RTSP) the address was looked for in the environment only; with
  none, `Options::no_route_through_system_proxy` refused, as if a proxy were
  in force and unreadable. A computer with no proxy at all was the usual case.
- **Fix**: `proxy::system_proxy_in_force` says whether the computer has a
  proxy at all (an environment variable; on macOS the network settings,
  read with `scutil --proxy`, whose SOCKS, HTTPS or HTTP proxy is now also
  the address used). With none, dm's own connections go direct; with a
  proxy in force and no address they could use (a configuration script
  alone), they are still refused, never sent around it.
- **Regression test**: `tests/ftp.rs`
  `under_the_computers_proxy_settings_with_no_proxy_ftp_goes_direct` (it
  replaced a test that expected the refusal), `download::tests`
  `under_the_computers_proxy_only_a_proxy_in_force_with_no_address_refuses`,
  `proxy::tests` `the_mac_network_settings_are_read_for_a_proxy`; the stress
  `ftp` phase.

### BUG-009 — an incoming peer waited for the next connection after a refused one

- **Scenario**: building encrypted peer connections (gaps row 150), 2026-11-06:
  a seeder taking only encrypted connections never finished an encrypted
  handshake that began while another connection was being refused; the
  connecting side timed out after 20 s.
- **Root cause**: librqbit's listener (`session.rs` `task_listener`) collects
  the checks of incoming connections in a `tokio::select!` branch with the
  pattern `Some(Ok(..))`. A check that failed did not match, which disables
  that branch until the loop goes round, which it did only when another
  connection was accepted; until then no check under way was polled. A
  plaintext handshake is read at once, so it rarely showed; an encrypted one
  takes several round trips.
- **Fix**: every finished check is taken, a failed one dropped inside the
  branch (dm's patched librqbit, `desktop/vendor/README.md`).
- **Regression test**: `tests/torrent_features.rs`
  `a_seeder_that_takes_only_encrypted_connections_is_reached` (it runs a
  refused plaintext connection beside the encrypted one; it timed out before).

### BUG-010 — Wikimedia's originals stalled for ever on "Too many requests"

- **Scenario**: `scripts/video-sites-check.py --phases public-url` on Windows,
  2026-11-07 (test PUB-11a): a 36 MB video original from
  `upload.wikimedia.org` stood at 9 to 13 MB for 30 minutes, the row saying
  "Too many requests (retry 7 of 15)". Its 10 MB transcode from the same host
  came whole.
- **Root cause**: Wikimedia's media servers answer a browser's name from a
  program with 429 and `Retry-After: 600` ("please contact noc@wikimedia.org
  to discuss a less disruptive approach") and serve dm's own name at once
  (measured: Internet Explorer's and Firefox's names 429, `dm/0.8.7 (+…)` 206,
  with and without a `Referer`; eight ranges at once by dm's name all 206). dm
  sends Internet Explorer's name first, as reference A does, and moved to its
  own only when a server answered 403; a 429 was waited out under the same
  name. The limit came now on the first request and now only on the ranges
  after it.
- **Fix**: a 429 to the browser's name asks once more by dm's own at the
  first request (`probe.rs`), and on any later request moves that server to
  dm's name for the rest of the run (`agents::limited`, from
  `Error::from_response`). A server that limits every name gets one request
  more than before and is waited out as before; with Identify as dm only on,
  a limit never moves dm to another name.
- **Regression test**: `tests/browser_agent_limited.rs`
  `a_browsers_name_told_too_many_requests_is_asked_again_by_dms_own_once`
  (four cases: ranges limited in the browser's name, the first request
  limited, everyone limited, the switch on). Before the fix the first case
  failed with `Refused { status: 429 }` after 481 s and the second after
  four tries in the browser's name.

### BUG-011 — a large download on Windows took twice reference A's time

- **Scenario**: `D:\compare\plan_side.py large` (the video test plan's 4.5 GiB case, both
  apps against one server on loopback), 2026-11-07, release build: dm 194 s, reference A
  101 s, both byte for byte.
- **Root cause**: the staged file is made as long as the download before any range writes
  (`ranged::preallocate`), on the understanding that "the room is only taken as bytes land".
  So it is on Linux and macOS; on NTFS a write far into such a file makes Windows write zeros
  over everything before it first. Measured on this disk: 64 KB written at the end of a 4.5
  GiB file took 94.5 s; the same write into a sparse file, 0.00 s.
- **Fix**: on Windows the staged file is made sparse before its length is set
  (`download/files.rs` `sparse`, `FSCTL_SET_SPARSE`). The same case: dm 73 s, reference A
  96 s.
- **Regression test**: `download::files::tests::a_staged_file_takes_its_room_only_as_bytes_land`
  (a GiB staged file with 64 KB written at its end takes under 16 MB on disk); without the
  fix it fails, "not sparse: attributes 0x20".

### BUG-012 — on Windows every finished download's toast kept three handles

- **Scenario**: `scripts/stress-check.py leak` on Windows (five waves of 150 files of 2 MB):
  open handles grew 545 -> 935 while memory and threads stayed level (gaps row 156). Counted
  by kind of kernel object on 2026-11-07: about two semaphores and one event a download, kept
  after 20 s idle; one download at a time, 100 files left +184 semaphores and +92 events.
- **Not the cause**, though the row first suspected them: the add-and-start path (files
  added for later and started by Start queue kept as many), the engine (`dm-core`
  `tests/handles.rs`: one range or four, +4 handles over forty downloads), the proxy lookup
  and the quiet-hours question (50 asked, +0).
- **Root cause**: a finished download's toast. On Windows notify-rust registers a click
  handler and a dismissal handler on every toast and never takes them off, and Windows keeps
  two semaphores and an event for each registration for as long as the program runs, the
  Action Center cleared or not: 30 toasts with the two handlers kept +90 handles, 30 without
  +0, from one thread or a thread each. A batch finishing in one pass is one toast, which is
  why 256 KB files (finishing together) kept fewer than 2 MB ones.
- **Fix**: on Windows dm posts its toasts itself (`dm-desktop` `toast.rs`, WinRT through the
  `windows` crate it already had): the same toast (title, an empty line, the body, short,
  silent, PowerShell's id); none with nothing to open has a handler, and one with a file has
  its click heard until it is clicked or leaves the screen, then both handlers are taken off;
  at most eight are heard at once. notify-rust is no longer built for Windows.
- **Regression test**: `dm-desktop` `tests/handles.rs`
  `toasts_give_their_handles_back` (toasts into the Action Center without a banner, then
  taken out: 30 plain and 30 with a click keep at most 10 handles each; +0 and +0 on this
  computer). The same toasts with their handlers left on, as notify-rust left them, kept +89.
- **After**: 450 downloads of 2 MB through the headless app kept +2 semaphores and +5 events
  (the release build before the fix: +170 and +101), and `stress-check.py leak` passes: handles
  409 -> 429 and threads 81 -> 79 from the first wave to the fifth. That phase also read its
  numbers 3 s after a wave, when the runtime's idle blocking threads (kept 10 s, each holding
  a handle) were still there, and called them a leak; it now reads the first and last waves
  once the threads stop falling (`settled_stats`).

### BUG-013 — on Windows a torrent naming another drive made empty files there

- **Scenario**: the source review of 2026-11-07 (`docs/security-report.md`, gaps row 159 b):
  a torrent whose file is named `D:x` passes librqbit's name check (no `..`, no separator).
- **Root cause**: the vendored storage joins each name onto the download folder with
  `PathBuf::push`, which on Windows starts again at a name with a drive (`D:x` is `x` in drive
  D:'s current folder), and creates every file before dm's own check refuses the torrent. No
  content was written, but empty files were made where the torrent chose.
- **Fix**: the vendored `init` checks every file's name before it makes any: plain parts only,
  no root, no `..`, no drive, no `:` (`plain_relative`, `desktop/vendor/README.md`).
- **Regression tests**: `dm_tests::a_torrents_file_name_must_stay_in_its_folder` in the
  vendored `fs.rs`, and dm-core `tests/torrent_names.rs`
  `a_torrent_naming_a_drive_makes_no_file_on_it`, which without the fix found the file made in
  the test's own folder, outside the download folder.

### BUG-014 — saved engine-wide settings were not in force until Options' OK

- **Scenario**: dm started with "Identify as dm only" saved on (contract row B51: "the change
  applies at once and is kept"), a second network chosen, or a dial-up connection to wait for
  (gaps row 93, G84). Found 2026-11-08 beside reference A: a download started at once, with
  the dial-up connection set and down, where reference A dialled first.
- **Root cause**: `shell::apply` put those three in force (they live in the engine, not on the
  queue), and only a change of settings called it; `shell::start` built the queue from the saved
  settings and left the engine as it began, so the switch read on and was not in force.
- **Fix**: one `engine_wide` in `shell.rs`, called at the start and at every change (at the
  maintainer's standing word of 2026-11-07, "if found bug or improvement just implement it").
- **Regression test**: `dm-shell/tests/start_in_force.rs`
  `saved_settings_are_in_force_from_the_start`, which failed before the fix (dm's own name was
  not the first sent).

## Open, not fixed

- Closed 2026-10-29: the "(1)" in "QA clip (1) [watch？v=ok-1].mp4" was not
  dm's: The external video tool's `HTML5MediaEmbed` numbers every video on a plain page
  (`'{title} ({num})'`, `'{id}-{num}'`), even the only one, and leaves the
  playlist fields out when there is one. dm now drops the number for a
  page's only video (`videopage/fetch.rs` `unnumbered`); `video_page` checks the
  name, and failed before the change.

## Tests that were wrong

- **T-001** `hostile_filenames`: a raw control byte (`\x01`) in a
  `Content-Disposition` header is not valid HTTP and the client rightly
  refuses the response. A hostile server's real form is percent-encoded in
  `filename*`, which the test now sends.
- **T-002** `http_errors`: waited 120 s for a lasting 500 to fail; dm's
  default is ten tries with backoff to 30 s (about 135 s), by design. The
  scenario now allows three tries.
- **T-003** `resume_after_restart` / `_kill9`: stopped dm 0.75 s in, before
  dm's first once-a-second record of how far each range got, so starting
  again was right. The file now arrives slowly enough that the stop comes
  seconds in.
- **T-004** `cookie_from_browser`: looked for a `Cookie` header; hyper sends
  header names in lower case.

- **T-005** `disk_full`, `five_gigabytes_on_small_disk`: expected "failed".
  dm, by design, pauses a download that will not fit before fetching a byte
  and says why in the row's `stalled` ("not enough room: the file needs
  250.0 MB, and … has 200.0 MB free"); the window offers another folder or
  Resume. The tests now expect that pause and its reason. A new scenario,
  `disk_fills_mid_download` (no length, no ranges, so nothing to refuse up
  front), covers the disk filling while bytes arrive.
- **T-006** the stack: Docker mounts a tmpfs `noexec` unless told otherwise.
  `chrome-check.py` copies the native host into `/tmp`, so Chromium could not
  start it ("Native host has exited") and every hand-off stayed in the
  browser, as designed when there is no app. `/tmp` is now mounted `exec`.
  Also: `browser` had `depends_on: [app]`, so `docker compose run browser`
  started a second copy of the whole suite beside the first; the two shared
  the origin's counters and the toxiproxy proxy, which is what made
  `folder_removed_or_read_only` and the network scenarios stall in run 1.
- **T-007** `videopage_crash_and_hang`, `video_page`: used `video.html`. An
  address that names a document is the external video tool's first and, if the external video tool cannot make
  a video of it, saved as the document (by design, `download/mod.rs`), so a
  crashing the external video tool gave a "completed" 188-byte page and the test passed without
  testing anything. They now use an extension-less `/watch?v=`, which is
  the external video tool's alone.

- **T-008** the suite: each scenario left its files on the shared 200 MB
  download disk; by `disk_full` 2.9 MB was free and every later download was
  (rightly) paused for want of room. Each dm's folder and home are now removed
  when it closes.
- **T-009** `scripts/chrome-check.py`, a plain click on a PDF link: the link
  was measured without scrolling, and in Brave's and Edge's shorter windows
  under Xvfb it was below the fold (y 410), so the click landed on nothing
  (`elementFromPoint` empty) and dm was never asked. Now scrolled into view
  first, as `human_click` already did. Brave and Edge then pass it.
- **T-011** `scripts/stress-check.py` `flaky/lieover`: a server whose
  `Content-Length` is 1000 over the truth while `Content-Range` tells it,
  closing after the real bytes. The phase took any "completed" as a finding;
  the file was byte-exact (8 388 608). Before BUG-001 that download hung for
  ever, which is how the phase passed. Now a finding only where the
  completed file differs; `lieunder` (fewer bytes than stated) still must
  not complete, and does not.
- **T-010** `scripts/chrome-check.py` exited 0 when the extension's worker
  never started (a `return` inside the `try`), so a browser that loaded
  nothing reported success. Now exits 1.

- **T-011** `scripts/stress-check.py` and `stress-protocols.py` set
  `"retries"`, a key dm's settings never had, so every phase ran with the
  default 15 attempts. `flaky`'s 429 and 503 servers then outlasted its
  240 s wait ("still running"): with `Retry-After` the wait grows to 60 s,
  so 15 attempts take about nine minutes. The scripts now set `"attempts"`;
  `flaky` waits 420 s for ten. Found 2026-11-06 on Linux, the same on macOS.

- **T-012** The torrent tests' own sessions had the DHT and local discovery
  on, as dm's downloads do: a test's torrent was announced to the public
  network and peers on the internet were connected to (seen 2026-11-06 while
  building row 150: five addresses outside this computer, one of which
  answered a handshake). `TorrentOptions::no_public_discovery` keeps a
  session to its trackers; the tests' options set it, and a run of the
  torrent tests watched with `lsof` saw no address but loopback.

- **T-013** dm-shell's `dms_home_is_this_accounts_alone_on_windows_too` failed on the CI's
  Windows runner before it reached dm (2026-11-07): it opened its parent with `icacls` and read
  the access list back through PowerShell's `Get-Acl`, which did not show the entry just added.
  It now sets and reads the lists through the calls dm itself makes (`owner_only::by_hand`), and
  every assertion prints the list it saw; with dm's change of the home switched off it fails
  naming the inherited users entry.

- **T-014** dm-desktop's `toasts_give_their_handles_back` failed once in two full runs here and
  once on the CI ("30 toasts with a file to open kept 15 handles", the limit 10): Windows' own
  worker threads and late releases come and go. A kept handle a toast grows round after round,
  so the toasts now go in three rounds of thirty, the count is the fewest over a second once
  settled (`GetProcessHandleCount`, not a PowerShell started for each reading), and the limit is
  half a handle a toast over the ninety: 201 to 205 as dm is, 272 kept with the handlers left on
  as notify-rust left them.

## Static checks and fuzzing (Step 3)

- `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`,
  `cargo test --workspace` (desktop and browser): clean.
- `cargo audit` runs in CI (`.github/workflows/audit.yml`); `cargo deny` is
  not set up (no `deny.toml`; adding licence and source policy is a decision
  for the maintainer, not made here).
- Fuzzing is proptest on stable Rust inside `cargo test`, with a weekly deep
  run (`fuzz.yml`, hundreds of thousands of cases), as the repository chose;
  `cargo fuzz` needs nightly and was not added. Existing properties cover range
  plans (every byte exactly once), server file names (never a path),
  native-messaging frames, control-port lines, cookies files, playlists,
  Metalink and digests. Added: `Retry-After` and backoff (never more than a
  minute), login challenges (Basic, NTLM), cookie headers (kept to their site).
  5 000 cases each: no failure.

## Loop runs

All on 2026-10-29, macOS (Apple silicon) with Docker Desktop, images built for
linux/arm64 from this branch.

| Run | Scenarios | Browser | What it found |
|-----|-----------|---------|---------------|
| 1 | 30 / 6 failed | 1 failed (host could not start) | T-005, T-006 (two suites at once, `noexec` /tmp), T-007 |
| 2 | stopped | — | the app image was rebuilt during it; not counted |
| 3 | build failed | — | Docker's disk full; the image's tool install moved before the source copy |
| 4 | 31 / 6 failed | 68 / 69 | T-008 (the shared disk), BUG-004, BUG-005 |
| 5 | 37 / 0 | 69 / 69 | — |
| 6, 7, 8 (`QA_RUNS=3`) | 37 / 0 each | 69 / 69 each | — (stop condition met) |
| 9, 10, 11 (`QA_RUNS=3`, after the commit) | 37 / 0 each | 69 / 69 each | — (timings within 2 % of runs 6 to 8) |

### CachyOS, four browsers (2026-10-29)

`sh scripts/linux/cachyos-browsers/run.sh`: CachyOS (x86-64, emulated on
Apple silicon), dm and the extension built from this checkout, the hand-off
checks in each browser.

| Browser | Version | Result |
|---------|---------|--------|
| Brave | 1.98.62 (Chromium 156) | 68 / 69 |
| Microsoft Edge | 156.0.4314.50 | 68 / 69 |
| Firefox | 159.0 | every `firefox-check.py native` check (the panel's hover needs real input) |
| Google Chrome | 156.0.8078.73 | not run: see below |

- The one failure in Brave and Edge is the latency check (first request
  within 700 ms of the click): 1.12 to 1.27 s here, 0.10 to 0.26 s in the
  native arm64 runs. Each click starts the 160 MB debug dm as the browser's
  host, an x86 program under emulation; not a dm defect.
- Google Chrome ignores `--load-extension` since 137. The check now loads the
  extension over `--remote-debugging-pipe` (`Extensions.loadUnpacked`, with
  Developer mode on): Chrome accepts it with the right id, but then blocks its
  pages (`ERR_BLOCKED_BY_CLIENT`, no disable reason in the profile) and never
  starts its worker. Open: the harness, not dm, until shown otherwise.

The engine's path was searched for `unwrap`, `expect`, `panic!` and unchecked
indexing: the first three are already denied outside tests in every crate
(`clippy::unwrap_used`, `expect_used`, `panic`). Indexing that clippy flags in
`download/ranged.rs`, `assemble.rs` and `mod.rs` is by a range's own number
into tables sized when the download was planned, never by anything a server
sends; nothing was changed there.
