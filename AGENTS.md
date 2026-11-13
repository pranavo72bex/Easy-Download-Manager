# AGENTS.md

Entry point for anyone — person or coding agent — about to change dm. Read
this, then `docs/gaps.md` (the ranked work list: take the top open item),
then `docs/parity.md`, then `docs/decisions.md`.

## Orientation

| Read | Why |
|---|---|
| `docs/gaps.md` | The ranked work list: take the top open item |
| `docs/feature-status.md` | Every feature, with the run that showed it and what is unchecked |
| `docs/parity.md` | dm's behaviour checklist, with the test or run that proves each line |
| `docs/ui-design.md` | dm's look as tokens; verify a visual change with `scripts/ui-shot.py` |
| `docs/video-panel.md` | The browser's "Download video" panel, line by line with its tests |
| `docs/decisions.md` | Decisions not obvious from the code, and why |
| `docs/scorecard.md` | Generated counts: behaviours, zoo, fuzzing, budgets |
| `docs/testing.md` | Every check, what it proves and how to run it, in Docker too |
| `bench/README.md` | Reference A's frozen benchmark: the floor for dm's speed, memory and CPU on every system |

The loop: write down what dm should do; build it so that it runs through a
registry command; test it in the window, in the real binary, or in a real
browser; look at a screenshot; then update the page it belongs to.

## What dm is

A free download manager for Windows, macOS and Linux, with a browser
extension that hands it the browser's downloads.

