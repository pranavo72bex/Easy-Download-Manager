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
| 10 | Dialogs' controls are 32 tall (Fluent 2); the window's own list, bars and menus keep 24 (`docs/fluent-audit.md` §1). At 32 across the window, `the_last_row_can_be_reached_at_large_text` fails with the clipboard bar shown at 300% text: the list sizes itself from the last frame, and the last row drifts down and out of view over six frames (measured), so the list's sizing needs its own fix first | PARTIAL | The list's height settled by what is drawn (not by last frame's overshoot) so that menus, toolbar and clipboard bar can be 32; then `interact_size.y` is set once in `theme/apply.rs`, the menu items keep a 32 pitch with no gap, and `press` in `tests/window/support.rs` tells a menu item from a toolbar button by more than height |
| 11 | No High Contrast handling: nothing reads the system's forced colours, so Aquatic and Desert are not covered (§3, §10); and no jump list entries (New download, Open downloads folder; §12) | BLOCKED | A fifth look built from the system's contrast colours with a window test per colour role; the jump list added in `platform.rs` for Windows. Both need a Windows machine to be shown working |
| 12 | Shadows are 28 (dialog) and 16 (menu, tooltip) where the standards give 128 and 32 | OPEN | The window sized without its shadow (egui counts it in the size), so a dialog can carry 128; a menu 32 and a tooltip 16 kept apart |
| 14 | Nothing holds notifications and windows back while a fullscreen app, a presentation or Focus Assist is on (`UX_REPORT.md`, standards §9). The close button's question is done (asked once, kept in Options › General) | BLOCKED | A quiet check per system, in `platform.rs`: Windows `SHQueryUserNotificationState`, macOS Focus; the held ones shown after. Needs a Windows machine: `cargo check --target x86_64-pc-windows-msvc` does not get past the C build of `aws-lc-sys` on this one, so the Windows code cannot even be compiled here |
| 16 | Options has a line under 13 of the settings whose names are the program's own, and a Restore defaults per tab; the rest have none, and there is no separate Advanced page for connections, proxies and the user agent (§8) | PARTIAL | A line under each remaining setting (each is a string in all thirteen catalogs, which `tests/i18n.rs` enforces); the expert tabs (Network, Proxy) behind one Advanced page |
| 17 | A retry does not say when it will try again ("next try in 20 s"): the row says "Connection dropped (retry 2 of 5)" and nothing of the wait (`UX_REPORT.md`, §4) | OPEN | `dm-core` gives the status the time the next attempt starts; the row says it; a window test with a stalled server that refuses once |

