# The benchmark: dm against reference A's frozen baseline

Reference A was measured beside dm on one Windows laptop on 2026-11-09 and 2026-11-10: 84 runs
over seven scenarios, every parameter written down. Its numbers are frozen in
[reference-a-baseline.json](reference-a-baseline.json). They are the **bottom line** for dm on
every system: dm must beat every judged parameter, on Windows, macOS and Linux. The baseline
never changes. Only dm has to get better.

Reference A runs only on Windows, and only on the maintainer's computer has it been measured.
Elsewhere dm runs the same harness alone and `compare.mjs` judges it against the frozen numbers.
So work on macOS or Linux can aim at the same target as work on Windows.

## Files

| File | What it is |
|------|------------|
| `reference-a-baseline.json` | The frozen baseline: the machine, both apps' settings, the harness's every parameter, the floors, the ceiling and all 84 of reference A's runs. **Never edited.** |
| `bench.mjs` | The harness: the local server, the scenarios, the sampler. It drives dm (and reference A where `local.json` names it). |
| `compare.mjs` | Judges a results folder against the baseline, parameter by parameter. Exit 0 only when dm is better everywhere. |
| `ceiling-client.mjs` | The reference client, used by the `ceiling` scenario to measure what the computer itself allows. |
| `refa-windows.ps1` | Answers reference A's windows during a run (Windows only). |
| `local.example.json` | The shape of `local.json` (git-ignored), which names reference A's program on the computer that has it. |
| `results/` | Your runs, one folder per system and date (git-ignored). |

## Run it

Use a release build. The baseline was measured against one, and a debug build is several times
slower.

```sh
cd desktop && cargo build --release -p dm-gpui      # dm-desktop on Linux
cd ..
node bench/bench.mjs                                 # idle fast per_conn flaky outage batch big40 ceiling
node bench/compare.mjs                               # judges the newest folder in bench/results
```

- **Any system, dm alone** (the usual case): the commands above. `DM_BINARY=/path/to/dm` runs an
  installed or packaged dm instead. The whole run takes about 5 minutes.
- **One scenario while you work on it**: `node bench/bench.mjs ceiling flaky`, then
  `node bench/compare.mjs`. A machine-bound scenario (`fast`, `big40`) is judged only when
  `ceiling` was run on the same computer into the same results folder.
- **16 connections**: `BENCH_DM_CONNECTIONS=16 node bench/bench.mjs per_conn flaky fast`. Those
  results are judged against the `connections16` group.
