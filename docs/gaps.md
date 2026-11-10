# Gaps

The work list: what is known to be missing from a production-ready download
manager and is both wanted and possible for free. A row says what is missing
and how it closes; when it closes, its row is deleted (what was done is in
`CHANGELOG.md`, `docs/parity.md` and git history).

| Status       | Meaning                                                      |
|--------------|--------------------------------------------------------------|
| OPEN         | Nothing done, or done but never run                          |
| PARTIAL      | Works and is tested, but not everywhere it must              |
| BLOCKED      | Needs something this project cannot get for free, or a person |

## State (2026-11-09)

The GPUI migration (rows G1 to G9, maintainer, 2026-11-09; `docs/migration/`) is the work that
can be done here; G2 to G5 come first and need no decision (G1 closed 2026-11-10). The other rows need a Windows
computer, a device or a person.
Checks on 2026-11-09 on the Mac: `cargo test --workspace` passes, line coverage 93.7% (CI's
floor 91%), the extension's 92 Edge checks, `video-sites-check.py` signed out (P0 7 of 7),
`human-qa.py` 64 of 64 rows. The 0.9.0 release's own files, downloaded from its draft (digests
as `latest.json` gives them), with extension 3.3.0's released zip: the macOS `.dmg` 64 of 64 rows;
the arm64 `.deb` installed by apt on Debian 12 with Chromium in Edge's place, 64 of 64 with
`DM_RELEASE=desktop-v0.9.0 sh scripts/linux-human-qa.sh` (B25 against the Linux baselines).

