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
can be done here; G1 to G5 come first and need no decision. The other rows need a Windows
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
| 5   | 89 strings (the 71 reworded in 0.7.0, the 14 of 2026-10-30 for the quality list, the part fields and the Playlist dialog, and Take out's 4) were put into the other twelve languages by a machine pass and have not been read by a native speaker | BLOCKED: native readers | One reader per language corrects them (`git log -p -- desktop/crates/dm-desktop/i18n`) |
| 20  | Stress phases not run on Windows: `fulldisk` (needs a small volume) and `torrent` (needs a sandbox off the public network); the clipboard offer is covered on Linux only, as on macOS and Windows the only clipboard is a person's | PARTIAL | On Windows: `fulldisk` with a VHD as the download folder, `torrent` in a sandbox; the clipboard phase on a machine whose clipboard nobody is using |
| 39  | Secrets sealed under a key in the system's store: proved on Windows, macOS and Linux, except a copy of dm's home started under another Windows account, which must not open them and must say so once | PARTIAL | On a Windows computer with a second account |
| 47  | The second network path (Options › Advanced › Network) is tested with a second local address only | PARTIAL | One run with a phone's connection beside the computer's own, recording the speeds and each path's share |
| G1  | GPUI migration, A1: the architecture docs and the code disagree in twelve places (`docs/migration/CURRENT_UI_ARCHITECTURE.md` section 9): `changes::take` never called, `schedule_tick` before `ran_dry`, `DmApp::logic` run twice per painted frame, the window never subscribing to typed events, stale comments (`i18n.rs`, `control.rs`, `lib.rs`) | OPEN | Each row made true one way or the other: the docs corrected where the code is right, the code where the docs are; `logic` once per frame only if measured to change nothing a person sees; `cargo test --workspace`, `human-qa.py` 64/64 and `linux-human-qa.sh` 64/64 |
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

## Periodic

- Search GitHub for copies of dm that drop `NOTICE` (`dm-engine`, `com.dm.engine.json`,
  `easy-download-manager@pranavo72bex`) and file a DMCA notice for one. Last run 2026-11-09:
  no copy (the one `dm-engine` crate found, `dionkadi/rDM`, is a separate Tauri and SQLite
  program).