- **Beside reference A** (Windows, the maintainer's computer): copy `local.example.json` to
  `local.json` and fill it in. `bench.mjs` then runs both apps in turn. Each repetition swaps which
  goes first, and `compare.mjs` prints reference A's number from the same run beside each row.
  Those numbers are not the floor: the floor stays the frozen one.
- **The baseline's own layout** (Windows): dm's home on the SSD (the default, under the temp
  folder), downloads on the hard disk:
  `BENCH_DM_DOWNLOADS=D:\dm-bench\down-<n> BENCH_REFA_DIR=D:\dm-bench\refa-<n> BENCH_CEILING_DIR=D:\dm-bench\ceiling node bench/bench.mjs`.
  Use a fresh download folder each run: names repeat between runs, and a second copy saved as
  `x (1).bin` reads as not whole.

`node bench/compare.mjs --json` prints the same judgement for a program to read. Each parameter
has its floor, dm's median, best and worst, and the verdict.

## How dm is judged

1. **Every file whole.** Each run's file is compared by SHA-256 with what the server served. A run
   that ends otherwise fails the scenario.
2. **Every parameter with a direction is beaten.** dm's median over its runs must be lower than
   reference A's floor. Equal is not better ("faster, not just close"). The one exception is a
   floor of 0, which only a 0 can meet.
3. **A floor is reference A's best.** For each scenario and group, a floor is the best value
   reference A reached in any whole run (the median and worst are in the file too). It is the
   strictest bar reference A has set. A run slowed by the harness could only have made a floor
   easier, never harder.
4. **Machine-bound floors follow the computer.** `fast` (no cap) is bound by the computer's
   disk, memory and CPU. On the baseline laptop so is `big40`: Windows' 15.6 ms timer holds the
   paced server near 19 MiB/s, under its 40 MiB/s cap. For these two, `seconds`,
   `transferSeconds` and `lastByteToDoneMs` are scaled by
   `min(1, ceiling here / ceiling on the baseline laptop)`, each the best of its runs. On a faster
   computer the floor gets stricter, and it never gets looser than it is here.
5. **Everything else applies as it is.** The capped scenarios are bound by the server's caps, not
   the computer, and memory and CPU are bars to come in under. On macOS the sampler cannot read
   private bytes, so `peakPrivateMB` is "not sampled here" there.
6. **Recorded, not judged:** throughput (judged through seconds), requests, connections, the most
   connections or files at once, bytes served, requests dropped in the outage. They show how an
   app goes about it. Reference A's are worth reading. In the outage it sent about 9,000 requests
   in 15 s, dm about 100.

## The scenarios

The server (`bench.mjs`) is part of the standard. It is a Node HTTP server on 127.0.0.1 in the
harness process, which answers ranges (`bytes=a-b`, `a-`, `-n`) with 206, sends an ETag and a
Last-Modified, and writes in pieces of 1 MiB uncapped and 64 KiB capped (never under 16 KiB).
Caps are token buckets: 128 KiB banked per connection, 256 KiB for a shared total. The bytes are
a seeded xorshift block with each 4 KiB stamped with its MiB, so a piece in the wrong place
changes the digest. **Do not change the server, the data or a scenario**: that makes dm's numbers
incomparable with the baseline. Add a scenario instead. The full description of each is in the
baseline's `harness` and `scenarios`.

| Scenario | What the server does | Bound by | Ideal |
|----------|---------------------|----------|-------|
| `idle` | nothing: the app's window is up for 20 s | - | - |
| `fast` | 512 MiB, no limit | the computer | - |
| `per_conn` | 64 MiB, each connection 1 MiB/s | the caps | 8 s on 8 connections (fewer with more) |
| `flaky` | 64 MiB, 8 MiB/s shared, every connection cut after a random 0.25 to 3 MiB | the caps | 8 s |
| `outage` | 64 MiB, 8 MiB/s shared; at 30% every connection closed and every request refused for 15 s | the caps | 23 s |
| `batch` | 24 files of 2 MiB added at once, each connection 2 MiB/s | the caps | 3 s at 8 connections |
| `big40` | 512 MiB, 40 MiB/s shared | the computer here, the cap elsewhere | 12.8 s |
| `ceiling` | the reference client, 8 ranges into 8 files, per scenario | the computer | - |
| `inet` | two public 100 MB files | the network | never a floor |

## The floors (group `standard`)

Reference A's best in any whole run. In this group both apps use 8 connections (their default),
dm's home is on the SSD (C:), both save to the hard disk (D:), and reference A keeps its parts on
C:. The laptop has an Intel Core i5-6200U (2 cores, 4 threads), 11.9 GB of memory and Windows 11.

| Parameter | `idle` | `fast` | `per_conn` | `flaky` | `outage` | `big40` |
|-----------|-------:|-------:|-----------:|--------:|---------:|--------:|
| seconds (address handed over to file whole) | | 3.682 | 9.446 | 8.762 | 23.822 | 31.785 |
| transferSeconds (first request to file whole) | | 3.138 | 8.942 | 8.236 | 23.312 | 28.337 |
| firstRequestMs | | 486 | 504 | 506 | 495 | 3448 |
| lastByteToDoneMs (the finish) | | 795 | 159 | 71 | 218 | 1060 |
| refetchedPct (fetched twice) | | 2.6 | 0.1 | 0.8 | 0.8 | 1.7 |
| peakWorkingSetMB | 39.2 | 20.9 | 21.1 | 21.1 | 21.6 | 39.6 |
| peakPrivateMB | 12.3 | 10.7 | 10.7 | 10.6 | 11.6 | 17.1 |
| cpuMs | 234 (in 20 s) | 2953 | 1281 | 1406 | 14203 | 5359 |
| recoveryAfterOutageMs | | | | | 0 | |
| reference A's whole runs | 1 | 39 | 8 | 13 of 14 | 4 | 2 |

The ceiling on the baseline laptop, best of 6 runs (2026-11-11), in seconds: `fast` 2.601,
`per_conn` 8.025, `flaky` 7.991, `big40` 26.878.

The other groups are `connections16` (both apps at 16 connections: `per_conn` 7.142 s, `flaky`
8.753 s, `fast` 8.468 s), `allSsd` and `allSsdConnections16` (everything on the SSD, `fast`
only). `batch` has no floor: reference A finished 3 and 4 of the 24 files in the 300 s allowed
when its queue was started from its command line, which says more about the harness than about
reference A (`docs/gaps.md` row 172). Until a fair run of it is frozen, dm's batch is judged
against its ideal.

## Where dm stood when it was frozen

On the same laptop, with dm's release build of 2026-11-10 (the egui window), `compare.mjs` passed
dm on end-to-end speed in `fast` (2.37 s against 3.68), `per_conn` (8.62 against 9.45), `flaky`
(8.71 against 8.76) and `outage` (23.64 against 23.82). It was worse in 22 parameters, each a row
in `docs/gaps.md` (171, 175 to 177):

- memory: 80 to 91 MiB working set while downloading against about 21, and 66 to 77 at rest
  against 39
- CPU over capped connections: about twice reference A's in `per_conn` and `flaky`
- the finish after the last byte: 0.4 to 0.6 s against 0.07 to 0.2 in the capped scenarios
- bytes fetched twice under cuts and the outage (1.8 to 2.3% against 0.8)
- time from the first request in `flaky` and `outage`
- 70 ms to resume after the outage against reference A's 0

## Changing the baseline: never

`cargo test --workspace` fails when `reference-a-baseline.json` differs by a byte from what was
frozen (`desktop/xtask/src/baseline.rs`, `the_reference_a_baseline_never_changes`), and
`compare.mjs` refuses to judge against a changed one. Both pin its SHA-256
(`113366e4f1b8c4c1fca96c4285fcbfd706f6d72a7b8cc67fcb88b75447f568f8`, with `\n` line endings).

A new measurement of reference A (a fair `batch` run, a new scenario, another computer) goes into
a new file beside this one, pinned the same way, with its own floors. The floors in this file
stay.

## Learnt the hard way

- **Never block the harness's event loop.** The server runs in the same process. A `spawnSync`
  made reference A look 3 to 5 times slower than it was on 2026-11-09, and kept `batch` from ever
  finishing until 2026-11-10. Window helpers and samplers are spawned asynchronously.
- **The window helper runs only until the first request.** Kept running all through a download,
  it took a core of the 2-core laptop from whichever app was measured.
- **The hard disk is noisy.** The same reference A took 3.7 to 14.5 s for `fast` on different
  days, depending on what Windows still had to write. Keep the download disk idle, and do not
  judge a change by one run: `BENCH_REPS=5`.
- **Not curl for the ceiling.** On Windows, the curl first on Git Bash's PATH is a debug build,
  and even Windows' own wrote 512 MiB in 21.6 s where reference A took 3.7. The reference client
  writes through 4 MiB buffers in a process of its own.
