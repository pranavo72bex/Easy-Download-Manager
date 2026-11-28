# Testing EDM

What checks EDM, what each one proves, and how to run it. Every command is
run from the repository's root unless it says otherwise. What is currently
proven, behaviour by behaviour, is [parity.md](parity.md); what is not yet
is [gaps.md](gaps.md).

## The rule

**Wired is not working.** A control that calls a command proves the call,
not the download. A claim in these docs is backed by a test or a recorded
run that checks the outcome: bytes on disk, a hash, a state that survives a
restart. Never record a run that did not happen.

## On any machine with Rust

```sh
cargo test --workspace                          # engine, protocol, shell and window
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo xtask scorecard | layers | privacy | secrets
cd crates/edm-catcher && cargo test                     # the extension's decisions, on the host
```

- The engine's tests run against a real HTTP server on loopback and a real
  queue, and a loopback BitTorrent swarm. The GPUI window, the one shipped, is
  opened in GPUI's test platform over a real queue and driven through the
  control port's methods (`crates/edm-gpui/src/harness.rs`; the tests in
  `src/window_tests/`, grouped by what they check: `cargo test -p edm-gpui
  window_tests`), and driven whole by `scripts/human-qa.py` and
  `scripts/app-check.py`. The older egui window's tests were ported to it,
  file by file, before that window was removed (gaps row G9).
- Every reader of outside input (names, headers, playlists, archives, the
  control protocol) has a property test in `tests/fuzz.rs`; `PROPTEST_CASES=200000
  cargo test --release -p edm-core --test fuzz` hunts deeper.
- Coverage-guided fuzzing (gaps row H11): `fuzz/` holds a libFuzzer target per reader
  (torrent files, HLS, DASH, Metalink, archives, a ZIP's list, MP4, YouTube's framed stream,
  the JavaScript interpreter). `cd fuzz && cargo +nightly fuzz run archives` runs one;
  `.github/workflows/fuzz.yml` runs each for ten minutes every night, its corpus kept from
  night to night. A crash's input is uploaded; once fixed it becomes a test in the reader's
  own tests. Without a C++ compiler, `cargo check --no-default-features` in `fuzz/` checks the
  targets build.
- `cargo test -p edm-core --test zoo -- --nocapture` runs the server zoo
  ([zoo.md](zoo.md)); `EDM_RAISE_FLOOR=1` raises its floor after a fix.
- `cargo xtask scorecard` rewrites [scorecard.md](scorecard.md) from
  [parity.md](parity.md) and [zoo.md](zoo.md); a test fails if it is stale.
- Tests use their own temporary directories and never touch a real
  Downloads folder or browser profile. A password or token in a test comes
  from `edm_test_support::secrets` (`crates/edm-test-support`), never
  from the code.

## The testing plan, and where each part lives

The maintainer's plan (2026-10-29) in six parts, and what answers each:

1. **Unit tests**: `cargo test --workspace` from the repository's root (the engine's
   ranges, retries, pause and resume, names, speed limits, the queue; 1 250
   tests against real servers on loopback) and `crates/edm-catcher/` (the extension's
   decisions). Property tests in each crate's `tests/fuzz.rs`.
2. **Integration**: the Docker QA stack (`sh qa/run.sh`: 37 scenarios on
   servers that misbehave, 69 browser checks), the browser checks in real
   Chrome, Edge and Firefox (`chrome-check.py`, `firefox-check.py`), the
   real-site runs (`real-sites.py`, `youtube-check.py`), restart and kill
   recovery in the contract (B21, B22) and the stress suite.
3. **Test first**: AGENTS.md "Rules for a change" 1 (reproduce first with a
   failing test) and BUGLOG.md, where every fix names the test that failed
   before it.
4. **Security testing**: an authorized agent's black-box run and code
   reading, reported in `docs/security-report.md` when one is made; the
   privacy check (`cargo xtask privacy`); the `names`, `hostile` and
   `control` phases of the stress suite; `cargo audit` on both workspaces.
5. **Review and fix**: findings go to gaps.md as rows, critical and high
   first, each closed with a regression test (BUGLOG.md).
6. **Load, stress and reliability**: `python3 scripts/stress-check.py`
   (19 phases: thousands of small files, 20 000 queued links, a 1.5 GB
   file, cuts, stalls, lies, outages, kill -9, a pause storm, limits,
   leaks, the window under load), measured and printed.

## QA: the behaviour contract, whole

`python3 scripts/human-qa.py` goes through every row of
[behaviour-contract.md](behaviour-contract.md) as a person would: EDM's real
window driven through its own buttons and dialogs against a local server,
then Microsoft Edge with the extension (`youtube-check.py` on a real video,
and `every-type-check.py`: every kind of file goes to EDM with EDM set as
installed). In sight, never headless; about half an hour. It fails when any
row fails or was not checked, and `--save-baseline` records the window's
pictures once a change to them is approved. This is what "QA" means here.
On Windows, start it from PowerShell, not Git Bash (next section).

