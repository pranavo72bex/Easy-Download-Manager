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
