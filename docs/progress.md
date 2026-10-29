# Progress: replicating IDM and FDM behaviour in dm's engine

Checkpoint, 2026-10-29. The work list stays [gaps.md](gaps.md); this page
records the reference investigation only.

## Baseline

- Branch `qa/docker-loop`, clean at `4c51c8f` before this work.
- Protected UI: `desktop/crates/dm-desktop/` (see [architecture.md](architecture.md)).
  `git diff --stat -- desktop/crates/dm-desktop` is empty after every change.
- Baseline: `cargo test --workspace` passes (desktop and browser); the Docker
  QA loop passed 3 runs in a row (37/37 scenarios, 69/69 browser checks).

## Done

1. **Connection reuse (IDM, Verified).** Established dm's behaviour with a new
   test, `dm-core/tests/reuse.rs`: 11 requests on 4 TCP connections. Already
   a match; no engine change needed.
2. **A page's only video named without "(1)"** (gaps.md row 31, now closed).
   Cause found in yt-dlp's `HTML5MediaEmbed` reader, which numbers every
   entry. Core change: `ytdlp/fetch.rs` `unnumbered`. Tests: three unit cases,
   and the QA `video_page` scenario, which failed before the change
   ("QA clip (1) [watch？v=ok-1].mp4") and passes after ("QA clip [watch？v=ok].mp4").
   UI files changed: none.

## Commands run

- `cargo test --workspace` in `desktop/` and `browser/`: pass.
- `cargo clippy -p dm-core --all-targets -- -D warnings`, `cargo fmt --all`: clean.
- `QA_SCENARIOS="video_page video_page_without_media hls_streams" sh qa/run.sh`: 3/3.
- `sh qa/run.sh` (full): 37/37 scenarios, 69/69 browser checks.

## Next

- Every engine behaviour IDM documents now has a match, or a written difference (Kerberos), in
  [feature-matrix.md](feature-matrix.md). The differences left are in
  [known-differences.md](known-differences.md); none can be closed without
  running the reference itself or changing a visible setting.
- A side-by-side run needs FDM installed (macOS) or IDM on a Windows machine.
