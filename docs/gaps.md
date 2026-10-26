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
| 8  | A download that dm asks about first ("Download details") has already been let go by the browser, so a dialog hidden behind another window looks like a lost download (`docs/noob-ux-report.md` N2); and an older settings file keeps `ask_caught_downloads` on | OPEN | The dialog comes to the front and the toast says it is waiting; or the setting is migrated like the catch list when it was never changed. Closes with a window test for each |
| 9  | The download list's last column is cut off at 1920 px (a horizontal scrollbar and a header reading "C…"); a failed row reads "The server has no file at thi…" with no next step; some words need a plain sentence (Site files, Allow the command line to control dm, Referrer, SHA-256, "A digest you were given") and the theme choice has icons only (N4–N6) | OPEN | The Name column gives up width first; the failure says "This link no longer works" and the menu offers Update link; each option gets one line under it; the picture of each, before and after, in `scripts/ui-shot.py` |
