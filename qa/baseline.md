# Baseline (Phase 1), 2026-10-20

Measured before any optimisation, on this Mac (Apple Silicon, macOS 26),
release build of `dm-desktop` at commit c84913d (`lto = true`,
`codegen-units = 1`, `strip = true`). Every run used a throwaway dm home and
a local HTTP server that answers ranges, driven through dm's control
protocol by `scratchpad/baseline/measure.py` (kept outside the repo). Times
are wall clock; memory is resident set size (`ps -o rss`); CPU is process
CPU time over the window, as a percentage of one core.

| Metric | How measured | Result | Target |
|---|---|---|---|
| Launch → control protocol up | poll for `control.port`, 3 runs | 0.37 / 0.43 / 0.81 s (first run cold) | — |
| Launch → first frame drawn | first `ui.screenshot` answered, 3 runs | 0.42 / 0.52 / 0.96 s | < 300 ms |
| Launch → list with 500 finished rows drawn | same, home with 500 rows of history | 0.50 / 0.51 / 0.66 s | < 600 ms |
| Idle memory, window open | 5 s after launch, empty list | 103.5 MB | < 60 MB |
| Idle memory, no window (`--headless`) | same | 19.5 MB | — |
| Memory with 500 rows of history | 3 s after the window drew them | 153 MB | — |
| Memory, 10 downloads at once | peak over 30 s, 10 × 300 MB at 4 MB/s each | 213 MB | < 150 MB |
| Idle CPU, window open | 60 s, nothing downloading | 0.58 % | ~0 % |
| Idle CPU, no window | 30 s | 0.20 % | ~0 % |
| CPU, 10 downloads at once | 30 s, about 40 MB/s in all | 8.5 % of one core | < 5 % of 4 cores |
| UI frame time, 500 rows | `five_hundred_rows_draw_inside_a_frame_budget` (debug build, CI) | within its budget; not measured in a release build | < 16 ms |
| Throughput, 2 GB over loopback, 1 connection | to disk; curl to disk as the second opinion | dm 2.6–3.2 s; curl 3.4–4.3 s | ≥ 95 % of the link |
| Throughput, 2 GB over loopback, 8 connections | same | fetched in 4.4–5.0 s, **finished in 11.2–13.8 s** | ≥ 95 % of the link |
| Release binary | `ls -l target/release/dm-desktop` | 20.8 MB | as small as practical |
| Duplicate crate versions | `cargo tree -p dm-desktop -d` | 49 crates in two or more versions (two `reqwest`, three `dirs`, three `getrandom`, three `hashbrown`…) | — |

Notes on the method:

- The test server is Python and serves parallel connections one at a time
  under its interpreter lock, so 8 connections fetch slower than 1 here.
  That is the server, not dm; a link that a single connection cannot fill
  is where 8 connections earn their keep, and that was measured earlier
  (within 1–2 % of a capped link's ceiling).
- The 8-connection row's gap between "fetched" and "finished" is dm
  joining its part files into the finished file: every byte written a
  second time (gap 16).
- Not measured here: Windows (dm has never run on it, gap 2), 125 % and
  other display scales on Windows, a second monitor, Tracy frame traces.
  The Linux numbers come from a container run, reported separately.

## Top 5 suspected bottlenecks (before any change)

1. **The join copies every byte of a multi-connection download.** Parts
   are fetched into their own files and copied into the finished file at
   the end: 7–9 s of a 11–14 s download of 2 GB, about 60 % of the time on
   a fast link, and twice the disk writes. Writing each range at its own
   offset into one preallocated file would remove the copy; the resume
   plan, the per-part check of the last 64 KiB and the join's integrity
   checks would have to move with it (gap 16).
2. **The window costs 84 MB of the 103 MB idle.** Headless dm is 19.5 MB.
   Suspects: the GL context and driver (`glow`), egui's font atlas and the
   fonts loaded with it (`default_fonts` brings egui's own faces and emoji
   besides Inter), and texture memory. To be split with a heap profiler
   and the driver's share.
3. **About 100 KB per finished row.** 500 rows of history add 50 MB.
   Suspects: per-row cached text layouts, the per-place row cache, and
   what each `Entry` carries. To be measured with dhat.
4. **Work before the first frame.** 0.4–0.5 s warm to the first frame:
   settings, queue and HTTP client built while blocking (`shell::start`),
   the login item set to match the setting, the tray built, and the
   browsers' host manifests checked, all before the window draws. The last
   three could follow the first frame.
5. **CPU per byte while downloading.** 8.5 % of a core for about 40 MB/s
   across 10 downloads, and 0.6 % idle with the window open (a repaint
   each second). Suspects: buffer size and per-chunk work on the hot path
   (progress, limiter), the repaint while idle, and the join's copy. To be
   profiled with samply.

The binary's size (20.8 MB) and the 49 duplicated crates come after these:
they cost disk and download time, not memory or CPU at run time.
