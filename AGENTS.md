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

The loop: write down what dm should do; build it so that it runs through a
registry command; test it in the window, in the real binary, or in a real
browser; look at a screenshot; then update the page it belongs to.

## What dm is

A free download manager for Windows, macOS and Linux, with a browser
extension that hands it the browser's downloads.

- `desktop/` is the product. `crates/dm-core` is the engine (probe, plan,
  ranged fetch, resume, assembly, queue, HLS, Metalink, BitTorrent, yt-dlp
  hand-off). `crates/dm-shell` is everything around the window that is not
  the window: settings, the native messaging host, the one-instance lock,
  startup and the inbox. `crates/dm-desktop` is the window, drawn in Rust
  with egui, no web view, plus the
  tray, login item and notifications. `cargo packager` there builds the
  installers. The Tauri web-view window it replaced is in git history.
  Inside `dm-desktop`, `src/app/` is the window's state and behaviour,
  `src/dialogs/` is one file per dialog, and `main.rs` is split into
  `platform`, `tray`, `window`, `hidden`, `signals` and `login`.
  `crates/dm-protocol` is what the extension and the native host say to
  each other, and the rule for which downloads are caught — one crate used
  by both sides.
- `browser/` is the browser extension, in Rust (crate `dm-catcher`),
  compiled to WebAssembly. It catches downloads and hands them to the app,
  and downloads nothing itself: with no app the browser keeps them.
  `extension/` holds what the browser loads around it: two manifests, the
  icons, `background.js`, a 20-line loader (a Manifest V3 background must
  register its listeners before any WebAssembly can load), and
  `content.js`, which loads the page script and itself answers Alt-click
  and "Download all links", so a page that forbids WebAssembly keeps them. `scripts/package.sh` builds it into
  `extension/pkg`, `dist/chrome` and `dist/firefox`.
- `scripts/` holds the checks that drive the built program, a browser or a
  container; `docs/testing.md` lists them.
- `shared/naming-cases.json` holds the filename cases the engine is held to.

## Where status lives

| File | What it says |
|---|---|
| `docs/parity.md` | Every behaviour, its status, and the test or run that proves it |
| `crates/dm-desktop/tests/window.rs` (indexing `tests/window/`) | Every menu item, toolbar button and row-menu item clicked by its name and checked for what it does |
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

```sh
cd browser && cargo test                   # the extension's decisions, on the host
cd browser && cargo clippy --target wasm32-unknown-unknown -- -D warnings
sh scripts/package.sh                      # the WebAssembly, dist/chrome, dist/firefox, zips
cd desktop && cargo test --workspace       # Rust unit + integration (real HTTP server)
cd desktop && cargo clippy --workspace --all-targets -- -D warnings
cd desktop && cargo fmt --check
python3 scripts/app-check.py               # drives the built binary (DM_BINARY= for a packaged one)
cd desktop && cargo test -p dm-desktop --test window   # the window, through its accessibility tree
DM_SHOTS=docs cargo test -p dm-desktop --test window shots   # the README's pictures, rendered
./target/debug/dm-desktop --control 0      # drive the running window: see control.rs
./target/debug/dm-desktop --headless --control 0   # no window, for scripts and agents
./target/debug/dm-desktop mcp              # MCP tools for an AI client, relayed to a running dm
./target/debug/dm-desktop list | status N | pause N | resume N | cancel N   # the command line, against a running dm
cargo xtask scorecard | layers | privacy   # the scorecard, the crates' layers, nothing phones home
cargo test -p dm-core --test zoo -- --nocapture   # the server zoo; DM_RAISE_FLOOR=1 after a fix
PROPTEST_CASES=200000 cargo test --release -p dm-core --test fuzz   # a deep fuzz hunt
sh scripts/linux-package.sh                # .deb and AppImage built, the .deb installed and driven
sh scripts/linux-check.sh                  # app-check.py against the Linux binary, in a Debian container
sh scripts/linux-gui-check.sh              # real mouse and keys on the window and tray, in a container
sh scripts/linux-kde-close-check.sh        # closing the window under KDE's compositor (KWin), in a container
python3 scripts/chrome-check.py            # Chrome for Testing, handing off to a real dm
python3 scripts/firefox-check.py native    # a real Firefox, when one is installed
```

`cargo` lives in `~/.cargo/bin`, which a fresh shell may not have on `PATH`.
There is no Node, npm or JavaScript test suite; do not add one.

## Rules for a change

1. Reproduce first: a failing test against the local server in
   `crates/dm-core/tests/common`, or a recorded run.
2. One defect per change. No drive-by refactors.
3. Never report `Completed` for bytes that were not all received and
   finalised; never let a remote name escape the download folder.
4. Tests use their own temporary directories. Nothing touches a real
   Downloads folder, browser profile or credential.
5. No DRM, paywall, login or CAPTCHA circumvention, and no site-specific
   extraction: player pages go to the user's own yt-dlp.
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
