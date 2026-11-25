# The desktop window

dm has one window, drawn with GPUI by `crates/dm-gpui` on macOS, Windows, Linux and (under 600
points wide) Android. A change to it is a change on every system.

## Read first, in this order (the first wins)

1. `docs/behaviour-contract.md`: what a person does and what then happens. A bot never changes
   or removes a row; a new feature adds a row at the end.
2. `docs/ui-design.md`: dm's own rules (one filled button per dialog, three uses of the
   accent, three ranks of text, a state is a mark and a word, labels in one column, the word
   rules).
3. `.claude/skills/dm-design/SKILL.md`: maps the `better-*` skills in `.claude/skills/` onto
   GPUI and says which rule wins where they disagree. Then the `better-*` skill the job needs
   (layout, typography, colours, accessibility, writing, motion, UI).
4. [ui-ux.md](ui-ux.md) for anything those leave open.

## Where things are

- `crates/dm-gpui/src/window/`: the main window (list, toolbar, side panel, status bar).
- `crates/dm-gpui/src/dialogs/`: each dialog, one module (a module with children is `foo.rs`
  beside `foo/`, never `foo/mod.rs`).
- `crates/dm-gpui/src/window_tests/` with `src/harness.rs`: the window driven by name through
  the control port, in GPUI's test platform. Every menu item, button and row menu is clicked
  here and checked for what it does.
- `assets/i18n/`: the words. English is `assets/i18n/template/catalog.json`; a string added
  to it goes, untranslated, into each language's `unread/<code>.json`; `cargo xtask i18n`
  counts them. Never hard-code a word the window shows.
- Colours, sizes and gaps are named tokens and steps; use the ones there, never a new raw
  number.

## How to test

- `cargo nextest run -p dm-gpui -E 'test(~WORDS)'` for the tests you touched;
  `cargo test -p dm-gpui window_tests` for the whole window.
- A layout change: the sweep tests (`window_tests/layout/`) run each dialog at four widths,
  two text sizes and in German and Hebrew (right to left). Words must fit inside their
  box in all of them.
- The window cannot be seen in CI; a test is the only proof. Write the test that would fail
  if the change were wrong, and cover the worst cases in `.claude/skills/break/scenarios.md`
  that can reach what you changed ([testing.md](testing.md)).

## Traps

- Some window tests fail only on Windows (fonts and text measuring differ). If a fix is
  reasoned on Linux, say in the summary that Windows CI is the check.
- A source file over 800 lines or a function over 150 fails `cargo xtask layers`: split by
  concern instead of growing one.
- No `unwrap`, `expect` or `panic` in shipped code.

## Lessons

- 2026-11-25: Changing native window sizes in dm-gpui::widgets::native requires updating the pinned expectations in widgets::native::tests::window_sizes_are_as_they_were.
