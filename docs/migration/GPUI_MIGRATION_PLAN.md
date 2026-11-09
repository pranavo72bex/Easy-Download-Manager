# GPUI migration plan

Status: **Phase 0 (audit) done, 2026-11-09, on branch `gpui-migration`. Nothing
of the source has changed.** Next: Phase A, below.

This page turns the specification the maintainer gave on 2026-11-09 ("egui → GPUI
Migration") into steps on this repository. It rests on four audit documents:

| Document | What it holds |
|---|---|
| [CURRENT_UI_ARCHITECTURE.md](CURRENT_UI_ARCHITECTURE.md) | The egui presentation as the code runs it: modes, threads, the tick, state ownership, every coupling to egui, and where the architecture docs and the code disagree (section 9) |
| [EGUI_FEATURE_INVENTORY.md](EGUI_FEATURE_INVENTORY.md) | 151 features in 18 areas, each tied to its contract row(s), module, application API, proposed GPUI view, platform dependency and tests; all 64 contract rows mapped |
| [PLATFORM_GAP_ANALYSIS.md](PLATFORM_GAP_ANALYSIS.md) | 61 OS integrations, each marked Keep / Adapt / Redo for GPUI, with GPUI's support checked in Zed's source; 12 ranked risks and a Phase 1 check list |
| [GPUI_DEPENDENCY_DECISION.md](GPUI_DEPENDENCY_DECISION.md) | The distributions compared on 2026-11-09 and a **proposed** pin, to be confirmed by Phase B's shell |

The behavioural reference is [docs/behaviour-contract.md](../behaviour-contract.md)
(64 rows) and `scripts/human-qa.py`, which checks all of them through the
control port on macOS, Windows and (since 0.9.0) Linux
(`scripts/linux-human-qa.sh`). The migration is done when a GPUI window holds
the same 64 rows on every system, not when it looks done.

## What the audit found that shapes the plan

1. **The seam is real but not finished.** `dm_shell::app` holds the queue, the
   session, the commands and every server request, with no window toolkit
   (`cargo xtask layers`). But about **2,500 lines of application rules still
   sit in `DmApp`**: question detection (B39 to B41), pending renames,
   once-only schedule dates, saved searches, the weekly update check, one-time
   hints, the hand-off cancel counter, the script guard
   (CURRENT_UI_ARCHITECTURE.md 4.2). A second window would have to copy them.
2. **About 7,100 lines of `dm_desktop` never touch egui** (`control`, `mcp`,
   `cli`, the `Desktop` Platform, `power`, `autostart`, `awake`, `wake`,
   `quiet`, `region`, `details`, `i18n`, most of `format`). `MODULE_BOUNDARIES.md`
   forbids a GPUI crate to depend on `dm_desktop`, so they must move first.
3. **The QA oracle is the control port's `ui.*` methods**, which today read
   egui's painted text shapes and feed it input one step per frame.
   `human-qa.py` and MCP depend on their exact JSON. A GPUI window that answers
   them the same way is checked by the same 64 rows unchanged; one that does
   not cannot be checked at all.
4. **The architecture docs and the code disagree in 12 places**
   (CURRENT_UI_ARCHITECTURE.md 9): `changes::take` is never called,
   `schedule_tick` runs before `ran_dry`, `DmApp::logic` runs twice per painted
   frame, the egui window never subscribes to the typed events, and comments
   are stale (`i18n.rs` says Latin and Cyrillic only; `control.rs` lists 7 of
   24 methods).
5. **GPUI is feasible only if its accessibility holds.** The contract requires
   screen readers to reach every dialog, each a window of its own (B42).
   GPUI's AccessKit support arrived in 2026, is opt-in control by control, has
   open bugs, and is absent from crates.io's `gpui` 0.2.2. This is the deciding
   risk, and it is checked first (Phase B).
6. **Other hard risks** (PLATFORM_GAP_ANALYSIS.md 10): no software rendering
   path proven for Xvfb/Docker and GPU-less Windows VMs (every Linux check runs
   there); X11 ignores a requested window position (B42's "over the middle",
   Zed issue #64447); GPUI's macOS dialog kind is a sheet (B42 needs a movable
   window); links and text cannot be dropped (only files), and drag-out exists
   only on macOS and Wayland; no tray (keep `tray-icon`); Linux file dialogs
   need the desktop portal (keep `rfd`); dm's three vendored winit/egui-winit/
   eframe patches do not carry over. 485 window tests are written on
   `egui_kittest`.
7. **The proposed dependency is one maintainer's.** `gpui-pre` (weekly
   snapshots of Zed's GPUI) and `gpui-component` are both published by one
   person (checked on crates.io, 2026-11-09). `gpui-ce` 0.3.0 is the fallback.

## Decisions that are the maintainer's

These are asked, not assumed, when their phase is reached:

| # | Decision | When |
|---|---|---|
| D1 | Go or no-go on GPUI after Phase B's shell, on its pass/fail list (accessibility first) | End of Phase B |
| D2 | The distribution and exact pin (proposed: `gpui-pre =0.3.12` + `gpui-component =0.9.1`) | End of Phase B |
| D3 | New B25 baseline pictures: a GPUI window will not match egui's pixels. The contract requires approval in words | When the first GPUI window passes the other rows |
| D4 | Any contract row GPUI cannot hold as written (for example B42's position on X11, or drag-out on X11), with the fallback proposed | As found |
| D5 | Switching the default window to GPUI, then removing egui (separate change) | Phase E |

Until D1, nothing in the shipping app changes behaviour: Phase A moves code
behind the same window, and Phase B is a separate binary.

## Phases

Every step keeps `cargo test --workspace` green, and every step that touches
`desktop/` is held to the whole of `human-qa.py` on macOS and
`linux-human-qa.sh`, as CLAUDE.md requires.

### Phase A: finish the seam, under the egui window (no GPUI yet)

Worth doing whatever D1 decides: it is the architecture the project already
chose (`docs/architecture/`), finished.

| Step | What | Done when |
|---|---|---|
| A1 | Make the architecture docs say what the code does (section 9's twelve rows), or the code what the docs say where the docs are right: call `changes::take`, run `DmApp::logic` once per frame (measured first: it is behaviour if a frame depends on it), fix the stale comments | Docs and code agree; 64/64 |
| A2 | A toolkit-free crate for the desktop's non-drawing parts (proposed `dm-desktop-host`, or into dm-shell where sockets already live): `control` (minus `ui.*`), `mcp`, `cli`, `Desktop` Platform, `power`, `autostart`, `awake`, `wake`, `quiet`, `region`, `details`, `i18n` (minus `pick`), `format`. `cargo xtask layers` forbids it egui | dm-desktop is drawing only; 64/64 |
| A3 | Move CURRENT_UI_ARCHITECTURE.md 4.2's rules into `dm_shell::app`, one at a time, each with its test moved or written | `DmApp` holds view state only |
| A4 | Define the control port's `ui.*` answers as a toolkit-free contract: a trait the window implements (texts with rects and window, click/type/key/scroll, inspect, select, place, windows, screenshot, accessibility), with the JSON shapes fixed by tests in the shared crate | egui implements the trait; human-qa unchanged; 64/64 |
| A5 | Measure the egui baseline the spec's Phase 8 asks for, with a script that uses only the control port (so it measures GPUI the same way later): start to first usable frame, idle CPU, memory (`memory-check.py`), frame time with 20 active downloads, scroll and search at 100 / 1,000 / 10,000 rows (`stress-check.py window` seeds rows) | Numbers recorded in `GPUI_TEST_PLAN.md` |

### Phase B: the GPUI shell, as a go/no-go spike

A separate crate and binary (proposed `desktop/crates/dm-gpui`, binary
`dm-gpui`), on the proposed pin, depending on dm-shell, dm-core and A2's crate
only. It opens the main window and one dialog window, runs the A3 tick on one
foreground waker, keeps the tokio runtime for the engine, and implements a
first slice of A4's `ui.*`.

It is judged on GPUI_DEPENDENCY_DECISION.md section 5's twelve checks and
PLATFORM_GAP_ANALYSIS.md section 11, each pass or fail, on macOS, Windows,
Linux X11 and Wayland (including under Xvfb in `linux-human-qa.sh`'s image):

1. Screen readers (VoiceOver, NVDA, Orca) reach OK and Cancel in the dialog's own window, through `human-qa.py` B42, `windows-a11y-check.py`, `linux-gui-check.sh`. **A fail here is a no-go** unless the maintainer accepts a patched GPUI (as eframe is patched today).
2. Software rendering under Xvfb/Mesa and on a GPU-less Windows VM.
3. Dialog placement and modality (B42) on X11, Wayland, macOS, Windows.
4. Tray (`tray-icon`) beside GPUI's run loop; hidden login launch.
5. Drag and drop in (files, links, text) and out.
6. Text input and IME; RTL text.
7. Release size against today's 10 to 13 MB; `cargo packager` outputs unchanged.

Output: `GPUI_DEPENDENCY_DECISION.md` confirmed or changed, a pass/fail table,
and D1 and D2 asked.

### Phase C: the window in slices (only after D1 is go)

The spec's eight slices, each held to the contract rows it covers
(EGUI_FEATURE_INVENTORY.md's index), run side by side with egui by
`human-qa.py` with `DM_BINARY` naming either window. A slice is done when its
rows hold in GPUI on every system, not when it draws:

| Slice | Rows (main ones; the engine's rows B06 to B15, B21, B22 are shown, not drawn, and hold through every slice) |
|---|---|
| 1 Main window, toolbar, side panel, status bar, theme | B45, B63, B42 (main window) |
| 2 Download table: virtualized, stable `Id`s, coalesced progress (250 ms, as today) | B03, B04, B05, B16, B45, B46, B48 |
| 3 Row commands, context menus, bulk actions | B17, B18, B23, B24, B38, B55 |
| 4 Add URL, Download details, the questions | B01, B02, B39, B40, B41, B44, B47, B50 |
| 5 Queues, Scheduler, speed limiter, Grabber | B19, B20, B64, B49, B62, B65, B60, B61, B52 |
| 6 Progress window, failure windows, properties, playlist | B53, B54 |
| 7 Options, every tab | B43, B51, B56, B57 |
| 8 Dialogs as windows, toasts, tray, shortcuts, focus, accessibility, the pictures (D3) | B42, B25, B52 (tray) |
| (the browser's rows) | B26 to B37, B59: the extension and the app's hand-off, unchanged; held by `human-qa.py`'s browser half in every slice |

The window tests move with their slice: toolkit-free assertions to the shared
crates, window assertions to GPUI's test context (Phase B decides which).

### Phase D: platforms and packaging

PLATFORM_GAP_ANALYSIS.md's "Adapt" and "Redo" rows: drag-out per OS, link
drops, taskbar progress and jump list, Dock, title-bar colours, the .deb's
dependencies for GPUI's renderer, the macOS bundle and the Windows installer.
Done when the release workflow's packages, downloaded from a draft, hold 64/64
(`linux-human-qa.sh DM_RELEASE=…`, the `.dmg` on macOS, Windows' run).

### Phase E: parity, switch, remove

`EGUI_PARITY_REPORT.md` row by row; D5 asked; the default switched in one
change, egui removed in a later one (dependencies, the three vendored patches,
kittest, `--headless`'s stand-in `egui::Context`, docs, CI).

## Sizes, for scale

dm-desktop is 35,818 lines (2026-11-09), of which about 7,100 move in A2 and
about 2,500 in A3; the drawing that is rewritten is the remaining ~26,000,
plus 485 window tests. Phase A is several sessions of work; Phase B one to two;
Phase C is the bulk.

## Still to write (from the spec's list)

`GPUI_UI_ARCHITECTURE.md` (Phase B), `GPUI_TEST_PLAN.md` (A5 and Phase B),
`EGUI_PARITY_REPORT.md` (Phase E).
