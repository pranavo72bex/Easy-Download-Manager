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
  queue, and a loopback BitTorrent swarm. The window's tests click every
  control through its accessibility tree (`crates/dm-desktop/tests/window.rs`,
  which indexes `tests/window/`: `support.rs` is the fake system and shared helpers,
  the rest are grouped by what they check).
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

## The built program, driven

| Check | What it proves | Run |
|---|---|---|
| `scripts/app-check.py` | The real binary as a process: the one-instance lock, a browser hand-off, downloads sorted, HLS, a player page handed to yt-dlp, a proxy not gone around, a speed limit, a schedule, the login launch in the tray, the command line (`list`, `status`, `pause`…, exit codes), the control protocol, headless and MCP | `python3 scripts/app-check.py` (`DM_BINARY=` for a packaged one) |
| `scripts/smoke.py` | An *installed* dm starts, answers as the browsers' native host, opens a window, runs headless and downloads byte-exact. The same on Linux, Windows and macOS | `python3 scripts/smoke.py /path/to/dm-desktop` |
| `scripts/chrome-check.py` | The extension in Chrome for Testing handing off to a real dm: byte-exact files, the browser's copy gone, cookie-gated files left to the browser, no host | `sh scripts/package.sh` then `python3 scripts/chrome-check.py` |
| `scripts/firefox-check.py` | The same in a real Firefox | `python3 scripts/firefox-check.py native` |
| `scripts/panel-check.py` | The "Download video" panel's hard cases in Chromium | `python3 scripts/panel-check.py` |
| `scripts/ui-shot.py` | A picture of the window for checking a visual change by eye | `python3 scripts/ui-shot.py` |

By hand only (they reach real sites, so they are not in CI):
`scripts/youtube-check.py`, `scripts/live-panel-check.py`,
`scripts/real-sites.py`, `scripts/out-of-box-check.py`.

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
  -p dm-desktop -p xtask` inside a container on that volume frees them and
  keeps the third-party builds. Nothing in these scripts deletes anything.

## On a real desktop

What only a person can check is listed step by step in
[windows-checklist.md](windows-checklist.md) (17 checks, about 45 minutes).
Nothing here substitutes for a physical login, a real panel, a real GPU
driver or SmartScreen; [gaps.md](gaps.md) says which of those are still
unseen.

## Before a push

Run the whole workspace's tests, not only the crate you changed: `xtask`'s
privacy check reads comments in every crate, and a change in `dm-desktop`
can break it. The pre-commit hook (`scripts/pre-commit`; install with
`ln -sf ../../scripts/pre-commit .git/hooks/pre-commit`) refuses a commit that
writes a credential into a tracked file.
