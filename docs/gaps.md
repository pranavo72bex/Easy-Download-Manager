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
