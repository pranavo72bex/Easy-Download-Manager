# BUGLOG — the Docker QA loop

Bugs found by `qa/` (the Docker test stack), how each was reproduced, its
root cause and its fix; and the places where a test, not dm, was wrong.

## Summary

The loop ended on 2026-10-29 after five full runs and three in a row with
every scenario passing (37 of 37) and every browser check passing (69 of 69),
with no flaky result in those three.

- **Found and fixed: 5.** BUG-001 a range whose server overstates its length
  was fetched for ever; BUG-002 an untrusted certificate was retried for two
  minutes; BUG-003 Brave refused the panel's WebAssembly on YouTube; BUG-004 a
  silent yt-dlp kept its row running for ever; BUG-005 the new panel loader
  logged an error where WebAssembly is forbidden. Each has a regression test
  that failed before its fix.
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
    Metalink, torrents, and yt-dlp (run as a child process) for video pages.
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

### BUG-004 — a yt-dlp that never says anything keeps the row running for ever

- **Scenario**: `ytdlp_crash_and_hang` (a fake yt-dlp that is `sleep 100000`,
  on an extension-less watch address, which is yt-dlp's alone).
- **Symptom**: after 300 s the row is still "running" at 0 bytes; nothing is
  said. The page reading gives up after 120 s (`READ_WITHIN`), but the
  fallback, yt-dlp's own download (`ytdlp.rs`), waited on it with no limit.
- **Root cause**: the download loop read yt-dlp's output until it closed and
  checked only pause and cancel while it was quiet.
- **Fix**: `Heard`, a watchdog on both streams: no word for 120 s before the
  first (as long as a reading may take; `DM_YTDLP_QUIET` sets it for tests),
  or for ten minutes once it has spoken, ends yt-dlp's process tree and says
  "yt-dlp stopped answering".
- **Regression test**: `dm-core/tests/ytdlp_quiet.rs`
  `a_yt_dlp_that_says_nothing_is_ended` (before: still waiting after 60 s;
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

## Open, not fixed

- Closed 2026-10-29: the "(1)" in "QA clip (1) [watch？v=ok-1].mp4" was not
  dm's: yt-dlp's `HTML5MediaEmbed` numbers every video on a plain page
  (`'{title} ({num})'`, `'{id}-{num}'`), even the only one, and leaves the
  playlist fields out when there is one. dm now drops the number for a
  page's only video (`ytdlp/fetch.rs` `unnumbered`); `video_page` checks the
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
- **T-007** `ytdlp_crash_and_hang`, `video_page`: used `video.html`. An
  address that names a document is yt-dlp's first and, if yt-dlp cannot make
  a video of it, saved as the document (by design, `download/mod.rs`), so a
  crashing yt-dlp gave a "completed" 188-byte page and the test passed without
  testing anything. They now use an extension-less `/watch?v=`, which is
  yt-dlp's alone.

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
