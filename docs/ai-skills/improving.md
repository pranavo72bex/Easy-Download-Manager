# Improving dm, unattended

For the `improve` task: one row of `docs/gaps.md`, taken by the bot with no one to ask. The
method is adapted from jakubkrehel/skills' `afk` skill (MIT; the skill itself is in
`.claude/skills/afk/`), in dm's own words and to its guardrails.

## Better for someone

Name the person the step helps and the moment it helps them: someone on a phone whose
download stalls, someone reading the progress window in German. A step that only tidies code
is not an improvement here.

## Match the repository

A change reads as if the maintainer wrote it. Before writing, find how dm already solves the
nearest problem (`.bot/context.md` names the files) and copy that:

- Reuse its modules, helpers and test harnesses; add no dependency, no new architecture.
- Use its named colours, sizes and steps; invent no value.
- Follow `AGENTS.md`'s layout (`foo.rs` beside `foo/`, tests in `tests.rs`), and its words'
  rules (`docs/ui-design.md`); every word the window shows goes in `assets/i18n`.

## The smallest complete step

Most rows a bot takes are steps already: an id ending in a letter (K3a) under a row marked
`· split`, each saying what to do, where, the test to write, when it is done and the skill
pages to read (`docs/gaps.md`, "How to read this file"). Do exactly
that step. When it is done, delete its row from `docs/gaps.md` (a closed row is deleted; git
keeps it): the steps marked `· after` it become free to take. If it was the last step of its
row, bring that row up to date too.

A row that is not split: do the next part of what it leaves, complete on its own, with its
tests, and bring the row up to date (what is now done, with the test's name; what is left).
Not the whole row if it is large. Either way, a `CHANGELOG.md` line in a user's words when a
user would notice.

## Try to reject it

Before writing the summary, review the change as someone who wants to send it back:

- Every place the changed code shows or runs, not only the one in mind: a shared widget is in
  every dialog that draws it; the phone's layout is the desktop's at a narrow width.
- The worst cases (`.claude/skills/break/scenarios.md`): empty, one word, an unbreakable
  string, right to left, many items and none, the narrowest and widest window, the largest
  text size, an error, a disabled control, dark mode.
- Remove the change and run its new test: a test that still passes shows nothing.

## Stop instead when

The step needs an existing row of `docs/behaviour-contract.md` changed, a person's decision or
wording, a real phone or other hardware, a new permission, network access dm did not have, or
a dependency. Change nothing; write why in `.bot/summary.md`.
