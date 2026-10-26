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

## Is dm production ready?

As far as code and automated checks can say, yes, as of 2026-10-24; nothing
is open. What they cannot stand in for is a person at a real computer: Windows
has run only on CI's machines ([windows-checklist.md](windows-checklist.md)),
nothing is signed with a paid certificate, and no release has been cut. That
is not a gap in the program; it is what has not yet been watched.

## Open work

Found by a security review (2026-10-24); each row says how it closes.

| #  | Gap | Status | How it closes |
|----|-----|--------|---------------|
| 5  | The 71 reworded strings were translated into the other twelve languages by a machine pass (consistent with each catalog's own vocabulary, checked for placeholders and length) and have not been read by a native speaker | BLOCKED | One native reader per language goes through the 71 strings listed by `git log -p -- desktop/crates/dm-desktop/i18n` for the 0.7.0 commit and corrects them |
| 6  | A video site's watch page still needs yt-dlp: with the helper switched off (`DM_YTDLP` empty) the panel on a real YouTube watch page (Edge 156, 2026-10-25) offers no quality and says dm could not take it, so the pair the player fetches apart is not reached there; dm takes such a pair only where a page's own requests show it (`docs/decisions.md`, 2026-10-21) | PARTIAL | Keep the helper as the route for YouTube (done: it is the default). Closes for other sites with a recorded run on a page that serves the two files in plain; for YouTube only if a way past its token-gated requests is found that is not a way around its protection |
| 9  | The remaining first-use rough edges from the noob test (`docs/noob-ux-report.md` N4, N6): at a 960-point window the sixth column, Queue, is cut to a half-letter beside the horizontal scrollbar (by design, the first five fit, but it reads as a bug); some words need a plain sentence under them (Site files, Allow the command line to control dm, Referrer, SHA-256, "A digest you were given"); and the theme choice is four icons with no words | OPEN | The Queue column is shown only where a download has a queue of its own, or the header is not drawn half; each option gets one line under it and the theme icons get words (each needs the line translated into all thirteen catalogs, which `tests/i18n.rs` enforces); a before/after picture each from `scripts/ui-shot.py` |
| 10 | Dialogs' controls are 32 tall (Fluent 2); the window's own list, bars and menus keep 24 (`docs/fluent-audit.md` §1). At 32 across the window, `the_last_row_can_be_reached_at_large_text` fails with the clipboard bar shown at 300% text: the list sizes itself from the last frame, and the last row drifts down and out of view over six frames (measured), so the list's sizing needs its own fix first | PARTIAL | The list's height settled by what is drawn (not by last frame's overshoot) so that menus, toolbar and clipboard bar can be 32; then `interact_size.y` is set once in `theme/apply.rs`, the menu items keep a 32 pitch with no gap, and `press` in `tests/window/support.rs` tells a menu item from a toolbar button by more than height |
| 11 | No High Contrast handling: nothing reads the system's forced colours, so Aquatic and Desert are not covered (§3, §10); and no jump list entries (New download, Open downloads folder; §12) | BLOCKED | A fifth look built from the system's contrast colours with a window test per colour role; the jump list added in `platform.rs` for Windows. Both need a Windows machine to be shown working |
| 12 | Shadows are 28 (dialog) and 16 (menu, tooltip) where the standards give 128 and 32 | OPEN | The window sized without its shadow (egui counts it in the size), so a dialog can carry 128; a menu 32 and a tooltip 16 kept apart |
| 13 | The extension popup has "Take every download from this site" but no "Pause capturing for this site" (`UX_REPORT.md`, standards §2); a site can only be left alone by typing it into Options › Browser | OPEN | A popup line that adds the page's site to `skipHosts` through the host, with the browser check showing a click on a link from that site stays with the browser |
| 14 | Nothing holds notifications and windows back while a fullscreen app, a presentation or Focus Assist is on (§9); and the close button is not asked about once and remembered ("Keep running in the tray to finish downloads?") | OPEN | A quiet check in `platform.rs` per system (Windows `SHQueryUserNotificationState`, macOS Focus), the held ones shown after; the close question as a one-time dialog with a saved answer. The first needs a Windows machine to show working |
| 15 | A finished row shows no date, and "Last try" is the seventh column, so a file from last week is found by name, not by when (task 7) | OPEN | "Complete" followed by when it finished on the row (`format::when`), or Last try moved into the first five columns, with the window tests that read "Complete" kept passing |
| 16 | Options has no one-line description under any setting, no Restore defaults per section, and no separate Advanced page for connections, proxies and the user agent (§8) | OPEN | A line under each setting (each is a string in all thirteen catalogs, which `tests/i18n.rs` enforces), a Restore defaults button per tab, the expert tabs behind one Advanced page |
| 17 | First run is a banner, not three skippable steps (connect the browser, confirm the folder, done); no one-time hint for a 5 GB file ("Schedule it for the night?"); a retry does not say when it will try again (§4, §7) | OPEN | The three steps as one dialog on the first launch only, each skippable; the hint once, kept dismissed in the settings file; the retry row's status saying "next try in 20 s" |

