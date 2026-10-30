# Progress: two other download managers' behaviour in dm's engine

Checkpoint, 2026-10-29. The work list stays [gaps.md](gaps.md); this page
records the reference investigation only.

## Baseline

- Branch `qa/docker-loop`, clean at `4c51c8f` before this work.
- Protected UI: `desktop/crates/dm-desktop/` (see [architecture.md](architecture.md)).
  `git diff --stat -- desktop/crates/dm-desktop` is empty after every change.
- Baseline: `cargo test --workspace` passes (desktop and browser); the Docker
  QA loop passed 3 runs in a row (37/37 scenarios, 69/69 browser checks).

## Done

1. **Connection reuse (reference A, verified from its documentation).** Established dm's behaviour with a new
   test, `dm-core/tests/reuse.rs`: 11 requests on 4 TCP connections. Already
   a match; no engine change needed.
2. **A page's only video named without "(1)"** (gaps.md row 31, now closed).
   Cause found in yt-dlp's `HTML5MediaEmbed` reader, which numbers every
   entry. Core change: `ytdlp/fetch.rs` `unnumbered`. Tests: three unit cases,
   and the QA `video_page` scenario, which failed before the change
   ("QA clip (1) [watch？v=ok-1].mp4") and passes after ("QA clip [watch？v=ok].mp4").
   UI files changed: none.

3. **Reference B run in Docker** (Linux build 6.35.2, emulated x86-64; the
   lab is kept outside the repository). Its engine was measured against
   dm's on the same logging server, and its video add-on watched: the add-on
   is yt-dlp, which only reads the page while reference B fetches the files.
   Findings in reference-behavior.md; candidates in feature-matrix.md
   "Worth taking". UI files changed: none.
4. **yt-dlp in a sandbox on Linux** was built and tested, then set aside by
   the maintainer (not in the repository); kept as a candidate.
5. **yt-dlp out of the box**: `scripts/out-of-box-check.py` passed 2026-10-29
   from a fresh dm home (720p MP4, 480p WebM, sound alone). No replacement for
   yt-dlp was found (gaps.md row 32); it stays for now.

## Commands run

- `cargo test --workspace` in `desktop/` and `browser/`: pass.
- `cargo clippy -p dm-core --all-targets -- -D warnings`, `cargo fmt --all`: clean.
- `QA_SCENARIOS="video_page video_page_without_media hls_streams" sh qa/run.sh`: 3/3.
- `sh qa/run.sh` (full): 37/37 scenarios, 69/69 browser checks.

## Mac QA before merging into main (2026-10-29)

- `cargo test --workspace` (desktop and browser): 1246 passed, 0 failed;
  clippy, fmt and `cargo xtask privacy` clean.
- A person's walk through the real window (Add URL, Download details >
  Start now, Stop, Resume, a kill and reopening, quit and reopening), against
  a local server with slow, refusing, rangeless, short, redirecting and named
  files: 21 of 21 on this branch and 21 of 21 on `main`, every step the same.
  The window's pictures differ from `main`'s only in the clock and the
  temporary folder's name.
- Microsoft Edge 156 with the extension, `youtube-check.py` on a real YouTube
  video: 49 of 49 (the panel over the playing video, its qualities with
  sizes, the video whole in dm's folder).

## The behaviour contract (2026-10-29)

What a person does and sees is fixed in `docs/behaviour-contract.md` (38
rows); `CLAUDE.md` and `AGENTS.md` forbid changing a row without the
maintainer's approval, and QA is `scripts/human-qa.py`, the whole list, in
sight. First full run: 38 of 38, on macOS with Microsoft Edge 156, the
window's pictures recorded in `docs/behaviour-baseline/`. Writing it found
three rows that first described dm wrongly, each corrected to what dm
does, not the other way round: a repeated address is asked about ("Already
in the list"), a finished download leaves the list with Undo and no
question, and an unfinished one is asked about. One earlier run failed only
on real YouTube's quality list (yt-dlp read the same video at once just
after, and the next run passed): a site's slow moment, not a change in dm.

## The yt-dlp costs, closed (2026-10-29, approved by the maintainer)

Rows 33, 34, 35, 36 and 38 of gaps.md, each with a test that failed before
its change: a failed run fetches the newest yt-dlp once and tries again
(`tests/ytdlp_heals.rs`); the Linux sandbox (`ytdlp/sandbox.rs`, Docker both
ways); the helper fetched at the first start and its bytes told
(`tests/ytdlp_prefetch.rs`, `tests/tools.rs`); yt-dlp's failures in dm's
words from a corpus of 38 of its lines (`tests/ytdlp_words.rs`); the
portable build on the computer's Python where the release has no standalone
one (`tests/ytdlp_portable.rs`). Row 37 (proof-of-origin tokens) looked
into and left blocked: it needs Node.js and a third-party provider kept
current with YouTube's bot program. The stress suite ran whole (19 phases):
one finding, which was the phase expecting a download to hang (T-011).
`cargo audit` on both workspaces: no advisories. UI files changed: one
status gained its bytes and one failure sentence names both ways out, as
row 35 says, in all thirteen catalogs.

## QA before merging the yt-dlp rows (2026-10-29)

- `scripts/human-qa.py` whole, in sight: 36 of 38 the first time; the two
  were B33 (the panel left in the player's old corner when YouTube re-laid
  its page under load) and B36 with it. The cause was in the extension
  (BUG-006), fixed, and the browser rows run again on the fixed build:
  the local browser check 70 of 70 (a new check shrinks the player and
  expects the panel to follow; 69 of 70 without the fix), real YouTube in
  Edge 49 of 49, every file type 52 of 52, Firefox as before. The 25 window
  rows held in the first run and nothing in `desktop/` changed after it.
- The Docker QA stack on the changed engine: 37 of 37 scenarios, 69 of 69
  browser checks. `cargo test --workspace` (desktop and browser), clippy,
  formatting, the privacy and layering checks: clean. The stress suite:
  19 phases, no finding after T-011.

## Next

- Every engine behaviour reference A documents has a match, or a written
  difference (Kerberos), in [feature-matrix.md](feature-matrix.md).
- The two candidates in feature-matrix.md "Worth taking" wait for the
  maintainer's decision; neither is applied, as each changes existing behaviour.
- A side-by-side run of reference A needs a Windows machine (the maintainer's
  laptop, once it can be reached).

## QA before merging the security fixes (2026-10-30)

- `scripts/human-qa.py` whole, in sight, twice on the same build (dm follows
  redirects itself, a row's secrets beside the queue file, the host checks
  the extension id): the first run 36 of 38, B01 and B25 failing because the
  clipboard held a copied link and Add URL offered it, as it should (the
  script now empties the clipboard for its walk and puts it back); the
  second 36 of 38, B33 and B36 failing on real YouTube with the panel 51 px
  past the player's right edge after the page re-laid itself (BUG-006's
  shape again, after its fix), which held in the first run. Every row held
  in one of the two runs; B33 is open again in `BUGLOG.md`.
