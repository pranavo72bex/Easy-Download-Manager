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
| 3 | `cd desktop && cargo build -p dm-desktop && cd .. && sh scripts/package.sh && python3 scripts/human-qa.py` | macOS, visible window: `64 of 64 rows hold` |
| 4 | `sh scripts/linux-human-qa.sh` (Docker running) | Debian 12 on X11, a build of the checkout: `64 of 64 rows hold` |

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
| 1 | A screen reader reaches OK and Cancel in a dialog that is a window of its own, and the tree survives windows opening and closing 50 times | `ui.accessibility` (macOS), `windows-a11y-check.py`, Orca/AT-SPI as `linux-gui-check.sh` reads it | **pass** (2026-11-10: `dm-gpui --accessibility --churn`) | | | |
| 2 | Draws with no GPU | Xvfb with Mesa (llvmpipe/lavapipe) in `linux-human-qa.sh`'s image; a Windows VM without a GPU | n/a | | | |
| 3 | A dialog opens over the middle of the main window, is modal, and can be moved outside it (B42) | `ui.windows`, `ui.move_window` | | | | |
| 4 | Tray (`tray-icon`) works beside GPUI's run loop; a login launch makes no window until shown | `linux-gui-check.sh`'s tray steps; by hand elsewhere | | | | |
| 5 | Drop in: files, a link, text; drag out: a finished file | `linux-drag-check.sh`, `linux-wayland-drag-check.sh`, `drag-out-check.py` | | | | |
| 6 | Text input: IME (Chinese, Japanese), Arabic and Hebrew in reading order | By hand with the system's input methods | | | | |
| 7 | Size: the release package against today's (`.dmg` 9.7 MB, `.deb` 12.4 to 13.3 MB, setup `.exe` 9.9 MB, 0.9.0) | `cargo packager` on the release profile | | | | |

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
| G2 | 2026-11-10 | fmt, clippy, 2135 tests; Windows clippy and `windows-modules-check.sh`; clippy on Linux (`dm-linux-check`); `human-qa.py` 64/64 on macOS; `linux-human-qa.sh` 64/64 | `--help` and MCP `initialize` + `tools/list` byte for byte as at 06b3659; `launch-latency.py` (debug builds, medians of 7) cold 122.8 → 120.3 ms, warm 52.6 → 55.3 ms; `handover-check.py` 67 of 68, LibreOffice failing the same at 06b3659 (gaps row 166) |
| G3 | 2026-11-10 | fmt, clippy, 2160 tests; Windows clippy; `human-qa.py` and `linux-human-qa.sh` 64/64 at a9a9ebd and at 0b0a699 | Each rule moved has a dm-shell test (25 new tests) |
| G4 | 2026-11-10 | fmt, clippy, 2169 tests; Windows clippy; `human-qa.py` and `linux-human-qa.sh` 64/64 at 7cf34db, `human-qa.py` unchanged | Nine probe tests fix every answer's JSON, the parameter errors and which door answers which method |
| G7 slice 4 (macOS) | 2026-11-10 | fmt, clippy, 2185 tests; `human-qa.py` 64/64 for the egui window at 1711002 (its dialogs now on `dm_desktop_base::file_info`, `add_url` and `asks`) | Driven through the control port in the GPUI window: Add URL (nothing typed, a link typed, Return), Download details (the server's size, Start now, a name's clash and Replace), "Already in the list" (each answer, Remember my choice), B39 to B41 (each answer; B39 says "dl"), Quit from a dialog, SIGTERM with a sheet open (0.3 s), a question raised with dm in the background (asked once in front); each pictured and compared with the system's own dialog |
| G7 slice 6, row dialogs (macOS) | 2026-11-10 | fmt, clippy, 2198 tests; the egui window's Update download link, Move to queue, Move/Rename and Properties now on `dm_desktop_base::row_edits` (its `human-qa.py` run owed) | Driven through the control port in the GPUI window against a local server: Properties of a finished ZIP (every line, its Contents, the SHA-256 checked with "It matches.", Escape), Update download link on a failed row (the new address taken, the row retried to Complete), Move to queue ("Night" typed, Return), Move/Rename (the name without its ending chosen, `renamed.txt` on disk); a server that turns to 403 part-way opens Update download link by itself; each pictured |
