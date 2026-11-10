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

## Bottom line (the maintainer, 2026-11-10)

What every change is measured against. These two never close, and nothing below may be closed
in a way that goes against them:

1. **Faster than reference A in every benchmark.** Every case of the side-by-side harness
   (`D:\compare\release-0.9.0\bench.mjs`: `fast`, `per_conn`, `flaky`, `outage`, `batch` and
   `inet`, and `idle`, where dm must use no more CPU and memory at rest), and any case added to
   it later. Both apps run on the same computer, in the same run, and the file must come out
   intact. A speed row closes only when dm is faster, not just close. Any case where dm is
   slower is a row in this list.
2. **Easier to use than reference A.** For the same task dm takes fewer steps, uses clearer
   words and has no dead ends. A gap found here is a row that proposes the change. A change to
   a behaviour in `docs/behaviour-contract.md` still needs the maintainer's approval in words
   before it is made (`CLAUDE.md`).

What reference A does comes from four folders of local notes. They are listed in
`docs/engine-local/sources.md`, which is git-ignored because the notes name it. Their findings
reach dm only as black-box items in `docs/engine-local/requirements.md`, and a run of both apps
must prove each one.

## State (2026-11-09)

No code is left that can be written and checked alone, so there is nothing left for an
autonomous session to do: each row below needs a Windows computer, a device or a person.
Checks on 2026-11-09 on the Mac: `cargo test --workspace` passes, line coverage 93.7% (CI's
floor 91%), the extension's 92 Edge checks, `video-sites-check.py` signed out (P0 7 of 7),
`human-qa.py` 64 of 64 rows. The 0.9.0 release's own files, downloaded from its draft (digests
as `latest.json` gives them), with extension 3.3.0's released zip: the macOS `.dmg` 64 of 64 rows;
the arm64 `.deb` installed by apt on Debian 12 with Chromium in Edge's place, 64 of 64 with
`DM_RELEASE=desktop-v0.9.0 sh scripts/linux-human-qa.sh` (B25 against the Linux baselines).

0.9.0 from its GitHub release, tested on Windows on 2026-11-09 (installed with its setup.exe, the
extension from dm-chrome-3.3.0.zip): `smoke.py`, `memory-check.py`, `launch-latency.py`,
`out-of-box-check.py` (30 of 30) and `human-qa.py` (64 of 64 rows) pass. Measured beside
reference A the same day, dm was slower in three cases (rows 166 to 168). On 2026-11-10 rows 167
and 168 closed (a reconnect no longer fetches its end again; past 8, connections only while they
add speed) and row 166 came down from about 2× to 1.5×.

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
| 121 | An expired signed stream address is renewed: from the open page (`decide::Handed`, `Start::renews`) and by the browser's next hand-over of the same file (`Entry::fresh_link`, row 57). Tested offline only | OPEN | A real video site's CDN address that runs out mid-download, tab left open in Microsoft Edge, goes on in the same row |
| 163 | GitGuardian fails pull request #33 on two made-up test passwords (`s3cret`, `p4ss`) in commit 62baeca's history; nothing like them is in the tree, and gitleaks finds no leak | OPEN: the maintainer | In GitGuardian's dashboard, resolve incidents 39386940 and 39386941 as test credentials |
| 166 | A download saved to another drive than dm's parts folder (dm's home, on the system drive, by default) is still slower than reference A, though no longer 2×. Since 2026-11-10 the row says Completed once the file is copied (Windows' own copy) and its mark written, with no flush waited for (`docs/decisions.md`): 512 MiB from 127.0.0.1, dm's home on the SSD (C:) and both apps saving to a hard disk (D:), dm took 5.4 and 6.4 s from its first request (10.0 and 12.7 s before), reference A 3.5 and 4.4 s. All of the difference is after the last byte: dm 2.5 to 3.6 s, reference A 0.9 to 1.2 s, and it is the copy itself. dm's copy of its one staged file took 1.4 to 3.2 s, by Windows' copy and by a 4 MiB buffered one alike (six traced runs); reference A appends its eight part files one after another, about 450 MB/s. A copy right after the file was written swings from 0.4 to 3.8 s with how much the system still has to write, whichever way it is made, and in a probe (an unsigned program, eight threads writing 16 KiB at a time) eight plain files were written in 0.42 s where one sparse file took 1.8 to 1.95 s, and copied in 1.1 s on average against 1.6. Saved on the same drive the finish is a rename: Completed 0.09 s after the last byte. Tried and dropped on 2026-11-10 (`docs/decisions.md`): copying the file to the folder's drive while it downloads, its pieces fetched in file order; side by side in one session it took 5.25 to 5.85 s, where the committed build took 2.31 to 5.77 s (once faster than reference A, 3.72 s) and reference A 3.65 to 3.94 s, so the copy saved less at the end than the pieces and the copying cost during the download. Found on the way: a large file that was sparse while it was written, as the staged one is, could not be opened afresh on this hard disk for 5 to 13 s after (512 MiB; written front to back and plain, 0.6 to 0.9 s; reference A's at once), which a person whose dm home and download folder are both on a hard disk may meet; on the SSD dm's finished file opened at once. Harness: `D:\compare\release-0.9.0\bench.mjs fast` with `BENCH_DM_HOME`, `BENCH_DM_DOWNLOADS`, `BENCH_REFA_DIR`; the probe and traces in the 2026-11-10 notes of `docs/decisions.md` | PARTIAL | dm faster than reference A on the same run (the bottom line above), the file intact and `integrity.rs` passing. What is left to try: the staged file written so it copies as fast as reference A's parts (no sparse file where the folder's drive is not the staging one, or one file a range there), which is a change to the staging the maintainer decides |
| T5  | dm is listed in Windows' Settings › Default apps for magnet links and `.torrent` files (`register.rs` `list_in_default_apps`, tested with a stand-in `reg`) but has not been seen there | OPEN | A person on Windows with an installed build sees dm there and chooses it for a magnet link |
| T6  | The test that writes the magnet key, opens a magnet with `Start-Process` and sees a row appear in the running dm is written (`dm-desktop/tests/magnet_registry.rs`, `#[ignore]`, `DM_REGISTRY_TEST=1`; it saves the key as it was and puts it back) and passes on macOS, where it runs the key's command itself; its Windows half type-checks but has not run | PARTIAL | On Windows: `DM_REGISTRY_TEST=1 cargo test -p dm-desktop --test magnet_registry -- --ignored` passes |

## Periodic

- Search GitHub for copies of dm that drop `NOTICE` (`dm-engine`, `com.dm.engine.json`,
  `easy-download-manager@pranavo72bex`) and file a DMCA notice for one. Last run 2026-11-09:
  no copy (the one `dm-engine` crate found, `dionkadi/rDM`, is a separate Tauri and SQLite
  program).
