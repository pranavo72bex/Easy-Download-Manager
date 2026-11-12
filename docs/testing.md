# Testing dm

What checks dm, what each one proves, and how to run it. Every command is
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
cd desktop
cargo test --workspace                          # engine, protocol, shell and window
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo xtask scorecard | layers | privacy | secrets
cd ../browser && cargo test                     # the extension's decisions, on the host
```

- The engine's tests run against a real HTTP server on loopback and a real
  queue, and a loopback BitTorrent swarm. The GPUI window, the one shipped, is
  opened in GPUI's test platform over a real queue and driven through the
  control port's methods (`crates/dm-gpui/src/harness.rs`; the tests in
  `src/window_tests/`, grouped by what they check: `cargo test -p dm-gpui
  window_tests`), and driven whole by `scripts/human-qa.py` and
  `scripts/app-check.py`. The older egui window's tests were ported to it,
  file by file, before that window was removed (gaps row G9).
- Every reader of outside input (names, headers, playlists, archives, the
  control protocol) has a property test in `tests/fuzz.rs`; `PROPTEST_CASES=200000
  cargo test --release -p dm-core --test fuzz` hunts deeper.
- `cargo test -p dm-core --test zoo -- --nocapture` runs the server zoo
  ([zoo.md](zoo.md)); `DM_RAISE_FLOOR=1` raises its floor after a fix.
- `cargo xtask scorecard` rewrites [scorecard.md](scorecard.md) from
  [parity.md](parity.md) and [zoo.md](zoo.md); a test fails if it is stale.
- Tests use their own temporary directories and never touch a real
  Downloads folder or browser profile. A password or token in a test comes
  from `desktop/test-support/secrets.rs`, never from the code.

## The testing plan, and where each part lives

The maintainer's plan (2026-10-29) in six parts, and what answers each:

1. **Unit tests**: `cargo test --workspace` in `desktop/` (the engine's
   ranges, retries, pause and resume, names, speed limits, the queue; 1 250
   tests against real servers on loopback) and `browser/` (the extension's
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
[behaviour-contract.md](behaviour-contract.md) as a person would: dm's real
window driven through its own buttons and dialogs against a local server,
then Microsoft Edge with the extension (`youtube-check.py` on a real video,
and `every-type-check.py`: every kind of file goes to dm with dm set as
installed). In sight, never headless; about half an hour. It fails when any
row fails or was not checked, and `--save-baseline` records the window's
pictures once a change to them is approved. This is what "QA" means here.

## Real sites, beside reference A

[site-list.md](site-list.md) lists the sites reference A is known to work on (the maintainer's list, 2026-11-05),
with what to try on each and columns for what reference A and dm did. A person runs it in the
final human QA, in Microsoft Edge with both extensions; a place where dm does less is a row in
`gaps.md` (row 152).

## Speed, memory and CPU against reference A's frozen floor

[`bench/`](../bench/README.md) holds the side-by-side benchmark. Reference A's runs of 2026-11-09
and 10 are frozen in `bench/reference-a-baseline.json` as the floor of every parameter, and a test
(`the_reference_a_baseline_never_changes`) keeps that file from changing. `node bench/bench.mjs`
runs dm's release build on Windows, macOS or Linux (and reference A as well, on the computer that
has it), and `node bench/compare.mjs` judges each parameter. It exits 0 only when dm is better in
all of them. A parameter where dm is worse is a row in `gaps.md`.

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
`DM_MEMORY_JSON`).

`python3 scripts/binary-size.py <binary>` compares the release binary's size with the one
recorded for the system in `scripts/binary-size.json` and fails on a growth over 10%. CI runs it
after each release build. `python3 scripts/stress-protocols.py detach` (macOS) pulls the download
folder's drive (a disk image, detached by force) during a download: the row must end paused with
why, and dm must keep answering.

## Profiling

A profile is read against a build that has symbols. The release profile strips them, so
`cargo build --profile profiling -p dm-gpui` builds the same code (the release profile, with
line tables and symbols kept) into `desktop/target/profiling/dm-gpui`. Run that, do what is slow,
and sample it:

- **macOS.** `sample dm-gpui 10 -file dm.sample.txt` (in the system, no install) writes 10 seconds
  of call stacks of every thread, heaviest first, with Rust's names still mangled (`_RNv…`;
  samply and Instruments show them plain). `footprint dm-gpui` and `heap dm-gpui` say where
  the memory is, `leaks dm-gpui` what leaked. With Xcode, Instruments' Time Profiler
  (`xcrun xctrace record --template 'Time Profiler' --attach dm-gpui`) gives a flame graph, and its
  Animation Hitches template the frame times (GPUI reports none of its own). `samply record
  desktop/target/profiling/dm-gpui` (`cargo install samply`) opens a flame graph in the browser.
- **Linux.** `perf record -g --call-graph dwarf -p $(pidof dm-gpui) -- sleep 10`, then `perf report`;
  or `samply record`, or `cargo flamegraph --profile profiling -p dm-gpui`. Memory: `heaptrack`.
- **Windows.** `wpr -start CPU -filemode`, do the slow thing, `wpr -stop dm.etl`, and open it in
  WPA with the symbol path set to `desktop\target\profiling` (the PDB is beside the exe); or
  `samply record`. Memory: the same with `-start Heap`.
- **The engine alone.** `dm-gpui --headless --control 0` takes no window, so a profile of a
  download shows the engine's cost only; `ui-bench.py --only transfer` times one with and without
  the window.

What a release binary is made of: `cargo bloat --release -p dm-gpui --crates` with
`CARGO_PROFILE_RELEASE_STRIP=false` (it reads the symbols); the last record is in
`docs/decisions.md` (2026-11-11, the release profile measured for `dm-gpui`).

## Video sites: the compatibility test plan

`python scripts/video-sites-check.py` runs the video website compatibility test plan (the
maintainer's, 2026-11-07) against the real dm, signed out first and in this order:

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

dm gets no cookies file and no login in the public phases, and each row's `signed_in` is read
back to show it. A login wall or bot check is BLOCKED, never a signed-in retry counted as a
public pass. The report (`report.md`, `report.json`) has the plan's evidence fields for each
test and the release gate. `--phases engine` runs one phase; `--list` prints every test.

## The built program, driven

| Check | What it proves | Run |
|---|---|---|
| `scripts/app-check.py` | The real binary as a process: the one-instance lock, a browser hand-off, downloads sorted, HLS, a player page that is not YouTube's refused with a reason, a proxy not gone around, a speed limit, a schedule, the login launch in the tray, the command line (`list`, `status`, `pause`…, exit codes), the control protocol, headless and MCP | `python3 scripts/app-check.py` (`DM_BINARY=` for a packaged one) |
| `scripts/smoke.py` | An *installed* dm starts, answers as the browsers' native host, opens a window, runs headless and downloads byte-exact. The same on Linux, Windows and macOS | `python3 scripts/smoke.py /path/to/dm-gpui` |
| `scripts/chrome-check.py` | The extension in Chrome for Testing handing off to a real dm: byte-exact files, the browser's copy gone, cookie-gated files left to the browser, no host | `sh scripts/package.sh` then `python3 scripts/chrome-check.py` |
| `scripts/firefox-check.py` | The same in a real Firefox | `python3 scripts/firefox-check.py native` |
| `scripts/chrome-check.py` with `CHROME_PATH` set to Microsoft Edge | The same 69 checks in Edge itself (catching, helping connections, PDF in a tab, Alt, paused sites, popup, no-host); Edge's and Chrome's host registrations are set aside for the run and put back | `CHROME_PATH="/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge" DM_CHROME_HEADLESS=1 python3 scripts/chrome-check.py` |
| `scripts/panel-check.py` | The "Download video" panel's hard cases in Chromium | `python3 scripts/panel-check.py` |
| `scripts/ui-shot.py` | A picture of the window for checking a visual change by eye | `python3 scripts/ui-shot.py` |
| `scripts/launch-latency.py` | How long from a browser click (a `download.start` to the native host) to dm's first request for the file, with dm closed (cold) and running (warm). Numbers for comparing two builds, not a pass or a fail | `xvfb-run -a python3 scripts/launch-latency.py desktop/target/release/dm-desktop` |
| `tests/throughput.rs` | The engine's own speed: loopback with no pacing, a server 80 ms away, and one holding each connection to 1 MB/s. Ignored by default; for comparing two builds on one machine | `cargo test -p dm-core --release --test throughput -- --ignored --nocapture --test-threads=1` |

By hand only (they reach real sites, so they are not in CI):
`scripts/youtube-check.py`, `scripts/live-panel-check.py`,
`scripts/real-sites.py`, `scripts/out-of-box-check.py`, `scripts/video-sites-check.py`.

## On Linux, in Docker

These run from any machine with Docker; the working tree is streamed into a
container rather than mounted, so a checkout under a folder macOS guards
(Downloads, Desktop, Documents) still works. The first run builds dm in a
named volume (`dm-linux-target`); later runs take minutes.

| Script | What it proves |
|---|---|
| `sh scripts/linux-check.sh` | `app-check.py` against the Linux binary under Xvfb |
| `sh scripts/linux-gui-check.sh` | Real mouse and keys on dm's window and tray (xdotool, openbox, a StatusNotifier watcher and host): Add URL, Stop and Resume, Options, closing into the tray and bringing it back, Start on login started by `dex` as a session does, extracting a downloaded zip from its row menu, the command line against the live window, every tab of Options clicked and a value set and saved, a finished file's SHA-256 and a pasted digest checked in Properties, several files renamed by a pattern after a Ctrl-click, and a watched folder set in Options that picks up a saved link. Window positions come from `xwininfo`, not `xdotool`, which is one title bar out under some window managers. `DM_ORCA=1` adds Orca, a real screen reader |
| `sh scripts/linux-wayland-close-check.sh` | Closing the window under sway (headless Wayland) with a tray present: dm stays up, answers and is not marked hidden |
| `sh scripts/linux-kde-close-check.sh` | The same under **KWin 6** (KDE's compositor, Arch image `scripts/linux/Dockerfile.kde`): closing minimizes the window, a download started meanwhile completes, and restoring it shows it again. This is the reported ghost-window bug |
| `sh scripts/linux-wayland-drag-check.sh`, `linux-drag-check.sh` | A link dragged onto the window under Wayland (sway) and X11 (real Firefox), and a finished file dragged out |
| `sh scripts/linux-firefox-check.sh`, `linux-chromium-human.sh`, `linux-newcomer-check.sh` | A browser and dm used as a person uses them, on Debian, Ubuntu, Fedora or Arch (`DM_DISTRO=`) |
| `sh scripts/linux-package.sh`, `rpm-check.sh`, `arch-check.sh` | The `.deb` and AppImage, the `.rpm` (Fedora, openSUSE) and the Arch package built, installed and driven |
| `sh scripts/windows-check.sh` | The client compiles for Windows (`clippy` as an argument adds warnings-as-errors). Nothing runs; CI's Windows machines do |

Nothing needs installing on the machine itself: `chrome-check.py` fetches
Chrome for Testing into `~/.cache`, `firefox-check.py native` fetches Firefox
into `~/.cache/dm-firefox`, and `DM_CHROME_HEADLESS=1 python3
scripts/chrome-check.py` runs where there is no display (not from a shell that
denies the browser's processes the network). On an Apple Silicon Mac,
`cargo build --target x86_64-apple-darwin` and then
`DM_BINARY=… python3 scripts/app-check.py` runs the Intel build under Rosetta.
`DM_PLATFORM=linux/amd64` runs the x86_64 Linux build under emulation on an arm64
Mac; `DM_DISTRO=ubuntu2204` runs on Ubuntu 22.04, which CI builds releases on.

### What the container is not

Running the whole suite inside the Linux image (`cargo test --workspace`)
differs from a developer's machine in ways that are not dm's:

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
- Docker Desktop's disk fills, and dm then refuses to start a download for want
  of room: a GUI check shows a row "Paused: not enough room", which is dm
  telling the truth. The build volume `dm-linux-target` gathers a stale copy of
  the workspace crates' artifacts with every source change (17 GB after a day's
  work); `cd desktop && cargo clean -p dm-core -p dm-protocol -p dm-shell
  -p dm-desktop-base -p dm-gpui -p xtask` inside a container on that volume frees them and
  keeps the third-party builds. Nothing in these scripts deletes anything.

## On a real desktop

What only a person can check is listed step by step in
[windows-checklist.md](windows-checklist.md) (17 checks, about 45 minutes).
Nothing here substitutes for a physical login, a real panel, a real GPU
driver or SmartScreen; [gaps.md](gaps.md) says which of those are still
unseen.

## Before a push

Run the whole workspace's tests, not only the crate you changed: `xtask`'s
privacy check reads comments in every crate, and a change in `dm-gpui`
can break it. The pre-commit hook (`scripts/pre-commit`; install with
`ln -sf ../../scripts/pre-commit .git/hooks/pre-commit`) refuses a commit that
writes a credential into a tracked file.

## Memory at rest (Windows)

`python scripts/memory-check.py` starts the release build (`DM_BINARY` for another) with a home of its own, waits for its window to settle, samples it for twenty seconds and prints the working set, private bytes, threads and handles; it fails over `DM_MEMORY_BUDGET_MB` (100) or `DM_PRIVATE_BUDGET_MB` (60). Nothing of the person's is read or written: a dm with its own `DM_HOME` claims no browser and no file type.
