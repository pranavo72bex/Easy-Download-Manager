# Skills for dm's bots

What the free-model bots in `.github/workflows/ai-*.yml` read before they work: one page per
part of dm, saying where to look, which rule wins, how to test and what has gone wrong
before. `scripts/ai-context.py` picks the pages a task needs and names them, with the files
that matter, in `.bot/context.md`, so a model that would not find them alone is handed them.

| Page | For a task about |
|---|---|
| [desktop.md](desktop.md) | the desktop window (GPUI, `crates/dm-gpui`), its dialogs, menus, words |
| [android.md](android.md) | the phone app (`crates/dm-android`, the window under 600 points) |
| [engine.md](engine.md) | downloading (`crates/dm-core`, `crates/dm-net`): ranges, resume, connections, files |
| [ui-ux.md](ui-ux.md) | how anything in the window looks and reads, on any system |
| [testing.md](testing.md) | writing a test anywhere in the workspace |
| [improving.md](improving.md) | the `improve` task: a row of `docs/gaps.md`, unattended |
| [learning.md](learning.md) | how the bots learn: the lessons at the end of each page |

The window's own design skills, written for dm, are in `.claude/skills/` (`dm-design` first,
then the `better-*` skills it maps, and `break`'s worst cases); a page here sends a model to
them for window work. Those copied from outside are pinned by SHA-256 in
`.claude/skills/UPSTREAM.tsv` and checked like `vendor/`.

## What keeps these pages clean

- Text only. No script, program or link to run is ever copied in; `vendor/` holds data and
  text taken verbatim, each file pinned by its SHA-256 in `vendor/MANIFEST.tsv`, with its
  licence beside it.
- `scripts/ai-skills-check.py` is run before any model reads these pages: a vendored file
  that no longer matches its hash, a hidden or direction-changing character, a line that
  tells a model to drop its rules or hand over a secret, a link or a script in
  `docs/ai-skills/` fails the check, and the bot works without the pages that day.
- No model edits a page (`ai_guard` refuses `docs/ai-skills/` and `.claude/`). A lesson is
  added under a page's "Lessons" by the trusted publish job only, from what a run's own
  checks printed or from the maintainer's words, never from what a stranger wrote.

## Where the outside parts come from

| Taken | From | Licence | How |
|---|---|---|---|
| `vendor/ui-ux-pro-max/quick-reference.md`, `app-interface.csv`, `ux-guidelines.csv` | nextlevelbuilder/ui-ux-pro-max-skill at `f2f7266b07805025421611656045e9b44b41e2eb` | MIT (`vendor/ui-ux-pro-max/LICENSE`) | Verbatim, hash-pinned. Its scripts, CLI, workflows and the rest of its 748 files are not taken. [ui-ux.md](ui-ux.md) says which of its rules hold for dm's native window and which (the web's) do not. |
| `.claude/skills/better-*`, `interface-review`, `break`, `afk`, `upskill` | jakubkrehel/skills at `672ae44ace6aaed65b86cbb1e2fd6fbea0e79578` | MIT (`.claude/skills/LICENSE-better-skills`) | Verbatim but for their `agents/` files, hash-pinned in `.claude/skills/UPSTREAM.tsv`. [improving.md](improving.md) adapts `afk`'s method; `explain-interface` (fetches pages with `curl`), `build-design` (Figma), `state-machine` and `variant` (web harness pages) are not taken. |
| The way [learning.md](learning.md) turns a run's evidence into a lesson | rebelytics/one-skill-to-rule-them-all at `6325c8e477608942da2f98b7f3a0d625c722ee22` (its task-observer skill) | CC BY 4.0: the idea, credited here, in dm's own words | Nothing copied. Its own scripts and its person-approves-each-change flow are not used: here the guardrails above take the person's place. |

Both were read in full before they were taken (2026-11-24): no hidden characters but the
Persian README's ordinary zero-width non-joiners, no line addressed to a model, and nothing
of their code is used.
