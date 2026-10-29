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

## Next

- Every engine behaviour reference A documents has a match, or a written
  difference (Kerberos), in [feature-matrix.md](feature-matrix.md).
- The two candidates in feature-matrix.md "Worth taking" wait for the
  maintainer's decision; neither is applied, as each changes existing behaviour.
- A side-by-side run of reference A needs a Windows machine (the maintainer's
  laptop, once it can be reached).
