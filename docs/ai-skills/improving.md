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

Do the next part of what the row leaves, complete on its own: code, tests, the row in
`docs/gaps.md` brought up to date (what is now done, with the test's name; what is left), and a
`CHANGELOG.md` line in a user's words when a user would notice. Not the whole row if it is
large.

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