Removed from the list on 2026-11-09 at the maintainer's word (`docs/decisions.md`, "The work
list keeps only what can be done"): every check that needs a site sign-in (Facebook, Instagram,
Reddit, YouTube signed in, rows 63, 71, 75, 110, 122, 142, 152), one that needs reference A's
dialogs set by hand (row 65's proxies) or a person's hands or eyes on the window (rows 60, 66,
72f), the blocked proof-of-origin token (row 37), a reporter's confirmation (row 30), the site
rules that could only be checked signed in (rows 68 and 69), and the notes that were not work
(their findings are in `docs/reference-behavior.md`).

## Open work

| #   | Gap | Status | How it closes |
|-----|-----|--------|---------------|
| 5   | 89 strings (the 71 reworded in 0.7.0, the 14 of 2026-10-30 for the quality list, the part fields and the Playlist dialog, and Take out's 4) were put into the other twelve languages by a machine pass and have not been read by a native speaker | BLOCKED: native readers | One reader per language corrects them (`git log -p --follow -- desktop/crates/dm-desktop-base/i18n`; before 2026-11-10 they were under `dm-desktop/i18n`) |
| 20  | Stress phases not run on Windows: `fulldisk` (needs a small volume) and `torrent` (needs a sandbox off the public network); the clipboard offer is covered on Linux only, as on macOS and Windows the only clipboard is a person's | PARTIAL | On Windows: `fulldisk` with a VHD as the download folder, `torrent` in a sandbox; the clipboard phase on a machine whose clipboard nobody is using |
| 39  | Secrets sealed under a key in the system's store: proved on Windows, macOS and Linux, except a copy of dm's home started under another Windows account, which must not open them and must say so once | PARTIAL | On a Windows computer with a second account |
| 47  | The second network path (Options › Advanced › Network) is tested with a second local address only | PARTIAL | One run with a phone's connection beside the computer's own, recording the speeds and each path's share |
| G2  | GPUI migration, A2: about 7,100 lines of `dm_desktop` never touch egui (`control` but `ui.*`, `mcp`, `cli`, the `Desktop` Platform, `power`, `autostart`, `awake`, `wake`, `quiet`, `region`, `details`, `i18n` but `pick`, `format`), and a GPUI crate may not depend on `dm_desktop` | OPEN | They live in a toolkit-free crate that `cargo xtask layers` forbids egui; dm-desktop is drawing and `ui.*` only; the binary's modes byte for byte; tests and both QA runs 64/64 |
| G3  | GPUI migration, A3: about 2,500 lines of application rules still in `DmApp` (`CURRENT_UI_ARCHITECTURE.md` 4.2): question detection (B39 to B41), pending renames, once-only schedule dates, saved searches, the weekly update check, one-time hints, the hand-off cancel counter, the script guard, the inbox order files | OPEN | Each in `dm_shell::app` with its test, `DmApp` holding view state only; tests and both QA runs 64/64 |
| G4  | GPUI migration, A4: the control port's `ui.*` answers (what `human-qa.py` and MCP read) are written against egui's painted shapes, not as a contract another window can implement | OPEN | A toolkit-free trait and JSON shapes fixed by tests in the shared crate, implemented by the egui window; `human-qa.py` unchanged and 64/64 |
| G5  | GPUI migration, A5: no baseline of the egui window's own costs at 100 / 1,000 / 10,000 rows (start to first frame, idle CPU, memory, frame time with 20 downloads, scroll and search) | OPEN | A script that measures through the control port only, its egui numbers recorded in `docs/migration/GPUI_TEST_PLAN.md` |
| G6  | GPUI migration, Phase B: no GPUI shell yet to decide go or no-go (screen readers in every dialog window first; software rendering under Xvfb and GPU-less Windows; B42 placement; tray; drag and drop; IME; size) | OPEN | `desktop/crates/dm-gpui` on the proposed pin, each check of `docs/migration/GPUI_MIGRATION_PLAN.md` Phase B pass or fail on macOS, Windows, X11 and Wayland; the maintainer's D1 (go) and D2 (pin) |
| G7  | GPUI migration, Phase C: the window in eight slices (main window; table; row commands; Add URL and the questions; queues and Scheduler; progress and failure windows; Options; dialogs, tray, shortcuts, accessibility, B25 pictures) | BLOCKED: D1 | Each slice's contract rows (`docs/migration/GPUI_MIGRATION_PLAN.md` Phase C) hold in GPUI on every system, run side by side with egui by `human-qa.py`; D3 for the new B25 pictures |
| G8  | GPUI migration, Phase D: platform integrations to adapt or redo for GPUI (`docs/migration/PLATFORM_GAP_ANALYSIS.md`) and packaging | BLOCKED: D1 | The release workflow's packages, downloaded from a draft, hold 64/64 on macOS, Windows and Linux |
| G9  | GPUI migration, Phase E: parity report, the default switched, egui removed | BLOCKED: D1 | `docs/migration/EGUI_PARITY_REPORT.md` row by row; the maintainer's D5; egui, its three vendored patches and kittest removed in a change of their own |
| 121 | An expired signed stream address is renewed: from the open page (`decide::Handed`, `Start::renews`) and by the browser's next hand-over of the same file (`Entry::fresh_link`, row 57). Tested offline only | OPEN | A real video site's CDN address that runs out mid-download, tab left open in Microsoft Edge, goes on in the same row |
| 163 | GitGuardian fails pull request #33 on two made-up test passwords (`s3cret`, `p4ss`) in commit 62baeca's history; nothing like them is in the tree, and gitleaks finds no leak | OPEN: the maintainer | In GitGuardian's dashboard, resolve incidents 39386940 and 39386941 as test credentials |
| T5  | dm is listed in Windows' Settings › Default apps for magnet links and `.torrent` files (`register.rs` `list_in_default_apps`, tested with a stand-in `reg`) but has not been seen there | OPEN | A person on Windows with an installed build sees dm there and chooses it for a magnet link |
| T6  | The test that writes the magnet key, opens a magnet with `Start-Process` and sees a row appear in the running dm is written (`dm-desktop/tests/magnet_registry.rs`, `#[ignore]`, `DM_REGISTRY_TEST=1`; it saves the key as it was and puts it back) and passes on macOS, where it runs the key's command itself; its Windows half type-checks but has not run | PARTIAL | On Windows: `DM_REGISTRY_TEST=1 cargo test -p dm-desktop --test magnet_registry -- --ignored` passes |

## GPUI migration: picking it up (rows G1 to G9)

Written so that a new session can start from here alone. The maintainer asked on 2026-11-09
for dm's window to move from egui to GPUI, following a specification they attached (its
substance is in `docs/migration/GPUI_MIGRATION_PLAN.md`). The maintainer also said, twice:
**take the recommended option at every decision and keep going; do not stop to ask.** Record
each decision so taken in `docs/decisions.md` ("at the maintainer's word of 2026-11-09").

### Where and how

- **Branch `gpui-migration`**, not `main`: `git switch gpui-migration`. `main` receives it only
  by fast-forward after the full gate below, at the end of a phase.
- **Read first**, in this order: `docs/migration/GPUI_MIGRATION_PLAN.md` (phases, the decisions
  D1 to D5 and the recommended option for each), `docs/migration/GPUI_UI_ARCHITECTURE.md` (the
  design every step builds towards: crates, tick, state, views, `WindowProbe`),
  `docs/migration/GPUI_TEST_PLAN.md` (the gate and what each row adds to it),
  `docs/migration/CURRENT_UI_ARCHITECTURE.md`
  (sections 3.3 the real tick order, 4.2 the rules still in `DmApp`, 5.2 the control port,
  8 what a GPUI window must provide, 9 docs against code), then for the step at hand
  `EGUI_FEATURE_INVENTORY.md`, `PLATFORM_GAP_ANALYSIS.md`, `GPUI_DEPENDENCY_DECISION.md`, and
  `docs/architecture/MODULE_BOUNDARIES.md` (what may depend on what; `cargo xtask layers`
  enforces it from `desktop/xtask/src/main.rs`, `fn layers`, the `GUI` and `SOCKETS` lists).
- **Rules that still hold** (CLAUDE.md): no behaviour in `docs/behaviour-contract.md` changes
  (Phase A moves code, it changes nothing a person sees); name no other download manager
  ("reference A", "reference B"); no GPUI dependency in any crate before G6.
- **The gate, after every step that touches `desktop/`, `browser/` or `extension/`**, before
  its commit (all four, in this order; report every row they print):
  1. `cd desktop && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
  2. `sh scripts/windows-check.sh clippy` (Docker running; the Windows-only code G2 and G3 move
     is never built on the Mac otherwise; a plain `cargo check --target x86_64-pc-windows-msvc`
     stops at `ring`'s C, which needs the Windows headers the container carries)
  3. `cd desktop && cargo build -p dm-desktop && cd .. && sh scripts/package.sh && python3 scripts/human-qa.py`
     (macOS, a visible window, about 20 minutes): 64 of 64
  4. `sh scripts/linux-human-qa.sh` (Docker running; builds the checkout in Debian): 64 of 64
  Record each row's progress in `docs/migration/EGUI_PARITY_REPORT.md` once GPUI slices start.
  A row that fails once and passes on a second whole run (B40 did on 2026-11-09) is reported
  as such; a row that fails twice is a defect to fix before the commit.
- **One commit per step** (or per module moved, in G2 and G3), message naming the row.
  When a row closes, delete it here and say what was done in `CHANGELOG.md` (Unreleased,
  as "Inside, no change to what a person sees: ...") and in the plan's status line.

### State of each row (2026-11-09)

- **G1, docs and code agree: done** (2026-11-10). `refresh()` calls `changes::take()`, `logic()`
  reads `changes::pending()`; the architecture docs give the tick as the code runs it; stale
  comments fixed. Decided, not changed: `DmApp::logic` keeps running from both `Window::logic`
  and `DmApp::ui` (the window tests drive `DmApp::ui` alone).
- **G2, a toolkit-free crate: in progress.** Done (2026-11-10): the crate, and batch 1 below
  (`quiet`, `awake`, `wake`, `autostart`, `power`, `toast`, `control`), batch 2 (`region`, `mcp`,
  `cli`) and batch 3's `i18n` (with the catalogs, now `dm-desktop-base/i18n`), `format` and
  `details`, each re-exported at its old path (`i18n::pick` and `format::shortcut`/`key_name`
  draw with egui and stay in dm-desktop, which re-exports the rest with a glob);
  `tests/i18n.rs` scans both crates' sources. Gated per batch (`docs/decisions.md`, 2026-11-10);
  `human-qa.py` 64/64 after batch 1. Next: `platform.rs`'s `Desktop` (below). As planned: a new crate
  `desktop/crates/dm-desktop-base` that `dm-desktop` depends on and that `cargo xtask layers`
  forbids every name in `GUI` (add it to the `crates` list and to the `match` with
  `may = dm-core, dm-protocol, dm-shell`, `no_gui = true`, sockets allowed). Move one module per
  commit, leaving `pub use dm_desktop_base::<module>` at the old path in `dm-desktop/src/lib.rs`
  so no caller or test changes. Surveyed 2026-11-09 (`grep -cE "egui|eframe|winit"`, and
  the `crate::` modules each uses), in this order:
  1. no egui and no dm-desktop module used: `quiet` (210 lines), `awake` (90), `wake` (68),
     `autostart` (687), `power` (391), `toast` (210), `control` (577; its `ui.*` dispatch
     reaches the window through a callback today: keep that seam, G4 makes it a trait);
  2. no egui, using only modules already moved: `region` (201, uses `autostart`), `mcp`
     (609), then `cli` (416, uses `mcp`);
  3. egui in a few places, split first (the egui-typed helpers stay in dm-desktop):
     `format` (774, 6 mentions; uses `i18n`'s `i` and `region`), then `details` (356, uses
     `format`), `i18n` (602, 3 mentions; `pick` draws and stays; it uses `dialogs` and `theme`,
     which must be cut to a value passed in), and `platform.rs`'s `Desktop` (400, 5 mentions;
     `drag_out` needs the window's handle and stays as a method the window supplies).
  Each module brings its own crates (target-specific ones too, for example the `windows`
  and `objc2` families): move those lines from dm-desktop's `Cargo.toml` with it, and run
  `cargo xtask layers` after each move. Before moving a module: `grep -n "egui\|eframe\|winit" <file>`
  must be empty, or the egui part is split off first. Update `MODULE_BOUNDARIES.md` and
  `ARCHITECTURE.md`'s crate diagram in the last commit.
- **G3, rules into `dm_shell::app`: after G2.** The list, with files, is
  `CURRENT_UI_ARCHITECTURE.md` 4.2 (settings coordination and the script guard `by_script`,
  question detection B39 to B41, pending renames, once-only schedule dates, dial-up hang-up,
  saved searches, the weekly update check, one-time hints, the hand-off cancel counter, the
  inbox orders, adding, finished sizes, remove with Undo's bookkeeping, the speed limiter's
  memory). One rule per commit: the rule becomes a function or a small type in
  `dm_shell::app` (a module of its own where none fits, listed in `MODULE_BOUNDARIES.md`),
  `DmApp` calls it, and its tests move with it or are written (a test that fails without the
  move's code, as the project does). Wording, sounds and which dialog opens stay in the window.
- **G4, `ui.*` as a contract: after G2.** The methods are `ui.texts`, `ui.click`, `ui.type`,
  `ui.key`, `ui.scroll`, `ui.inspect`, `ui.select`, `ui.place`, `ui.windows`, `ui.window`,
  `ui.move_window`, `ui.screenshot`, `ui.accessibility`, plus `commands.list` and
  `command.run` (implemented in `dm-desktop/src/app/human.rs`, `app/control.rs`,
  `dialogs/apart.rs`; shapes in `CURRENT_UI_ARCHITECTURE.md` 5.2). Recommended: a trait
  `WindowProbe` in `dm-desktop-base` with one method per `ui.*`, request and answer as serde
  types whose JSON is fixed by tests written from what `scripts/human-qa.py` and `mcp.rs` read
  today; dm-desktop implements it; the control server dispatches to it. `human-qa.py` must
  not change.
- **G5, egui's own costs: after G4 (can run beside G3).** Recommended: `scripts/ui-bench.py`,
  using only the control port (so it measures a GPUI window the same way): a home seeded
  with 100, 1,000 and 10,000 rows (as `stress-check.py window` seeds them), then start to
  first `ui.texts` answer, idle CPU over 60 s (`ps`), memory (`memory-check.py`'s sampling),
  `ui.scroll` and Find (`ui.type` into the search box) latency, and the window's CPU with 20
  downloads running against a local server. Release build, three runs, medians. Record in a
  new `docs/migration/GPUI_TEST_PLAN.md` as the egui baseline.
- **G6, the GPUI shell: after G2 to G5.** Recommended pin (D2):
  `gpui = { package = "gpui-pre", version = "=0.3.12" }`,
  `gpui_platform = { package = "gpui-pre-platform", version = "=0.3.12" }` (macOS features
  `font-kit`, `runtime_shaders`; Linux `wayland`, `x11`; Windows none),
  `gpui-component = "=0.9.1"`; fallback `gpui-ce` 0.3.0 if the pair will not build. A new
  crate `desktop/crates/dm-gpui` (binary `dm-gpui`), depending on dm-core, dm-shell and
  dm-desktop-base only; add it to `cargo xtask layers` as a window crate. It opens the main
  window and one dialog window, runs the tick once per wake from one foreground waker
  (`changes::subscribe`), keeps the tokio runtime for the engine, and implements `WindowProbe`
  for what it draws. Judge it on `GPUI_MIGRATION_PLAN.md` Phase B's seven checks, each pass or
  fail per system, written to `GPUI_TEST_PLAN.md`. D1 (go): recommended go if check 1
  (screen readers reach OK and Cancel in the dialog's own window on macOS, Windows and Linux)
  passes, or can be made to pass by a patch to GPUI kept in `desktop/vendor/` as eframe's is;
  no-go otherwise, and then the plan stops at Phase A, which stands on its own.
- **G7 to G9: after D1.** The plan's Phases C, D and E. D3 (new B25 pictures): recommended
  approve once every other row holds in GPUI, drawing them with `--save-baseline` and looking
  at them. D4: for a row GPUI cannot hold as written, recommended the fallback the gap
  analysis names (for example `tray-icon`, `rfd`, dm's own drag-out code). D5: recommended
  switch the default only after 64/64 on macOS, Windows and Linux from release packages,
  and remove egui in a change of its own.

## Periodic

- Search GitHub for copies of dm that drop `NOTICE` (`dm-engine`, `com.dm.engine.json`,
  `easy-download-manager@pranavo72bex`) and file a DMCA notice for one. Last run 2026-11-09:
  no copy (the one `dm-engine` crate found, `dionkadi/rDM`, is a separate Tauri and SQLite
  program).
