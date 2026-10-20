# After (Phase 2), 2026-10-20

Each change measured against the commit before it, on the same Mac and the
same way as `qa/baseline.md`, old and new builds run in turn on one quiet
machine with room on its disk.

| Change | Metric | Before | After | Change |
|---|---|---|---|---|
| Ranges written in place, finishing a rename (gap 16) | 2 GB over loopback, 8 connections, launch to Completed (6 runs each) | 2.5–5.5 s, median about 3.6 s | 1.7–2.5 s, median about 2.0 s | about −45 % |
| same | same, 1 connection | median about 1.4 s | median about 1.4 s | none (one part was already renamed, never copied) |
| same | bytes written to disk for a file of N bytes on more than one connection | 2N (fetched into parts, copied into the file) | N | −50 % |
| An idle window waits to be told of a change rather than looking every second (the queue calls back on every change) | idle CPU, window open, 60 s, nothing downloading (old 2 runs, new 5) | 0.45–0.47 % | median 0.17 % (0.13–0.38, one outlier at 1.48) | about −63 % |
| same | wakes a minute, window open and idle | 60 | 2 (and one for each change to the queue) | −97 % |
| The computer's fonts for other scripts mapped, not read whole | window memory after a row named in Japanese is drawn (3 runs each) | +35.0 to +36.8 MB (138–140 MB) | +15.0 MB (118 MB) | −20 MB |
| The panel's WebAssembly loaded only where there is media (gap 73) | Chromium renderer, page of 20 plain frames: WebAssembly loads / private dirty memory | 21 / 55.9 MB | 0 / 30.2 MB | −26 MB |
| same | same, a page with one video | 1 / 15.1 MB | 1 / 15.2 MB | none |

| Against curl (the second opinion), 2 GB over loopback to disk, same session | | curl 2.19–2.39 s | dm 1.45–1.75 s on 1 connection, 1.82–2.21 s on 8 | dm at or above curl |
| 10 downloads at once, CPU (after the repaint fix 48d3a13) | % of one core, 30 s | 9.7–11.7 % | 8.9–9.1 % | about −15 % |

Notes:

- The baseline's throughput rows (2.6–3.2 s and 11.2–13.8 s for 2 GB) were
  taken on a disk with little room left, which slowed every write; the
  figures above are both builds on the same, emptier disk, and the
  baseline's are not compared with them.
- The fetch-then-finish gap that remains (0.3–1.0 s on 8 connections) is
  the rest of finishing: the queue noticing, the file renamed and dated,
  the row saved.
- The Chromium figures are from the Linux container (Chromium 155, the
  real dm as the native host), median of 3 fresh browsers.

## Startup (bottleneck 4), split, 2026-10-20

Timed inside a release build with temporary marks (removed again), three
launches on a fresh home, this Mac:

| Stretch | Time |
|---|---|
| Launch → settings, queue and client built (`shell::start`) | 13–66 ms (the first launch cold) |
| → handed to eframe | 6–19 ms |
| eframe making the window and its GL context, before dm's code runs again | 290–320 ms warm, about 1 s cold |
| tray, browser check and dm's state | about 10 ms |
| first `ui()` | 4–7 ms |

dm's own work before the first frame is about 30–40 ms; the rest is the
system making a window with a GL context. Moving the tray or the browser
check after the first frame would save under 10 ms. The wgpu renderer
(Metal) was tried in place of glow: the same 320 ms and the same 102 MB,
with a larger binary, so glow stays. Linux reaches the first frame in
0.13–0.18 s. The 300 ms target is not met on macOS and is left as eframe's.

## Memory as the system counts it, 2026-10-20

The baseline's memory rows are resident set size, which on macOS counts
the system frameworks every windowed program maps and shares (AppKit, the
GL driver's code). `vmmap` puts 355 MB of resident `__TEXT` and 35 MB of
`__OBJC_RO` in such a process; none of it is dm's. What Activity Monitor
shows is `phys_footprint` (`footprint <pid>`):

| State | RSS | phys_footprint |
|---|---|---|
| Idle, window open | 103–105 MB | 43 MB |
| Idle, `--hidden` (in the tray) | 101 MB | 43 MB |
| Idle, `--headless` | 19 MB | 4 MB |

So the window costs about 39 MB, not 84, and the idle target (< 60 MB) is
met. A window made only when first shown would save those 39 MB for a
login launch; eframe can be driven from dm's own event loop
(`eframe::create_native`), not done yet.

Ten downloads of eight connections each (part 4 of the harness, 2 runs
each, footprint):

| Write buffer per connection | Window | Headless | 2 GB loopback, 8 conns, time / CPU |
|---|---|---|---|
| 256 KiB (before) | 160–180 MB | 126–136 MB | 2.01–2.39 s / 2.24–2.54 s |
| 64 KiB | 164 MB | 110 MB | 1.99–2.34 s / 2.39–2.61 s |
| 16 KiB (now) | 146–147 MB | 88–107 MB | 2.03–2.07 s / 2.50–2.57 s |

tokio's file keeps its own copy of each write, so every connection held
the buffer twice; the heap was 108 MB of a 126 MB headless footprint. The
rest is the HTTP client's read buffers, about 1 MB a connection.

## A login launch with no window, and threads after downloads, 2026-10-20

| State | Before | After |
|---|---|---|
| `--hidden`, idle in the tray, phys_footprint | 43 MB | 13 MB |
| same, CPU over 60 s | — | 0.07 s (0.12 %) |
| shown from there (second launch) | — | 42–43 MB, window drawn |
| threads a minute after ten 60 MB downloads (headless) | 40 | 13 (14 before they began) |

The window is made the first time it is asked for; until then dm's logic
runs on a stand-in context woken by what wakes the window. On Linux the
QA run's 100 threads and 200+ MB after a soak are expected to fall with
the thread count: each thread kept a glibc malloc arena.

Linux (Debian 12 container, release build, headless, the same ten
downloads): 12 threads at launch, 27 as they finished, 15 within 20 s of
idle and staying there; 24 MB resident. QA loop 1 saw 100 threads that
never left, on the build before.