**Twenty everyday tasks, counted** (gaps row W3). `python -X utf8 scripts/task-steps.py`
does each task of [tasks.md](tasks.md) marked "driven" in EDM's real window, through the
control port as a person would, each in an EDM of its own as a new install has it, counts its
clicks, keys, typed fields and windows, and checks the task was done; the browser's tasks are
counted by hand from their path. With reference A's counts filled in by a person, it lists each
task where EDM takes more, a row to propose in gaps.md. It is a measure, not part of QA: in sight,
from PowerShell on Windows, like `human-qa.py`.

**On CI, reported and not gating** (`.github/workflows/contract-linux.yml`, gaps row R5). On
each pull request, each push to main and by hand, CI's Linux runner runs `sh
scripts/linux-human-qa.sh` exactly as it runs on a machine with Docker: a build of the checkout in
a Debian 12 container, `human-qa.py` on an X display nobody watches, Chromium in Edge's place.
The run's artifact (`contract-linux`) holds human-qa.py's words (`human-qa.txt`), its full
record and the screen pictured every ten seconds; `scripts/human-qa-report.py` writes the rows
held, failed and not checked into the run's summary, a warning for each failed row. A failed row
does not fail the workflow (the job and its QA step are `continue-on-error`). It does not replace
the QA above: the gate before a merge is still a person's run of `human-qa.py`, in sight, and
nothing here changes its rules or the contract. `python3 scripts/human-qa-report.py --self-test`
checks the counting.

## On Windows: what goes wrong, and what to do

What a Windows computer does differently, met on the maintainer's laptop and
written here so the next person need not meet it again (gaps row X4):

- **UTF-8 is set by each script.** Every Python script under `scripts/` and
  `qa/` sets UTF-8 for its own output, for the files it reads and writes, and
  for what it reads from the programs it runs, so plain `python` is enough;
  `python -X utf8` is no longer needed. Without it, Windows' code page
  (Windows-1252) turned EDM's "—" and "›" into "â€”", and `human-qa.py` read
  nine browser rows as "not run" (2026-11-11). A new script does the same:
  the `_stream.reconfigure(encoding="utf-8", errors="replace")` lines after
  its imports, and `encoding="utf-8"` on every `open`, `read_text`,
  `write_text`, socket `makefile` and text-mode `subprocess` call. A
  PowerShell command whose output is read starts with
  `[Console]::OutputEncoding = [System.Text.Encoding]::UTF8;`.
- **QA from PowerShell, not Git Bash.** Started from Git Bash (or another
  sandboxed shell), EDM's window is pictured solid black: B25's empty window
  differed by 99.55 % (2026-11-14), from PowerShell by 0.02 %. `human-qa.py`
  now looks at its first picture and, if it is one colour all over or did
  not come back, stops there and says so, rather than reporting rows it
  could not see. Start it from PowerShell or Windows Terminal, naming the
  build when `CARGO_TARGET_DIR` put it outside `target`:

  ```powershell
  $env:EDM_BINARY = "D:\edm-target\debug\edm-gpui.exe"
  python scripts/human-qa.py
  ```
- **`CARGO_INCREMENTAL=0` for a debug build.** The window's incremental
  cache grows with every rebuild until the disk is full. Turn it off, and put
  the build on a drive with room, before running cargo:

  ```powershell
  $env:CARGO_INCREMENTAL = "0"; $env:CARGO_TARGET_DIR = "D:\edm-target"
  ```
- **Packages: `cargo xtask package`.** `cargo packager` finds its settings
  only in the window's own folder (`crates/edm-gpui/Cargo.toml`,
  `[package.metadata.packager]`): from the checkout's root it found none,
  and in the workspace without `-p` it packaged every crate that has them.
  `cargo xtask package`, from the repository's root, runs it in that folder
  for `edm-gpui` with this system's formats (NSIS on Windows, `.app` and
  `.dmg` on macOS, `.deb` and AppImage on Linux); anything after `package`
  goes to cargo-packager (`--formats`, `--target`, `--out-dir`). With
  `CARGO_TARGET_DIR` set, the packages go to `<it>\release\bundle`. It needs
  `cargo install cargo-packager --locked`; a release build takes about
  25 minutes on a 4-core laptop.

## Real sites, beside reference A

[site-list.md](site-list.md) lists the sites reference A is known to work on (the maintainer's list, 2026-11-05),
with what to try on each and columns for what reference A and EDM did. A person runs it in the
final human QA, in Microsoft Edge with both extensions; a place where EDM does less is a row in
`gaps.md` (row 152).

## Speed, memory and CPU against reference A's frozen floor

[`bench/`](../bench/README.md) holds the side-by-side benchmark. Reference A's runs of 2026-11-09
and 10 are frozen in `bench/reference-a-baseline.json` as the floor of every parameter, and a test
(`the_reference_a_baseline_never_changes`) keeps that file from changing. `node bench/bench.mjs`
runs EDM's release build on Windows, macOS or Linux (and reference A as well, on the computer that
has it), and `node bench/compare.mjs` judges each parameter. It exits 0 only when EDM is better in
all of them. A parameter where EDM is worse is a row in `gaps.md`.

**Weekly on CI, against EDM's own last numbers** (`.github/workflows/bench.yml`, gaps row R5).
Every Tuesday and by hand, a Linux runner builds EDM's release, runs `bench.mjs`'s scenarios (all
but `inet`, `BENCH_REPS=3`) with EDM alone under Xvfb, and `node bench/against-last.mjs <now>
<last>` compares each parameter's median with the last passing run of the workflow on the same
branch (its `bench-EDM` artifact, kept 90 days). A parameter is worse when it is over the last run
by more than 20% plus the harness's slack (0.25 s, 150 ms, 0.5 point of refetched, 3 MiB); the
machine-bound scenarios' times (`fast`, `big40`) are scaled by the two runs' ceilings, as
`bench/README.md` scales the frozen floors, but both ways, since a slower runner this week is not
EDM getting slower. A worse parameter, a file not whole or a scenario missing fails the run and is
named in the summary; the first run on a branch has nothing to compare and passes. Run by hand
with `accept` set, a change meant to move the numbers is reported without failing, and its
numbers become the last. `compare.mjs` against reference A's frozen floor is printed beside it
for reading only. Neither replaces the benchmark beside reference A on the maintainer's computer,
and reference A's baseline is never touched. `node bench/against-last.mjs --self-test` checks the
comparison.

## The window's cost, against its own baseline

`python3 scripts/ui-bench.py --rows 100,10000 --runs 3` drives the release window through its
control port and times what a person waits for: a start with the file cache emptied and a warm
one, idle CPU and memory, a scroll, paging through the list, a sort by each heading, the search
box's filter and Find next, the CPU with downloads running, the window's answer with 300 downloads
queued and during one at full speed, and that download's time with the window against
`--headless`. Each figure is the median of the runs with its spread. It writes JSON with what
produced it (commit, rustc, target, profile, binary size, GPU, display scale, rows:
`scripts/provenance.py`) and compares it with the stored baseline for the system,
`scripts/ui-bench-baselines/<system>-<machine>.json` (`--save-baseline` replaces it, when a change
is meant to move the numbers). Its header lists each figure. `bench/bench.mjs` records the same
provenance in each results file, and `memory-check.py` prints it (and writes it as JSON with
`EDM_MEMORY_JSON`).

`python3 scripts/binary-size.py <binary>` compares the release binary's size with the one
recorded for the system in `scripts/binary-size.json` and fails on a growth over 10%. CI runs it
after each release build. `python3 scripts/stress-protocols.py detach` (macOS) pulls the download
folder's drive (a disk image, detached by force) during a download: the row must end paused with
why, and EDM must keep answering.

## The window's goldens: every word where it was

`crates/edm-gpui/src/window_tests/golden.rs` draws 19 scenes of a computer's window (the main
window at 1280, 840, 600 and 360 points wide, empty and with three rows, at 200% text, in German
and Hebrew; Add URL, four Options pages, the Scheduler, the speed limiter, Properties, About)
and 15 of the phone's, and holds every word each draws, with its window and where it is seen, to
`docs/behaviour-baseline/layout/<target>/<scene>.txt`. A word moved, cut, added or taken away, or
a dialog grown or shrunk, fails the test with the lines that differ. Words are measured by GPUI's
test text system, not the system's fonts, so a scene is the same on every machine of one system;
each system has its own files (`macos`, `linux`, `windows`, and `android`: the phone's layout
built on Linux with `--features phone-layout`, CI's Desktop job on Linux). A system with no files
is not judged: its pictures go to `target/goldens-new/<target>/`, which CI keeps as the run's
`goldens-new-<os>` artifact.

The files and `golden.rs` are among the bots' guardrails: only a person changes them. After a
change to the window that was meant, write them again and read the difference before committing:

```sh
EDM_GOLDEN=write cargo test -p edm-gpui golden                          # this computer's
EDM_GOLDEN=write cargo test -p edm-gpui --features phone-layout golden  # on Linux: the phone's
```

For a system you are not on, take the files from a CI run's `goldens-new-<os>` artifact
(`gh run download <run> -n goldens-new-ubuntu-latest`), read them, and copy them into
`docs/behaviour-baseline/layout/`.

## Profiling

A profile is read against a build that has symbols. The release profile strips them, so
`cargo build --profile profiling -p edm-gpui` builds the same code (the release profile, with
line tables and symbols kept) into `target/profiling/edm-gpui`. Run that, do what is slow,
and sample it:

- **macOS.** `sample edm-gpui 10 -file EDM.sample.txt` (in the system, no install) writes 10 seconds
  of call stacks of every thread, heaviest first, with Rust's names still mangled (`_RNv…`;
  samply and Instruments show them plain). `footprint edm-gpui` and `heap edm-gpui` say where
  the memory is, `leaks edm-gpui` what leaked. With Xcode, Instruments' Time Profiler
  (`xcrun xctrace record --template 'Time Profiler' --attach edm-gpui`) gives a flame graph, and its
  Animation Hitches template the frame times (GPUI reports none of its own). `samply record
  target/profiling/edm-gpui` (`cargo install samply`) opens a flame graph in the browser.
- **Linux.** `perf record -g --call-graph dwarf -p $(pidof edm-gpui) -- sleep 10`, then `perf report`;
  or `samply record`, or `cargo flamegraph --profile profiling -p edm-gpui`. Memory: `heaptrack`.
- **Windows.** `wpr -start CPU -filemode`, do the slow thing, `wpr -stop EDM.etl`, and open it in
  WPA with the symbol path set to `desktop\target\profiling` (the PDB is beside the exe); or
  `samply record`. Memory: the same with `-start Heap`.
- **The engine alone.** `edm-gpui --headless --control 0` takes no window, so a profile of a
  download shows the engine's cost only; `ui-bench.py --only transfer` times one with and without
  the window.

What a release binary is made of: `cargo bloat --release -p edm-gpui --crates` with
`CARGO_PROFILE_RELEASE_STRIP=false` (it reads the symbols); the last record is in
`docs/decisions.md` (2026-11-11, the release profile measured for `edm-gpui`).

## Benchmarks (criterion)

Microbenchmarks, with the `criterion` crate, of the code a large list leans on (gaps row 190).
They are run by hand, on a quiet machine, to compare two builds on that machine; **they never
gate CI**, and no CI job runs them, as shared runners are too noisy for them. `cargo test`
neither builds nor runs them (it would only with `--benches`). From the repository's root:

| Bench | What it times | Command |
|---|---|---|
| `plan` | Range planning (`edm_core::plan`): 512 KiB to 1 TiB, 1 to 64 connections, an unknown length, a server without ranges | `cargo bench -p edm-core --bench plan` |
| `naming` | `sanitize_name`, `filename_for`, and each waiting download's free name in a folder where half the names are taken, asking the disk for each (`nth_free_name`) and with the folder listed once (`nth_free_name_with`), at 100, 1,000 and 10,000 names | `cargo bench -p edm-core --bench naming` |
| `large_list` | A read of 10,000 rows (and 4 running) on a real queue: `Session::read` (the window's, and with two event subscribers), `SavedSizes::apply` (first read and unchanged), `Places` (placing, counting every place, the filter) and `earlier_of_the_same_name` | `cargo bench -p edm-shell --bench large_list` |

They build with the release profile (`opt-level = "z"`, `edm-core` at 3, LTO), so they time what
ships, and the first build takes minutes. One benchmark is picked by a filter after `--`
(`cargo bench -p edm-core --bench naming -- free_name/listed`); a quick look is
`-- --warm-up-time 1 --measurement-time 2`.

A change is judged against a baseline saved on the same machine:

```sh
git switch main      && cargo bench -p edm-shell --bench large_list -- --save-baseline main
git switch my-change && cargo bench -p edm-shell --bench large_list -- --baseline main
```

criterion prints each time's change and whether it is outside the noise (`--noise-threshold`,
1% by default); its results are under `$CARGO_TARGET_DIR/criterion/`. A baseline is that
machine's alone and is never committed. A result a change rests on goes into its entry in
`docs/decisions.md` with the machine, the two commits and both medians (AGENTS.md rule 10).

## Video sites: the compatibility test plan

`python scripts/video-sites-check.py` runs the video website compatibility test plan (the
maintainer's, 2026-11-07) against the real EDM, signed out first and in this order:

1. `engine`: CORE-08 to CORE-14 and ENG-01 to ENG-08 against a server on loopback that the
   script runs (ranges and none, redirects, pause and resume, Exit and a kill mid-download,
   dropped connections, 429/503 with Retry-After, an outage, an address that expires, a
   destination that is not a folder, a file bigger than the disk, a file past 4 GiB), every byte
   compared with what was served.
2. `public-url`: real public sites through Add URL, headless: YouTube (best, 360, 480 as a
   picture-and-sound pair, sound only), Internet Archive (MD5 from the item's metadata), Wikimedia
   Commons (SHA-1 from its API), NASA's library, Test-Videos.co.uk, an HLS and a DASH test stream,
   and refusals.
3. `public-panel`: YouTube, Vimeo, Dailymotion, Rumble, Reddit, TikTok, Twitch, Bilibili and
   PeerTube through the browser panel (`sites-check.py`), in Edge with a new profile.
4. `--auth youtube,reddit,…`: optional and outside the release gate. A visible Edge waits while
   the tester signs in by hand with an account of their own; the script creates no account and
   never types, reads or keeps a password.

EDM gets no cookies file and no login in the public phases, and each row's `signed_in` is read
back to show it. A login wall or bot check is BLOCKED, never a signed-in retry counted as a
public pass. The report (`report.md`, `report.json`) has the plan's evidence fields for each
test and the release gate. `--phases engine` runs one phase; `--list` prints every test.

## The built program, driven

| Check | What it proves | Run |
|---|---|---|
| `scripts/app-check.py` | The real binary as a process: the one-instance lock, a browser hand-off, downloads sorted, HLS, a player page that is not YouTube's refused with a reason, a proxy not gone around, a speed limit, a schedule, the login launch in the tray, the command line (`list`, `status`, `pause`…, exit codes), the control protocol, headless and MCP | `python3 scripts/app-check.py` (`EDM_BINARY=` for a packaged one) |
| `scripts/smoke.py` | An *installed* EDM starts, answers as the browsers' native host, opens a window, runs headless and downloads byte-exact. The same on Linux, Windows and macOS | `python3 scripts/smoke.py /path/to/edm-gpui` |
| `scripts/chrome-check.py` | The extension in Chrome for Testing handing off to a real EDM: byte-exact files, the browser's copy gone, cookie-gated files left to the browser, no host. With `EDM_CHROME_YOUTUBE=1`, last, the one case that goes to the network (its rows print as SKIP without it): a local page embedding a YouTube player by iframe, from youtube-nocookie.com and youtube.com, gets the panel in the frame and its click lists the qualities EDM read from the watch page (gaps row Y12) | `sh scripts/package.sh` then `python3 scripts/chrome-check.py` (`EDM_CHROME_YOUTUBE=1` for the embedded player) |
| `scripts/firefox-check.py` | The same in a real Firefox | `python3 scripts/firefox-check.py native` |
| `scripts/chrome-check.py` with `CHROME_PATH` set to Microsoft Edge | The same 69 checks in Edge itself (catching, helping connections, PDF in a tab, Alt, paused sites, popup, no-host); Edge's and Chrome's host registrations are set aside for the run and put back | `CHROME_PATH="/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge" EDM_CHROME_HEADLESS=1 python3 scripts/chrome-check.py` |
| `scripts/panel-check.py` | The "Download video" panel's hard cases in Chromium | `python3 scripts/panel-check.py` |
| `scripts/ui-shot.py` | A picture of the window for checking a visual change by eye | `python3 scripts/ui-shot.py` |
| `scripts/launch-latency.py` | How long from a browser click (a `download.start` to the native host) to EDM's first request for the file, with EDM closed (cold) and running (warm). Numbers for comparing two builds, not a pass or a fail | `xvfb-run -a python3 scripts/launch-latency.py target/release/edm-desktop` |
| `tests/throughput.rs` | The engine's own speed: loopback with no pacing, a server 80 ms away, and one holding each connection to 1 MB/s. Ignored by default; for comparing two builds on one machine | `cargo test -p edm-core --release --test throughput -- --ignored --nocapture --test-threads=1` |

By hand only (they reach real sites, so no change's CI runs them):
`scripts/youtube-check.py`, `scripts/live-panel-check.py`,
`scripts/real-sites.py`, `scripts/out-of-box-check.py`, `scripts/video-sites-check.py`.

**The daily site watch** (`.github/workflows/sites.yml`, gaps row W6) runs two of them each day
on Linux under Xvfb, with a release EDM in a new browser profile signed in nowhere and
`EDM_SKIP_LOCAL=1`: `youtube-check.py --json` and `sites-check.py --public --json` (the sites
`docs/site-list.md` says need no sign-in). `scripts/site-table.py` turns their JSON into the
README's site table (between `<!-- site-table:start -->` and `<!-- site-table:end -->`; the run's
`site-table.md` artifact, put in by hand with `--readme README.md`, since CI commits nothing) and,
when a site is broken or the check could not reach it, one issue labelled `site-watch` with the
failing sites and the end of each log, added to while it stays open and closed by the first run
that passes. A login wall, a bot check, or a file still arriving when the wait ended is *not
judged*, as `video-sites-check.py` judges it; a day on which nothing could be judged is a red run
and no issue. `python3 scripts/site-table.py --self-test` checks the judging and the table.

## On Linux, in Docker

These run from any machine with Docker; the working tree is streamed into a
container rather than mounted, so a checkout under a folder macOS guards
(Downloads, Desktop, Documents) still works. The first run builds EDM in a
named volume (`edm-linux-target`); later runs take minutes.

| Script | What it proves |
|---|---|
| `sh scripts/linux-check.sh` | `app-check.py` against the Linux binary under Xvfb |
| `sh scripts/linux-gui-check.sh` | Real mouse and keys on EDM's window and tray (xdotool, openbox, a StatusNotifier watcher and host): Add URL, Stop and Resume, Options, closing into the tray and bringing it back, Start on login started by `dex` as a session does, extracting a downloaded zip from its row menu, the command line against the live window, every tab of Options clicked and a value set and saved, a finished file's SHA-256 and a pasted digest checked in Properties, several files renamed by a pattern after a Ctrl-click, and a watched folder set in Options that picks up a saved link; and, over AT-SPI as Orca reads it, the list as a screen reader is told it (a table of rows and cells, a greyed button, a running bar's value, what finished or failed said in the status region; gaps row U23). EDM's main window is found by its title, "EDM" or one ending " — EDM" while downloads run (gaps row U16). Window positions come from `xwininfo`, not `xdotool`, which is one title bar out under some window managers. `EDM_ORCA=1` adds Orca, a real screen reader |
| `sh scripts/linux-wayland-close-check.sh` | Closing the window under sway (headless Wayland) with a tray present: EDM stays up, answers and is not marked hidden |
| `sh scripts/linux-kde-close-check.sh` | The same under **KWin 6** (KDE's compositor, Arch image `scripts/linux/Dockerfile.kde`): closing minimizes the window, a download started meanwhile completes, and restoring it shows it again. This is the reported ghost-window bug |
| `sh scripts/linux-wayland-drag-check.sh`, `linux-drag-check.sh` | A link dragged onto the window under Wayland (sway) and X11 (real Firefox), and a finished file dragged out |
| `sh scripts/linux-firefox-check.sh`, `linux-chromium-human.sh`, `linux-newcomer-check.sh` | A browser and EDM used as a person uses them, on Debian, Ubuntu, Fedora or Arch (`EDM_DISTRO=`) |
| `sh scripts/linux-package.sh`, `rpm-check.sh`, `arch-check.sh` | The `.deb` and AppImage, the `.rpm` (Fedora, openSUSE) and the Arch package built, installed and driven |
| `sh scripts/windows-check.sh` | The client compiles for Windows (`clippy` as an argument adds warnings-as-errors). Nothing runs; CI's Windows machines do |

Nothing needs installing on the machine itself: `chrome-check.py` fetches
Chrome for Testing into `~/.cache`, `firefox-check.py native` fetches Firefox
into `~/.cache/edm-firefox`, and `EDM_CHROME_HEADLESS=1 python3
scripts/chrome-check.py` runs where there is no display (not from a shell that
denies the browser's processes the network). On an Apple Silicon Mac,
`cargo build --target x86_64-apple-darwin` and then
`EDM_BINARY=… python3 scripts/app-check.py` runs the Intel build under Rosetta.
`EDM_PLATFORM=linux/amd64` runs the x86_64 Linux build under emulation on an arm64
Mac; `EDM_DISTRO=ubuntu2204` runs on Ubuntu 22.04, which CI builds releases on.

### What the container is not

Running the whole suite inside the Linux image (`cargo test --workspace`)
differs from a developer's machine in ways that are not EDM's:

- It runs as **root**, so the tests that make a folder unwritable
  (`integrity`'s read-only folder, `queue`'s unwritable queue file, the
  torrent move across disks) fail there and pass as an ordinary user.
- The CJK translation test needs `fonts-noto-cjk`; xtask's credential scan
  needs `git`. The window tests need no display at all: they render offscreen
  (207 pass with `DISPLAY` empty).
- A machine with no `systemctl` greys Sleep, Hibernate and Turn off, as the
  menu does; the window test follows `power::available`.
- A session bus under Wayland comes from `dbus-run-session`;
  `dbus-launch --exit-with-session` has no X session to wait for and lets
  the bus go.
- `kwin_wayland` carries a file capability a container refuses; the KDE check
  removes it (`setcap -r`) in its throwaway container.
- Docker Desktop's disk fills, and EDM then refuses to start a download for want
  of room: a GUI check shows a row "Paused: not enough room", which is EDM
  telling the truth. The build volume `edm-linux-target` gathers a stale copy of
  the workspace crates' artifacts with every source change (17 GB after a day's
  work); `cargo clean -p edm-core -p edm-protocol -p edm-shell
  -p edm-desktop-base -p edm-gpui -p xtask` inside a container on that volume frees them and
  keeps the third-party builds. Nothing in these scripts deletes anything.

## On a real desktop

What only a person can check is listed step by step in
[windows-checklist.md](windows-checklist.md) (17 checks, about 45 minutes).
Nothing here substitutes for a physical login, a real panel, a real GPU
driver or SmartScreen; [gaps.md](gaps.md) says which of those are still
unseen.

## Before a push

Run the whole workspace's tests, not only the crate you changed: `xtask`'s
privacy check reads comments in every crate, and a change in `edm-gpui`
can break it. The pre-commit hook (`scripts/pre-commit`; install with
`ln -sf ../../scripts/pre-commit .git/hooks/pre-commit`) refuses a commit that
writes a credential into a tracked file.

## The bots: free AI models that keep EDM, unattended

EDM is looked after by free AI models in GitHub Actions, with nobody watching (the
maintainer, 2026-11-24). The models are Kilo Code's free ones only (the maintainer,
2026-11-26: Gemini's free tier, 20 requests a day, was spent before each task began). They
answer without a key, but the keyless quota ran out part way through every task here, so a
free Kilo account's key (the `KILO_API_KEY` secret) is sent when set. Which model answers
is found each run from the gateway's own catalogue (`scripts/ai-ask.py`), so a retired
model does not stop anything. A model never holds a GitHub token: it reads and edits the
tree, and the workflow, not the model, decides what is done.

| Workflow | What it does |
|---|---|
| `ai-triage.yml` | Each issue opened or edited, and each comment: labels it, answers it, closes a duplicate, something already done, spam or not about EDM (reopened if its author replies), and queues a bug with enough to act on, or a feature the maintainer opened or that has `BOT_FEATURE_VOTES` (3) thumbs-up. A comment on the bot's pull request from the maintainer or the reporter queues a revision; the maintainer's "remember …" goes into `docs/ai-notes.md` (`scripts/ai-triage.py`). |
| `bug-report.yml` | A red run on main opens one issue per workflow and queues it; a green one closes it. When the red commit is the bot's own merge and the run before was green, it reverts that merge and queues its issue again. |
| `ai-worker.yml` | The queue, every two hours and after each task (`scripts/ai-worker.sh`): one AI job at a time, the most urgent first (main's red runs, revisions, bugs, features, then the nightly tests); a task no model could do is tried again after a week, three times; the site watch's reports are queued as bugs; and past 45 days without a commit it writes `.github/keepalive`, as GitHub turns off a quiet public repository's schedules at 60. |
| `ai-fix.yml` | One task (`ci`, `bug`, `feature`, `improve` or `revise`) for the models, each with two tries, the second told what failed (`scripts/ai-agent.sh`). Before a CI failure is given to a model, `scripts/ai-fix-context.sh` closes it if main went green since, leaves it when its failing tests pass alone on Linux, and gathers the failing tests, what changed since the last green run and each test run here. What passes becomes a pull request labelled `bot:automerge`, with CI and Desktop started on its branch. |
| `ai-tests.yml` | Once a day when nothing else waits: tests for the file with the most lines no test runs (`scripts/coverage-table.py`), kept only when that file has fewer untested lines and the whole is not down; the floor is raised to each whole percent passed. |
| `ai-merge.yml` | When CI, Desktop, Android and the human QA run (`contract-linux.yml`) have finished on a bot pull request's last commit (`scripts/ai-merge.sh`, one decision at a time): its branch holding all of main, all green, every contract row main holds held in EDM's real window (`scripts/ai-contract-gate.py`), and two models' reviews, each on its own, finding nothing against the rules, it is merged and all four run on main; otherwise it goes back to the fixer, twice, then is closed. |

**What a model is handed.** Free models do better with the way pointed out, so before one
starts, `scripts/ai-context.py` writes `.bot/context.md`: for a step of `docs/gaps.md`, the
step itself word for word first; how to work in small steps (the test first, then build with
`cargo test -p CRATE --no-run` after each edit, then make it pass, then the checks); an
outline of each file the task names (every fn, struct, impl and const with its line, the
private ones marked, so a model calls only what exists and is reachable); a test to copy
(the file's own tests, or the test file it names, or the nearest); for the window, the test
harness's public methods; then which part of EDM the task is about (desktop window, phone,
engine, extension, video), the skill pages to follow for it in the
order they win (`docs/ai-skills/`, written for the bots, and the window's own
`.claude/skills/edm-design` and `better-*`), the files that mention the task's words with the
lines, the last commits on them, and how to test that part. The pages are given only when
`scripts/ai-skills-check.py` finds them clean (also a CI job): vendored text at its pinned
SHA-256, no hidden or direction-changing character, no line written to steer a model, no
link in EDM's own pages, nothing executable. Each page ends with "Lessons" that runs add
(`scripts/ai-lesson.sh`, `docs/ai-skills/learning.md`): from a CI failure's run, the nightly
tests or a gaps row, never from a person's issue; kept only if the check still passes.

**Improving without being asked.** When nothing else waits and the nightly tests have run,
the worker takes the next row of `docs/gaps.md` (the maintainer's ranked work list) that
needs nothing only a person can give (`scripts/ai-improve-pick.py`: no "decide", no
maintainer's word or approval still to come, no real phone, Mac, Windows laptop, USB disk,
bench, store, certificate or signing), opens an issue for it (`bot:improve`) and gives it
to the fixer as `improve`: the next complete step, its tests, the row updated. Three in
flight at most, one new every six hours at most, no row again within a fortnight. A row's
state can steer it: `· person` keeps it from the bots, `· bot` hands it to them first,
`· split` hands its steps (K3a, K3b, …, each one small change with its files, test and "Done
when") to them instead, and `· after K3a` waits until K3a's row is deleted; a section whose
introduction says its rows wait for the maintainer's word is left whole. The section "Taken
by the free-model bots" in `docs/gaps.md` lists the order they take. A bot
branch that changes code also runs "Android, the engine built", and ai-merge waits for it.

**Main being red stops nothing.** The tests main already fails are read from its last
Desktop run (`scripts/ai-baseline.sh`, into a file the model cannot change): a bot's change
is refused only for a test that fails beside it and not on main, and a fix for main's red
run must leave fewer failing (`scripts/ai-test-failures.py`). At merge, a failing job on the
bot's branch is held against the same job on main: failing only where main fails too, it
does not stop the merge; a job that fails naming no test (a build, a lint, the coverage
floor) always does; a cancelled run is run again. Coverage is measured whatever tests fail
(`--ignore-run-fail`, also in Desktop's coverage job): failures are the tests' to report.

**The rules, checked by machine** (`scripts/ai-rules.py`, on what a change adds): no
`unwrap`, `expect` or `panic` in shipped code, `note!` not `eprintln!` or `dbg!`, a wait or a
timeout a named const, no `foo/mod.rs`, a test reaching only 127.0.0.1, no `unsafe`, no lint
turned off. The review adds what a machine cannot: words in `assets/i18n`, the theme's
tokens, one filled button. `scripts/ai-selftest.sh`, a CI job, tries every guardrail on a
throwaway repository, so one that stops working fails CI before a bot leans on it.

What any change is held to (`ai_check`): no edit to the guardrails (the workflows,
`scripts/`, `tooling/xtask/`, `.config/`, `.cargo/`, `Cargo.lock`, the floors,
`CLAUDE.md`, `AGENTS.md`, `docs/ai-notes.md`, `docs/ai-skills/`, `.claude/`), no test removed or `#[ignore]`d, no
contract row changed (a feature may add one), format, clippy, layering, headers, privacy,
the whole test run and the coverage floor. A bug a person reported must come with a test
that fails without the fix (`scripts/ai-fails-before.sh`). A change from an issue anyone
could open is never merged without a person when it touches the network, protocol,
extension, updater, secrets, packaging or a manifest, or changes over 1,500 lines: it is
labelled `bot:needs-maintainer` and waits. Releases are not the bots': a release needs the
maintainer's signing key (`docs/RELEASING.md`).