- The B33 failure was chased with a timeline of the real page (the player's
  and the panel's boxes every 100 ms from the hover): the player lays out
  twice, and between the two it moves 8 px left and 12 px up without
  changing size, which nothing followed (BUG-007, fixed: a shown panel looks
  every 200 ms whether its player moved); the check was also judging the
  panel against a box measured before the hover (now against the box as it
  is). With the fixed extension: the local browser check 71 of 71 (70 of 71
  before the fix, the new check failing), real YouTube in Edge 49 of 49
  twice, and `scripts/human-qa.py` whole, in sight: 38 of 38.

## Measured on the release build, macOS (2026-10-30)

`scripts/stress-check.py throughput concurrency edge` and
`scripts/launch-latency.py`, release build, after the security fixes:

- Throughput, 256 MB on loopback: 221 MB/s on 1 connection, 218 on 8, 231
  on 16; peak 19-21 MB resident, byte-exact each time.
- 150 files of 4 MB at once, 8 connections each, concurrency 32: all 150
  complete in 8 s, 56 connections at the server's peak, 45 MB resident.
- Microsoft Edge, 40 real clicks 80 ms apart: 40 of 40 in dm within 5 s,
  byte-exact, Edge's own list empty; a small file from a server holding
  each connection to 1 MB/s in 1.21 s on 9 connections; dm's first request
  120-212 ms after the click (median 209 ms, through the doorbell).
- From `download.start` to the server's first request: 50 ms with dm
  closed (the host starts it), 47 ms with dm running (medians of five).
- A ten-minute YouTube video (181 MB at 720p), release build, headless: 16.1 s
  and 14.1 s with the slice cap from yt-dlp's reading, 44.2 s twice without
  it (`DM_YTDLP_NO_CHUNK`); gaps.md row 22 closed.
- `scripts/real-sites.py` on the release build: 13 of 13 sites as expected
  in 30 s (YouTube through yt-dlp's reading, GitHub raw, Apple, Wikimedia,
  an HLS test stream, a Creative Commons torrent; Reddit and a signed-in
  Vimeo refused with a reason and nothing saved).
- `scripts/human-qa.py` after these: a run straight after the full test
  suite lost its first click (Add URL highlighted, no dialog; egui takes a
  release more than 0.8 s after its press for a hold, and the press and
  release went a frame apart), so `ui.click` now delivers both in one
  frame; the run after that 38 of 38.
- Reported: ".txt and .bin sometimes still download in Edge instead of
  dm". Verified with 40 real clicks in Edge, dm set as installed (every
  type): tiny, medium and big `.txt` and `.bin`, three rounds each, a
  redirect, a `download` link, a server refusing dm's own agent, a wrong
  content type, an inline `.txt` and eight clicks within a second all went
  to dm (36 of 40). Left with the browser, each with its notice: a server
  that did not answer dm's probe within 10 s (12 s and 25 s delays, both
  types) and a `.txt` made on the page (a blob, which only the browser
  holds). The probe's budget is now 16 of the extension's 20 s.
- `scripts/stress-check.py` whole on the release build (2026-10-30): no
  finding; 2 000 small files at 735 files/s, 150 × 4 MB at once in 8 s, a
  kill with 24 unfinished and all 24 whole after the restart, the pause
  storm with 0 control errors, every hostile name contained.