- `desktop/` is the product. `crates/dm-core` is the engine (probe, plan,
  ranged fetch, resume, assembly, queue, HLS, DASH, Metalink, BitTorrent,
  video-page handling), and on macOS the allocator the GPUI window runs with
  (`memory`: large blocks go back to the system when freed). `crates/dm-shell` is everything around the window that is not
  the window: settings, the native messaging host, the one-instance lock,
  startup and the inbox, and the application layer (`dm_shell::app`: the
  queue's verbs, the commands, what happened between reads and the typed
  events, schedules, hand-offs, the control port's rules, and every request
  to a server that is not a download), so another window toolkit can drive
  the same app (`docs/architecture/`). `crates/dm-desktop-base` is what the
  desktop program does that is not drawing, with no window toolkit: `Desktop`
  (the application's `Platform`), the login item, the control port, MCP, the
  command line, the words (`i18n`, its catalogs in `dm-desktop-base/i18n`) and
  numbers (`format`), and the system's services. `crates/dm-gpui` is the
  window people install on macOS, Windows and Linux, drawn in Rust with GPUI,
  no web view, plus the tray: one file per part of the window (`list.rs` the
  rows and their reads, `tick.rs` what each wake does, `table.rs`,
  `menus.rs`, `toolbar.rs`, `sidebar.rs`, one file per dialog such as
  `addurl.rs`, `details.rs`, `options.rs`, and `port.rs` with `probe.rs` for
  the control port). `cargo packager -p dm-gpui` there builds the installers,
  from its `icons/` and `macos/Info.plist`; its `Cargo.toml` holds dm's
  version. The older egui window (`crates/dm-desktop`) was removed with its
  fonts and vendored patches on 2026-11-13 (gaps row G9).
  `crates/dm-protocol` is what the extension and the native host say to
  each other, and the rule for which downloads are caught — one crate used
  by both sides.
- `browser/` is the browser extension, in Rust (crate `dm-catcher`),
  compiled to WebAssembly. It catches downloads and hands them to the app,
  and downloads nothing itself: with no app the browser keeps them.
  `extension/` holds what the browser loads around it: two manifests, the
  icons, the popup's page, and `content.js`, which has the background put the panel's code
  (`pkg/panel.js`) into the page's frame and itself answers Alt-click
  and "Download all links", so a page that forbids WebAssembly keeps them. `scripts/package.sh` builds it into
  `extension/pkg`, `dist/chrome` and `dist/firefox`, and writes the scripts that start the background
  and the popup (`pkg/background.js`, with the WebAssembly in it so Rust registers the listeners on the
  worker's first turn, `web/listen.rs`; `pkg/popup.js`): they have no JavaScript of their own.
  What the extension says (the popup, the four menu items, the panel, its name) is listed in
  `browser/src/words.rs` and said through the browser's own `i18n` from `extension/_locales`,
  one catalog per language the app speaks; a word added there needs an entry in every catalog,
  which `words/tests.rs` checks.
- `scripts/` holds the checks that drive the built program, a browser or a
  container; `docs/testing.md` lists them.
- `shared/naming-cases.json` holds the filename cases the engine is held to.

## Where status lives

| File | What it says |
|---|---|
| `docs/parity.md` | Every behaviour, its status, and the test or run that proves it |
| `crates/dm-gpui/src/window_tests/` (with `src/harness.rs`) | The GPUI window's every menu item, toolbar button and row-menu item clicked by its name and checked for what it does, through the control port's methods in GPUI's test platform; and whole by `scripts/human-qa.py` |
| `docs/zoo.md` | Thirty-one server behaviours and what dm does with each (generated; floor only rises) |
| `docs/scorecard.md` | The counts of all of the above, by area (generated) |
| `docs/how-it-works.md` | How the app and the extension fit together, and how to build them |
| `CLAUDE.md` | The feature checklist and the working rules for a coding agent |

**Wired is not working.** A control that calls a command proves the call,
not the download. A row is `PASS` in the parity file only when a test or a
recorded run checks the outcome — bytes on disk, a hash, a state that
survives a restart. Everything else is `PARTIAL`, `NOT VERIFIED` or
`BLOCKED`, with the reason.

## Commands

The Makefile is the short way, as with the flutter tool (`make help` lists
them all):

```sh
make run [ARGS="…"]     # build what changed and start dm with its window
make build              # installers for every platform this machine can build, into build/
make build-macos | build-linux | build-windows | build-extension   # one of them
make devices            # which platforms this machine can build, and why not
make test | analyze | format | qa | qa-docker   # tests; what CI refuses; the contract, whole
```

All of it that needs no window, in one command (formatting, lints, licences and the tests of
both workspaces; each step runs, and the failures are listed at the end):

```sh
cd desktop && cargo xtask check
```

Underneath:

```sh
cd browser && cargo test                   # the extension's decisions, on the host
cd browser && cargo clippy --target wasm32-unknown-unknown -- -D warnings
sh scripts/package.sh                      # the WebAssembly, dist/chrome, dist/firefox, zips
cd desktop && cargo test --workspace       # Rust unit + integration (real HTTP server)
cd desktop && cargo clippy --workspace --all-targets -- -D warnings
cd desktop && cargo fmt --check
python3 scripts/app-check.py               # drives the built binary (DM_BINARY= for a packaged one)
python3 scripts/readme-shots.py           # the README's pictures, of the GPUI window (macOS)
./target/debug/dm-gpui --control 0         # drive the running window, the one shipped: see dm-desktop-base control.rs
./target/debug/dm-gpui --headless --control 0   # no window, for scripts and agents
./target/debug/dm-gpui mcp                 # MCP tools for an AI client, relayed to a running dm
./target/debug/dm-gpui list | status N | pause N | resume N | cancel N   # the command line, against a running dm
cd desktop && cargo test -p dm-gpui window_tests   # the window, driven through the control port's methods
cargo xtask scorecard | layers | privacy   # the scorecard, the crates' layers, nothing phones home
cargo test -p dm-core --test zoo -- --nocapture   # the server zoo; DM_RAISE_FLOOR=1 after a fix
PROPTEST_CASES=200000 cargo test --release -p dm-core --test fuzz   # a deep fuzz hunt
sh scripts/linux-package.sh                # .deb and AppImage built, the .deb installed and driven
sh scripts/linux-check.sh                  # app-check.py against the Linux binary, in a Debian container
sh scripts/linux-gui-check.sh              # real mouse and keys on the window and tray, in a container
sh scripts/linux-kde-close-check.sh        # closing the window under KDE's compositor (KWin), in a container
python3 scripts/chrome-check.py            # Chrome for Testing, handing off to a real dm
python3 scripts/firefox-check.py native    # a real Firefox, when one is installed
node bench/bench.mjs && node bench/compare.mjs   # dm against reference A's frozen floor, every parameter (bench/README.md)
```

`cargo` lives in `~/.cargo/bin`, which a fresh shell may not have on `PATH`.
There is no npm or JavaScript test suite; do not add one. The benchmark
(`bench/`) is plain Node with no packages, because its server is part of
the frozen standard that reference A was measured against.

### On Windows (learnt the hard way, 2026-11-04)

- Build outside `C:`: `CARGO_TARGET_DIR=D:/dm-target` for `desktop/`, and a
  directory of its own (`D:/dm-target-browser`) for `browser/`, so the two do
  not wait on each other's lock.
- `python3` can be the Microsoft Store's placeholder, which only prints how to
  install Python: `scripts/package.sh` finds a Python that runs; elsewhere call
  `python`.
- `cargo test --workspace` stops at the first test program that fails, so the
  crates after it never run: add `--no-fail-fast` to see every failure. A
  test that fails in a full run and passes alone is most often waiting on
  something a loaded machine makes slow (a dialog laid out again when an
  answer comes, a peer still there): re-run it by name, then find what it
  waits on rather than lengthening its wait blindly (gaps row 115, closed
  2026-11-07 on Windows).
- The order a change is checked in, cheapest first: `cargo fmt --check`, both
  clippies, the crates' tests, `scripts/chrome-check.py`, then
  `scripts/human-qa.py` whole.
- A browser check's first hand-over can be slow: Windows' virus scanner reads
  each fresh copy of `dm-gpui.exe` the first time it runs. chrome-check
  asks the host for a hello before its first click, and a failed check prints
  what the extension said since the check before.
- `%RANDOM%` in a `.bat` is the same in every `cmd.exe` started in the same
  second: a name built from it is not unique between two hosts. chrome-check's
  host shim makes a folder for each start's stderr, which only one can win,
  inside a parent made beforehand (two `mkdir`s also making the parent at once
  can both succeed).
- A dm started from a Git Bash subshell with `&` ends with the subshell; start
  one to look at with PowerShell's `Start-Process`.
- Reference A's Edge extension is a Manifest V2 one, which Edge disables in a
  new or test profile: its panel can be watched only in a profile where it was
  already on (the maintainer's own Edge). The side-by-side harness lives
  outside the repository, since it names reference A.
- Windows Firewall asks about dm (or a test program) when a torrent listens
  for peers; Cancel is enough for a test, which only downloads.

## Rules for a change

0. What a person does and sees is fixed by `docs/behaviour-contract.md`:
   no row of it changes without the maintainer's approval in words, and QA
   is `python3 scripts/human-qa.py` run whole in a visible window, which
   fails on any row failed or not checked.
1. Reproduce first: a failing test against the local server in
   `crates/dm-core/tests/common`, or a recorded run.
2. One defect per change. No drive-by refactors.
3. Never report `Completed` for bytes that were not all received and
   finalised; never let a remote name escape the download folder.
4. Tests use their own temporary directories. Nothing touches a real
   Downloads folder, browser profile or credential.
5. No DRM, paywall, login or CAPTCHA circumvention, and no site-specific
   extraction: YouTube pages are read by dm itself, other sites by the browser extension.
6. Free means free: no paid service, no analytics, no crash reporting, no
   telemetry. `cargo xtask privacy` enforces it; do not weaken it.
7. Shipped code does not `unwrap`, `expect` or `panic`; a new reader of
   outside input gets a property test in its crate's `tests/fuzz.rs`.
8. Keep modules small and by concern: a source file over 2500 lines fails
   `cargo xtask layers`. A module's tests go in `tests.rs` beside it
   (`foo.rs` with `foo/tests.rs`), which the privacy and translation scans
   skip by name, so a test's made-up address or string is not taken for
   shipped code.
9. Update `docs/parity.md` and `docs/gaps.md` with the exact
   command and result. Never record a run that did not happen.

## Rules for performance and platform work

These come from the maintainer's optimization spec of 2026-11-11. Where dm
stands against each rule is in `docs/optimization-audit.md`, and what is
still missing is in `docs/gaps.md`, rows 180 to 192.

1. The engine never imports GPUI or any other window toolkit. `dm-core`,
   `dm-shell`, `dm-desktop-base` and `dm-protocol` stay free of them, and
   `cargo xtask layers` refuses a change that breaks this.
2. Render functions and event handlers never block. This rules out the file
   system, the network, `Backend` calls that `block_on` the engine, a
   synchronous file dialog, and a lock held across slow work. What takes
   time runs on the runtime or a worker, and its answer comes back to the
   window.
3. One download's progress never rebuilds the whole window's state. Read
   what changed and redraw only what shows it. A wake that changed nothing
   on screen draws nothing.
4. Queues of progress or telemetry are bounded or coalesced (latest value
   per download). Lifecycle events are the exception: they are kept and
   never dropped.
5. Shared crates hold no platform behaviour without a reason. Platform code
   sits behind `dm_shell::app::platform::Platform`, or in a module of its
   own whose small API is the same on every system.
6. Before adding an abstraction, find the one that already does the job
   (`docs/architecture/MODULE_BOUNDARIES.md`) and weigh using it. Two of
   anything is a cost each later change pays.
7. No `unsafe` without a `// SAFETY:` comment that states its invariants
   (clippy enforces the comment), and no new unsafe block without a
   reviewer's reading. Prefer a safe crate that already does it.
8. No new dependency without saying, in the commit, why the ones already in
   `Cargo.lock` cannot do it, and what it adds to the build time and the
   binary.
9. Every task spawned has an owner that can cancel it, and a plan for when
   dm quits. A task that is simply dropped with the runtime does not count
   as stopped.
10. Every optimization comes with its measurement before and after: the
    benchmark (`bench/`), `scripts/ui-bench.py`, `scripts/memory-check.py`
    or a profile. A change justified by reasoning alone is not an
    optimization.
11. What a person sees and does is kept unless the maintainer has approved
    the change (rule 0 above).
12. Engine or infrastructure work changes nothing in the window's design,
    layout or wording.
13. Public APIs are narrow and say their invariants. `missing_docs` is on in
    every crate.
14. A bug found gets a regression test first (rule 1 above).
15. Compiling for a system is not supporting it. A platform claim needs a
    run on that system: a check, `human-qa.py`, or a person's run recorded
    in `docs/gaps.md`.

An optimization goes in this order. Read the architecture and the
dependencies it touches. Find the hot path by measuring, and take a baseline
with the same command you will run afterwards. State a hypothesis, then make
the smallest change that tests it. Run the unit and integration tests, then
the same benchmark (medians of three or more runs), and look for side effects
on memory, CPU and binary size. Build and test on every system the change
touches. Its row in `docs/gaps.md` and an entry in `docs/decisions.md` then
say:

- the problem, and the evidence for it
- the files and modules changed, and any effect on the architecture
- the numbers before and after, and the commands that produced them
- the systems it was tested on
- what it costs, what it does not fix, and whether it needs more profiling
