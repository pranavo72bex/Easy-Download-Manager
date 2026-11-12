# GPUI test plan

How each step of the migration is checked, on which systems, and against what numbers.
The steps are `docs/gaps.md` rows G1 to G9 and [GPUI_MIGRATION_PLAN.md](GPUI_MIGRATION_PLAN.md)'s
phases. The behavioural oracle throughout is [docs/behaviour-contract.md](../behaviour-contract.md),
checked whole by `scripts/human-qa.py`; [EGUI_PARITY_REPORT.md](EGUI_PARITY_REPORT.md) records
each row's state in each window.

## 1. The gate (every step that touches `desktop/`, `browser/` or `extension/`)

Run in this order, before the step's commit. A step is not done until all four pass.

| # | Command | Passes when |
|---|---|---|
| 1 | `cd desktop && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` | No diff, no warning, no failed test (about 2,135 tests on 2026-11-09) |
| 2 | `sh scripts/windows-check.sh clippy` (Docker running; every crate, all targets) | Compiles with no warning for Windows: the Windows-only code that moves in G2 and G3 is never built on the Mac otherwise (a plain `cargo check --target x86_64-pc-windows-msvc` stops at `ring`'s C, which needs the Windows headers the container carries) |
| 3 | `cd desktop && cargo build -p dm-desktop && cd .. && sh scripts/package.sh && python3 scripts/human-qa.py` | macOS, visible window: `64 of 64 rows hold` | **pass** (2026-11-11: `human-qa.py` B42 on the GPUI window: Add URL a window of its own over the middle of dm's window, its OK and Cancel given to a screen reader, moved half outside dm's window and still working there, Options holding dm's window) 
| 4 | `sh scripts/linux-human-qa.sh` (Docker running) | Debian 12 on X11, a build of the checkout: `64 of 64 rows hold` | **pass** but the menu (2026-11-11: `--hidden` makes no window, `CGWindowListCopyWindowInfo` lists none of dm's, and the tray is made with no problem said; `ui.window` shows the window and puts it back; the tray's menu itself is drawn by the system and needs a person's click, not run here) 

`cargo xtask layers` is part of `cargo test` (the xtask tests); a crate added by G2 or G6 must be
added to its list in `desktop/xtask/src/main.rs` in the same commit.

A row that fails once and passes on a second whole run is reported as such (B40 did once on
2026-11-09, with no change between the runs); a row that fails twice is a defect, fixed before
the commit. Windows' run of `human-qa.py` is the maintainer's; a phase is not merged into `main`
without it.

## 2. What each row adds to the gate

| Row | Adds |
|---|---|
| G1 docs and code agree | Nothing beyond the gate (equivalent code only) |
| G2 toolkit-free crate | `cargo xtask layers` names the new crate and forbids it every GUI crate; the binary's non-window modes unchanged: `scripts/handover-check.py`, `scripts/launch-latency.py` (native host cold and warm), `dm-desktop --help` and the MCP server answering `tools/list` as before |
| G3 rules into `dm_shell::app` | Each rule moved has a test in dm-shell that fails without it; `session.rs`'s `no_window_tests` drive the rule with no window |
| G4 `ui.*` as a contract | Tests in the shared crate fix every `ui.*` answer's JSON (field names, types, order where `human-qa.py` depends on it), written from what `human-qa.py` and `mcp.rs` read; `human-qa.py` itself unchanged |
| G5 egui baseline | `scripts/ui-bench.py` (section 4) run three times on the release build; the medians written below |
| G6 GPUI shell | The Phase B checks (section 5), each pass or fail per system |
| G7 slices | For each slice, its contract rows (EGUI_PARITY_REPORT.md's Slice column) pass in `human-qa.py` with `DM_BINARY` naming the GPUI binary, on macOS, Linux and Windows; the egui window still 64/64 |
| G8 platforms and packaging | The release workflow's packages, downloaded from a draft, 64/64: `.dmg` on macOS, `DM_RELEASE=… sh scripts/linux-human-qa.sh`, the Windows installer |
| G9 switch and removal | EGUI_PARITY_REPORT.md all "holds" in GPUI; the gate after egui is removed |

## 3. Systems

| System | How | Covers |
|---|---|---|
| macOS (Apple silicon) | `human-qa.py` in a visible window, Edge with the extension, a real YouTube video | The whole contract; VoiceOver's view through `ui.accessibility` (B42) |
| Linux X11 | `scripts/linux-human-qa.sh` (Debian 12, Xvfb, openbox, Chromium in Edge's place) | The whole contract, including B25 against `docs/behaviour-baseline/linux` |
| Linux Wayland | `scripts/linux-wayland-close-check.sh`, `linux-wayland-drag-check.sh` | Close to tray, drag in and out |
| Linux desktops | `linux-gui-check.sh` (tray, Orca over AT-SPI), `linux-kde-close-check.sh` | Tray, screen reader, KDE |
| Windows | `human-qa.py` on a Windows computer (the maintainer's), `windows-a11y-check.py` (UI Automation) | The whole contract; Narrator and NVDA's view |

A GPUI window must pass on the same systems the egui window does. GPUI draws with Metal,
Vulkan or Direct3D 11; whether it draws under Xvfb and on a Windows computer without a GPU is
checked first, in G6 (section 5, check 2), because every Linux row above depends on it.

## 4. Performance: the egui baseline (G5) and the GPUI comparison (G6, G7)

Measured by `scripts/ui-bench.py` through the control port only, so that the same script
measures either window: release build, a home seeded once with N rows (added for later, against a
local server) and copied for each run, three runs, medians. Each figure's method is in the
script's opening comment.

egui, measured 2026-11-10 at 794799c (`cargo build --release -p dm-desktop`, 18.0 MB binary), on a
MacBook Air M1 (MacBookAir10,1), macOS 27.0.1, a visible window:

| Measure | How | egui, N = 100 | 1,000 | 10,000 | GPUI |
|---|---|---|---|---|---|
| Start to first drawn frame | spawn to the first `ui.texts` with words | 0.33 s | 0.37 s | 0.44 s | |
| Idle CPU, 60 s, nothing running | `ps` CPU time, % of one core | 0.2 % | 0.3 % | 1.3 % | |
| Resident at rest | `ps` rss after the idle spell | 113 MB | 119 MB | 136 MB | |
| Footprint at rest | macOS `footprint` | 80 MB | 80 MB | 93 MB | |
| `ui.scroll` 10 lines | until the words on screen change | 0.03 s | 0.04 s | 0.05 s | |
| Find the last row | Cmd-F, its name typed, Find next, until it is selected | 0.12 s | 0.13 s | 0.36 s | |
| CPU with 20 downloads at 1 MB/s each | `ps` CPU time over 30 s, % of one core | 12.0 % | 7.6 % | 27.5 % | |

The GPUI shell, measured the same way the same day (dm-gpui at c7f81e3, release build, 11.6 MB
binary against egui's 18.0 MB): it draws a header and the list only, so these are its floor, not
a slice's result. Each slice is held to beating egui's column with what it adds.

| Measure | GPUI shell, N = 100 | 1,000 | 10,000 |
|---|---|---|---|
| Start to first drawn frame | 0.32 s | 0.34 s | 0.43 s |
| Idle CPU | 0.2 % | 0.3 % | 1.2 % |
| Resident at rest | 96 MB | 99 MB | 115 MB |
| Footprint at rest | 45 MB | 46 MB | 67 MB |
| `ui.scroll` | not comparable: the shell's words then came from the scroll state, not the painted frame (they are recorded at layout since) | | |
| Find | not in the shell | | |
| CPU with 20 downloads | 10.5 % | 4.6 % | 9.3 % |

At 10,000 rows the shell's share with 20 downloads is a third of egui's (9.3 % against 27.5 %)
and its footprint 28 % less; idle CPU is the same (1.2 % against 1.3 %), which puts that cost in
dm's own work over the rows (the keep-awake watch lists the queue every 5 s), not in drawing.

What the numbers say for GPUI: idle CPU and the busy share grow with the list (the egui window
lays the visible table out each frame, and each read compares every row), so the GPUI table is
held to doing work for visible rows only (G7 slice 2); Find at 10,000 rows is a third of a
second, the application's `next_match` over every name.

Known figures to compare with: the application layer's tick at 10,000 rows is 2.8 ms
(`EVENT_FLOW.md`); a 5,060-row window answered a call in a median 3 ms, worst 19 ms (Linux,
`stress-check.py window`); the window at rest uses about 64 to 76 MB on Windows 11
(`memory-check.py`). The GPUI window must not be slower on any line by more than 10 %, nor
change download throughput (`stress-check.py`'s phases), without the maintainer's word.

## 5. The GPUI shell's checks (G6, Phase B)

Each pass or fail, per system (macOS, Windows, Linux X11, Linux Wayland), written into this
table when run. The source of each check is GPUI_DEPENDENCY_DECISION.md section 5 and
PLATFORM_GAP_ANALYSIS.md section 11.

| # | Check | How | macOS | Windows | X11 | Wayland |
|---|---|---|---|---|---|---|
| 1 | A screen reader reaches OK and Cancel in a dialog that is a window of its own, and the tree survives windows opening and closing 50 times | `ui.accessibility` (macOS), `windows-a11y-check.py`, Orca/AT-SPI as `linux-gui-check.sh` reads it | **pass** (2026-11-10: `dm-gpui --accessibility --churn`) | **pass** (2026-11-11: `windows-a11y-check.py` 5 of 5 dialogs through UI Automation; after 50 Add URL windows opened and closed, its OK and Cancel still read) | **pass** (2026-11-11, `linux-gui-check.sh`: the window and every button named over AT-SPI, a dialog's OK and Cancel in its own window, at their places once GPUI tells AccessKit where the window is, `vendor/gpui-pre-linux`; the 50-window churn not run here) | not run |
| 2 | Draws with no GPU | Xvfb with Mesa (llvmpipe/lavapipe) in `linux-human-qa.sh`'s image; a Windows VM without a GPU | n/a | by reading: Direct3D 11 on the first adapter that makes a device, Windows' own software adapter (WARP) listed last by DXGI; not run here (this computer has a GPU), CI's GPU-less runner runs the installer's `smoke.py` | **pass** (Mesa's llvmpipe, GL through EGL, under Xvfb: `linux-human-qa.sh`, `linux-gui-check.sh`) | **pass** (sway with no GPU: `linux-wayland-close-check.sh`) |
| 3 | A dialog opens over the middle of the main window, is modal, and can be moved outside it (B42) | `ui.windows`, `ui.move_window` |  | **pass** (2026-11-11: `human-qa.py` B42: over the middle of dm's window, a top-level window with no owner, moved and dragged by its title bar outside dm's window, Options holding dm's window) | **pass** (`human-qa.py` B42 under openbox, once a dialog's place is given to the window manager, `syswindow::place`) | n/a: the compositor places windows |
| 4 | Tray (`tray-icon`) works beside GPUI's run loop; a login launch makes no window until shown | `linux-gui-check.sh`'s tray steps; by hand elsewhere |  | **pass** (2026-11-11: `--hidden` makes no window and the tray is made, no problem said; Show brings the window; its close button asks "Keep running in the tray?"; the tray's menu itself by hand) | **pass** (`linux-gui-check.sh` 122 of 122: the tray's menu, a host that comes late or restarts, closing with no host quits, a login launch with no host opens its window) | **pass** (closing keeps dm in the tray, `linux-wayland-close-check.sh`) |
| 5 | Drop in: files, a link, text; drag out: a finished file | `linux-drag-check.sh`, `linux-wayland-drag-check.sh`, `drag-out-check.py` | drop in: coded (`drops.rs`, a drag destination on GPUI's window), not driven here: real input needs the Accessibility permission this terminal lacks; drag out: G10 | drop in: dm's own OLE target on GPUI's window (`dm_desktop_base::drops::win`, the egui window's), not driven here; drag out: not in the GPUI window on any system | drop in **pass** (a file of links; a link from Firefox, `linux-drag-check.sh`, with `vendor/gpui-pre-linux`); drag out **fail**: not built in the GPUI window yet (inventory 53) | drop in **pass** (`linux-wayland-drag-check.sh`); drag out **fail**, as on X11 |
| 6 | Text input: IME (Chinese, Japanese), Arabic and Hebrew in reading order | By hand with the system's input methods | | | | |
| 7 | Size: the release package against today's (`.dmg` 9.7 MB, `.deb` 12.4 to 13.3 MB, setup `.exe` 9.9 MB, 0.9.0) | `cargo packager` on the release profile | `.dmg` 10.4 MB (2026-11-10, `make app-gpui`, section 7) | setup `.exe` 10.4 MB (2026-11-11; the program 32.7 MB) | | |

D1 (go): check 1 passes on all four, or a patch to GPUI kept in `desktop/vendor/` makes it pass.

Findings so far (2026-11-10, `desktop/crates/dm-gpui` on `gpui-pre` and `gpui-pre-platform`
0.3.12, the pins of GPUI_DEPENDENCY_DECISION.md 3.1 without `gpui-component` yet):

- **Builds.** macOS (Rust 1.99.1, Apple M1): `cargo build -p dm-gpui` in 1 min 34 s from cold;
  the window opens with macOS's own title bar and buttons, titled "dm". Windows:
  `windows-check.sh clippy` type-checks the workspace with dm-gpui, no warning (not linked or
  run). Linux: Debian 12 (`dm-linux-check`, aarch64) builds once `libfontconfig-dev`,
  `libfreetype-dev`, `libxkbcommon-dev`, `libxkbcommon-x11-dev`, `libwayland-dev`,
  `libxcb1-dev`, `libx11-xcb-dev` and `libvulkan-dev` are installed; the binary links `libxcb`,
  `libxkbcommon` and `libxkbcommon-x11` (the rest is loaded at run time). These go into the
  image and the `.deb`'s `depends` when the shell is packaged (check 7).
- **Check 1 on macOS: pass.** `dm-gpui --accessibility` reads what VoiceOver is given of each
  window through NSAccessibility from inside (`a11y_tree`, moved to dm-desktop-base, as
  `ui.accessibility` reads it for egui): the dialog's own window (`WindowKind::Dialog`) gives
  `AXWindow "Delete the file?"` with `AXButton "OK"` and `AXButton "Cancel"`. GPUI's AccessKit is
  on by default (`Application::with_platform`; Zed opts out with `new_inaccessible`) and wakes
  lazily: the first ask gets a placeholder, the real tree comes with the next frame. With
  `--churn`, 50 dialog windows opened, read and closed first: no panic, and the tree after is the
  same. Each element needs `.id(..)` and `.role(..)`; the name arrives as the AX title.
- **Layers and privacy.** `cargo xtask layers` knows dm-gpui (dm-core, dm-protocol, dm-shell,
  dm-desktop-base, never dm-desktop) and forbids `gpui`, `gpui_platform` and `gpui-component`
  to every crate that is not a window; the privacy check finds no crate in GPUI's tree that
  reports anywhere.
- **Windows (2026-11-11, Windows 11, 4 cores, Rust 1.99.1).** `cargo build -p dm-gpui` in
  35 min from cold (debug, 95 MB); GPUI links its own manifest (per-monitor DPI v2, asInvoker,
  common controls 6), so `dm-gpui/build.rs` adds only the icon and version. A release needs the
  Windows SDK's `fxc.exe` (GPUI compiles its Direct3D shaders with it), found by itself in the
  SDK. Launched: its window in Windows 11's colours and caption glyphs, 2 % of a core and
  33 MB private memory at rest (debug). Found and put right on the way: GPUI's Windows windows
  report no title (the port now keeps each window's), their bounds are the client area (the
  port reports and places the frame as it is seen), and a wake to act on now waited out a
  quarter-second coalescing behind a running download's changes (`list.rs`), where the egui
  window acts at once. `human-qa.py` on the debug build: 51 of 64 on its first run (nine rows
  only because it was not run under `python -X utf8`, so the browser checks' "—" was misread),
  then 63 of 64 twice, B36 failing once in each on the browser check's click-to-first-request
  limit (1000 ms on Windows; medians 1180 and 1009 ms, egui's debug build 805 ms on the same
  computer); see section 7 for the release installer's run.


## 6. Tests that move

| Today | Count (2026-11-09) | Where they go |
|---|---|---|
| `desktop/crates/dm-desktop/tests/*.rs` (egui_kittest and `DmApp`) | 485 `#[test]` | Assertions about the application (what a command does to rows, settings, files) move to dm-shell's tests in G3; assertions about the window are rewritten on GPUI's test context in the slice that draws them (G7) |
| dm-desktop unit tests | 216 | With their module (G2 moves the toolkit-free ones unchanged) |
| dm-shell, dm-core, dm-protocol tests | the rest | Unchanged |
| `human-qa.py` and the scripts | 64 rows and their checks | Unchanged: they speak to the control port, which every window answers (G4) |

## 7. Results

| Row | Date | Gate | Row's own checks |
|---|---|---|---|
| G1 | 2026-11-10 | fmt, clippy, 2135 tests; Windows clippy; `human-qa.py` 64/64 (macOS, with G2's first batch) | — |
| G2 | 2026-11-10 | fmt, clippy, 2135 tests; Windows clippy and `windows-modules-check.sh`; clippy on Linux (`dm-linux-check`); `human-qa.py` 64/64 on macOS; `linux-human-qa.sh` 64/64 | `--help` and MCP `initialize` + `tools/list` byte for byte as at 06b3659; `launch-latency.py` (debug builds, medians of 7) cold 122.8 → 120.3 ms, warm 52.6 → 55.3 ms; `handover-check.py` 67 of 68, LibreOffice failing the same at 06b3659 (gaps row 173, then numbered 166) |
| G3 | 2026-11-10 | fmt, clippy, 2160 tests; Windows clippy; `human-qa.py` and `linux-human-qa.sh` 64/64 at a9a9ebd and at 0b0a699 | Each rule moved has a dm-shell test (25 new tests) |
| G4 | 2026-11-10 | fmt, clippy, 2169 tests; Windows clippy; `human-qa.py` and `linux-human-qa.sh` 64/64 at 7cf34db, `human-qa.py` unchanged | Nine probe tests fix every answer's JSON, the parameter errors and which door answers which method |
| G7 slice 4 (macOS) | 2026-11-10 | fmt, clippy, 2185 tests; `human-qa.py` 64/64 for the egui window at 1711002 (its dialogs now on `dm_desktop_base::file_info`, `add_url` and `asks`) | Driven through the control port in the GPUI window: Add URL (nothing typed, a link typed, Return), Download details (the server's size, Start now, a name's clash and Replace), "Already in the list" (each answer, Remember my choice), B39 to B41 (each answer; B39 says "dl"), Quit from a dialog, SIGTERM with a sheet open (0.3 s), a question raised with dm in the background (asked once in front); each pictured and compared with the system's own dialog |
| G7 slice 6, row dialogs (macOS) | 2026-11-10 | fmt, clippy, 2198 tests; the egui window's Update download link, Move to queue, Move/Rename and Properties now on `dm_desktop_base::row_edits` (its `human-qa.py` run owed) | Driven through the control port in the GPUI window against a local server: Properties of a finished ZIP (every line, its Contents, the SHA-256 checked with "It matches.", Escape), Update download link on a failed row (the new address taken, the row retried to Complete), Move to queue ("Night" typed, Return), Move/Rename (the name without its ending chosen, `renamed.txt` on disk); a server that turns to 403 part-way opens Update download link by itself; each pictured |
| G7, About and the adding dialogs (macOS) | 2026-11-10 | fmt, clippy, 2199 tests (the secrets check then caught a login written into an address in a new test, built with `format!` since); the egui window's About, Help and series words now from `dm_desktop_base::project` | Driven in the GPUI window: Add numbered series (count line, the list, Add for later, "The range runs backwards."), Links on the clipboard (one listed offered unticked, Return), About dm (every line, Escape), a scripted double-click on a finished row refused ("Opening a file is done by a person, not by a script"); each pictured |
| G7 slice 7, Options (macOS) | 2026-11-10 | fmt, clippy, 2205 tests; the egui window's Options OK, Restore defaults and the lists' words now `dm_desktop_base::options` (its `human-qa.py` run owed) | Driven in the GPUI window: the six pages (B43) and Advanced's Network, Proxy and BitTorrent; Identify as dm only kept (B51); the Fast (16) preset kept (B56); Add a site limit; HTTP chosen in the proxy's pop-up through the port and OK refused on Advanced with "Enter the proxy's host…"; a link copied offered above the list and Dismissed; each page pictured. The port now reads only the words in view, scrolls the window they are in, and chooses in a pop-up |
| G7 slice 8, the tray and closing (macOS) | 2026-11-10 | fmt, clippy, 2206 tests; the egui window's tray words and tooltip now `dm_desktop_base::tray` | Driven in the GPUI window: the status item made (a template image in the menu bar, pictured); `ui.window tray` puts the window away and a second launch's `show` brings it back; ⌘W asks "Keep running in the tray?" as a sheet, Keep running hides it and keeps `on_close: tray`; Download details for a hand-off closed by ⌘W runs its Cancel (nothing added) and the next hand-off is asked at once |
| G7, `human-qa.py` on the GPUI window (macOS) | 2026-11-10 | fmt, clippy, 2206 tests | `DM_BINARY=dm-gpui python3 scripts/human-qa.py`, in sight: 54 of 64 rows held on the first run (browser checks 68 of 68 and 70 of 70, every file type, YouTube); the failures were the port's (menus, the rows' words, dialog names) and three of the window's (Delete Completed's words, Options holding the window, Resume's progress window), each then put right and checked by itself; B25 waits for the GPUI window's own pictures (D3) |
| G7/G8, the drop target and `human-qa.py`'s second run (macOS) | 2026-11-10 | fmt, clippy, the workspace's tests; a unit test of a link dropped from a browser (a stand-in drag with a pasteboard), as the egui window's | `human-qa.py --save-baseline` on the GPUI window: 63 of 64 (browser checks 68 of 68 and 70 of 70, every file type, YouTube; B25's pictures saved under `docs/behaviour-baseline/gpui/macos/` and looked at); B48 failed only because Escape did not close the progress window B53 had opened, so the keys went to it; Escape closes it since, checked by itself. View › Drop target shows the card (pictured) and hides it again |
| G7/G8, `--headless`, When done and the menus (macOS) | 2026-11-10 | fmt, clippy, the workspace's tests (four of `dm_desktop_base::headless`'s answers, four of the menus' ticks and words, `dm-gpui/tests/quit.rs` signalling it headless) | `DM_BINARY=dm-gpui python3 scripts/app-check.py`: 98 of 98 (the window driven over the port, headless and MCP, the command line, the watched folder; its first run failed only "a menu opens and lists its items", the menu's items then lacking "…" and the port not listing a menu it opened); `smoke.py dm-gpui` passes (native host, the window, no window, 4 MiB byte-exact); When done › Exit driven in the window: the countdown sheet, Now ends dm with status 0 and `control.port` gone |
| G7, the View menu (macOS) | 2026-11-10 | fmt, clippy, the workspace's tests (the View menu's ticks, a heading dropped on another, the toolbar's styles by name) | Driven in the GPUI window through the menu bar over the port: View › Columns › Size hid the column (saved, gone from the headings), Customize toolbar › Text only and › Small buttons drew the toolbar so (pictured), › Stop All hid the button, Language › Deutsch put every word in German and Ansicht › Sprache › English back, Sort by › Sort by size sorted |
| G7, Resume's words and a toolbar button moved (macOS) | 2026-11-10 | fmt, clippy, the workspace's tests (`rows::resume_word`, the File menu's first item, `moved_onto` for headings and buttons) | Driven in the GPUI window: a download that failed (a 404), chosen: File's first item and the row menu read Retry. A button dragged onto another is the heading's drag, unit-tested; by hand, owed |
| G7, the window's frame kept (macOS) | 2026-11-10 | fmt, clippy, the workspace's tests on the tree merged with `main` at a6faac1 (`frame.rs`: a frame read back as kept, one off every display or too small not used) | Driven: started in the middle (309,165 to 1209,765), Window › Zoom (75,30 to 1440,900), Exit, started again on the same home: the same frame |
| G7, `human-qa.py`'s third run on the GPUI window (macOS) | 2026-11-10 | the tree synced with `main` at a6faac1 and the window's frame kept (c72ff2c), the extension packaged from it | `DM_BINARY=dm-gpui python3 scripts/human-qa.py`, in sight: **64 of 64 rows hold** (browser checks 68 of 68, every file type, YouTube) |
| G8, the release bundle (macOS) | 2026-11-10 | `make app-gpui` (release flags, LTO): `dm.app` and `dm_0.9.0_aarch64.dmg` (10.4 MB), signed ad hoc, `com.dm.desktop`, the `magnet:` scheme, no build paths in the binary | From the .dmg, as the release workflow does: `smoke.py` passes (native host, the window, no window, 4 MiB byte-exact over ranges); `human-qa.py` on that app: **64 of 64 rows hold** (browser checks, every file type, YouTube) |
| G7/G8, the top bar's menus on every system; Windows and Linux coded here | 2026-11-10 | fmt, clippy on macOS; `windows-check.sh clippy` (the workspace for `x86_64-pc-windows-msvc`); clippy for dm-gpui in Debian 12 (`linux-check.sh`'s image); the workspace's tests | macOS: `human-qa.py --save-baseline` **64 of 64** with the menus in the top bar (B25's GPUI pictures drawn again and looked at). Linux (Debian 12, Xvfb, no GPU): the GPUI binary passes `smoke.py` (native host, the window, no window, 4 MiB byte-exact) and `app-check.py` in full (its first run: a `--hidden` launch's window shown, dialogs unnamed, no screenshot; each put right). Windows: built and linted only; run on a Windows computer |
| G8/G9, macOS's default switched; each system's colours; shared drops (macOS) | 2026-11-11 | d6a0365: fmt, clippy, `windows-check.sh clippy`, Linux clippy, 2245 workspace tests | `human-qa.py` on the egui window **64 of 64** (B10 and B63 had failed only because the window, behind the browser, drew no frames for the port's clicks; the script brings it in front first); on the GPUI window, now the default, **64 of 64** (its first run failed B01, B25, B42 and B44 because that bring-forward landed over Add URL; the script lets it settle first) |
| G6 to G9, Windows (Windows 11, 4 cores) | 2026-11-11 | fmt; `cargo clippy --workspace --all-targets -- -D warnings` on Windows; `cargo test -p dm-gpui` (18), `-p dm-desktop-base --lib`, `-p dm-desktop --test i18n --test packaging`; the full suite left to the Mac (the maintainer's word) | Debug build, `python -X utf8 scripts/human-qa.py` with `DM_BINARY` naming dm-gpui: 51 of 64 (nine browser rows misread without `-X utf8`), then 63 of 64 twice (B36's click-to-first-request time, medians 1180 and 1009 ms against Windows' 1000); `--save-baseline` drew `docs/behaviour-baseline/gpui/windows/` (looked at). Release installer (`cargo packager -p dm-gpui --formats nsis`, 10.4 MB against 0.9.0's 9.9 MB; `dm-gpui.exe` 32.7 MB) installed over 0.9.0: `smoke.py`, `app-check.py`, `windows-a11y-check.py` 5 of 5, `out-of-box-check.py` 30 of 30, `portable-check.py` pass; `human-qa.py` 63 of 64 (B36's click timing 1.76 s on its first pass, straight after install with five review agents reading the tree, 0.71 s on its second; B25 0.00, 0.00 and 0.09 % against the Windows pictures). Then fixed from a review: the language at startup, greyed controls, High Contrast redrawn; a whole run on an installer with them is owed |
| The audit's rows A1, P1, D1, U1, U3 to U9, S1, S7, S10, S14, S17, F3, F5, F7, Y6, Y12 (macOS) | 2026-11-12 | b7e1a3d1: `cargo xtask check` from a clean build, every check passed, 2294 of 2294 tests | Debug build, `human-qa.py` in sight: 66 of 66 rows hold (B25's pictures against `docs/behaviour-baseline/gpui/macos` among them) |
| The security audit's fixes; the side panel's pictures (macOS) | 2026-11-12 | `cargo xtask check`: 2299 of 2300, the one failure (a literal login in a new test, `xtask secrets`) fixed and passing | `human-qa.py --save-baseline` (B25's pictures taken again with the side panel's pictures, the maintainer's word of that day), then `human-qa.py`: 66 of 66 rows hold |
| Rows S15, S19, Y5, Y11, Y13, Y14, Y17, Y19, Y23, Y25, F10, U2, item 28 (macOS), coded first and tested together | 2026-11-12 | e24389ab: `cargo xtask check`, every check passed, 2314 of 2314 tests | `human-qa.py`: 66 of 66 rows hold |
| The side panel's pictures in one colour (macOS) | 2026-11-12 | `cargo clippy` of dm-gpui | `human-qa.py --save-baseline` (B25's pictures taken again, the maintainer's word: "side pannel icon should be monocolor"), then `human-qa.py`: 66 of 66 rows hold |
