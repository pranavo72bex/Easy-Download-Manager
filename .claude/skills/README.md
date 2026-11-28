# Project skills

Skills Claude Code loads in this repository, and that EDM's free-model bots read
(`docs/ai-skills/`, `scripts/ai-context.py`). They are instructions for the AI, not part of
EDM: nothing here is built into or shipped with the app.

- `edm-design/`: EDM's own. Read first for any work on the window; it maps the skills below
  onto EDM's GPUI window and says which rules win.
- `better-interface/`, `better-ui/`, `better-typography/`, `better-colors/`, `better-layout/`,
  `better-accessibility/`, `better-writing/`, `better-motion/`, `interface-review/`: copied
  unchanged (less their `agents/` files) from github.com/jakubkrehel/skills at commit
  672ae44a (2026-11-18), MIT licence in `LICENSE-better-skills`. Written for web pages; read
  them through `edm-design`.
- `break/`, `afk/`, `upskill/`: from the same commit, the same way (2026-11-24). How they read
  for EDM:
  - `break`: its scenarios (`break/scenarios.md`) are the worst cases a window test covers.
    Its harness, a temporary web page looked at in a browser, is not EDM's: for EDM the harness
    is `crates/edm-gpui/src/window_tests/` and the layout sweep (four widths, two text sizes,
    German for long words, Hebrew for right to left).
  - `afk`: work done while the maintainer is away. Here the free-model bots in GitHub Actions
    do that unattended (`docs/testing.md`, "The bots"); they follow its method through
    `docs/ai-skills/improving.md`. In a session, its "push, open pull requests, merge only
    when asked" holds, and `CLAUDE.md`'s rules come first.
  - `upskill`: proposes skills, hooks and automations from past sessions, for a person to
    pick. A bot never runs it: what it would change (the workflows, the skills) is the bots'
    guardrails.
- `unslop/`: from github.com/heydamir/unslop at commit 620163ee (2026-11-27), by Damir
  Sokolovsky, CC BY 4.0 (`LICENSE-unslop`; keep the credit). Its markdown only: its
  `scripts/` (Node, for web pages), `agents/` and reference images were left out, so the
  image links in `references/refs.md` lead nowhere here. Written for web pages; read it
  through `edm-design`. For EDM its order of passes (spacing, color, icons, copy, type,
  layout, details, motion) and its list of tells (`references/signs.md`) are what apply;
  its hero visuals and landing-page advice do not fit a download manager's window.
- Not taken from that repository, though each was read and is clean: `explain-interface`
  (it fetches web pages and their stylesheets with `curl`), `build-design` (Figma),
  `state-machine` and `variant` (they render a component on a throwaway web page; EDM's window
  is not one).

## Checked, every time

`UPSTREAM.tsv` names every file copied in from outside, with its SHA-256 at the commit above.
`scripts/ai-skills-check.py` (a CI job, and run before any bot reads these) fails when one of
them changes, when a file appears inside a copied skill's folder that the list does not name,
or when any file here holds a hidden or direction-changing character, a line written to steer
a model, or anything executable. No bot may edit this folder.

To update them: copy the folders again from a newer commit (never the `agents/` files), read
the diff in full, write the new SHA-256s into `UPSTREAM.tsv`, and change the commit named
above.