**Security first.** The bots run unattended on what strangers write, so each layer
assumes the one before it failed:

- *A model's code never runs where a token can write.* `ai-fix.yml` and `ai-tests.yml`
  are split: the `work` job (read-only token, no token at all in the models' step) runs
  the models and every check as the user `dmbot`, which cannot read the runner's processes
  or use sudo, and whose every process is killed before the next step; the change leaves
  it only as a patch. The `publish` job, which holds the write token, never builds or runs
  the patch: it applies it with git, holds it again to `ai_guard` (no guardrail, contract
  row, removed test or symbolic link), and commits with hooks off. Builds and tests run
  with no secret in their environment (`ai_clean`).
- *Nothing a stranger writes is an instruction.* Issue text goes to a model only as data;
  the triage script chooses what is done from a fixed list. Only the reporter and the
  maintainer reach a model with a comment, four bot replies a day at most per issue.
  Only the maintainer's words and the machine's own output become notes. A feature needs
  three thumbs-up from accounts at least 90 days old, the reporter's not counted.
- *A merge is the last gate* (`scripts/ai-merge.sh`): the bot's own branch, every commit
  the bot's, holding all of main (a branch behind is brought up to date and checked again,
  so what passed is what main becomes; one that no longer merges is closed and its task
  queued again); CI, Desktop and Android green on Linux, Windows and macOS; the human QA
  run, every behaviour contract row in EDM's real window on Linux (`contract-linux.yml`),
  holding every row each of main's last three runs held, and every row the change adds
  (`scripts/ai-contract-gate.py`; a row that fails now and then on the runner stops nothing;
  the maintainer, 2026-11-26: the app is never made worse by a bot); the window looking as
  main's does, pictured on the same runner (`scripts/ai-looks-gate.py`, B25's own 1%: a bot
  may not change how EDM looks); EDM no slower than main when the app's code changed
  (`bench.yml`, held to main's numbers, measured again once when worse); no code scanning
  alert main does not have (`codeql.yml`: Rust, Python, the workflows); never a change to a
  guardrail; never, without a person, a change to what CI cannot check (YouTube refuses
  GitHub's servers, so its rows never run there); and three reviews by three different
  models, each reading the whole diff, the third about security alone (a diff too long to
  read whole waits for a person). From an issue anyone
  could open, a change waits for a person (`bot:needs-maintainer`) when it touches the
  network, protocol, extension, downloads, archives, video sites, the system, the
  updater, secrets, packaging, assets or a manifest, adds `unsafe`, a process, the
  network, the environment, a URL or deleted files anywhere, or changes over 600 lines.
- *A merge that turns main red, or after which main holds fewer contract rows (seen twice:
  the run goes again once first), is reverted* by `bug-report.yml` (`scripts/ai-revert.sh`),
  and its task queued again.
- *Windows and macOS* run the contract too (`contract-desktop.yml`), reported, not gating,
  until their runs on main show which rows a runner holds; then their words gate as Linux's do.

What is left: the free model key (`KILO_API_KEY`) is in reach of the code a model
writes, since the agent that holds it runs that code; it is a free key, worth a day's
quota. The agents' npm packages are pinned, their own dependencies are not; they run only
in the `work` job. Releases are never the bots'.

**Checked every session.** `python3 scripts/ai-health.py` reads GitHub and says whether the
bots work: their workflows on, a free model answering, the worker running, the guardrails
holding, their jobs not failing, work taken in the last day, pull requests not stuck, merges
and lessons in the last week, and main's own state. A FAIL is the session's first task
(`docs/gaps.md`, "Every session first: are the bots working?", says what to do for each).

Labels say where each issue stands: `bot:queue` with `bot-task:ci|bug|feature|revise`,
`bot:working`, `bot:pr`, `bot:merged`, `bot:no-fix`, `bot:green-since`,
`bot:not-reproduced`, `bot:needs-votes`, `bot:needs-info`, `bot:needs-maintainer`,
`bot:reverted`, `bot:closed`. To steer the bots, write on the issue or the bot's pull
request; to teach them, write "remember …" as the maintainer; to stop them, disable
`ai-worker.yml` and `ai-triage.yml` in the Actions tab.

## Memory at rest (Windows)

`python scripts/memory-check.py` starts the release build (`EDM_BINARY` for another) with a home of its own, waits for its window to settle, samples it for twenty seconds and prints the working set, private bytes, threads and handles; it fails over `EDM_MEMORY_BUDGET_MB` (100) or `EDM_PRIVATE_BUDGET_MB` (60). Nothing of the person's is read or written: an EDM with its own `EDM_HOME` claims no browser and no file type.
