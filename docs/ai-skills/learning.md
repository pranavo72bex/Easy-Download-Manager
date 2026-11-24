# How the bots learn

The idea is taken from rebelytics/one-skill-to-rule-them-all's task-observer skill (CC BY 4.0;
[README.md](README.md)): each piece of work leaves an observation, with its evidence and the
reason it matters, filed under the skill it concerns, so the next run starts knowing it.
There a person approves each change; here, unattended, the guardrails take that place.

## How a lesson is made

1. After a run on a CI failure, the nightly tests or an improvement, `scripts/ai-lesson.sh`
   gives a free model what the run's own checks printed, the failing tests and the run's
   summary, and asks for one sentence a later run should know, and which page it belongs to:
   `desktop`, `android`, `engine`, `ui-ux`, `testing` or `general` (`docs/ai-notes.md`).
2. The trusted publish job (never the model) adds it under that page's "Lessons", dated,
   unless a lesson starting the same way is there already. A page keeps its newest 40.
3. The maintainer teaches the same way: a comment of theirs on an issue or a bot's pull request
   saying "remember …" becomes a lesson.

## What never becomes a lesson

- Anything from a bug report, a feature request or a comment by anyone but the maintainer:
  one crafted issue must not reach every later prompt.
- A link, code to run, or an instruction to change the rules: links and backticks are
  stripped, and `scripts/ai-skills-check.py` fails a page with a line that tries to steer a
  model.

A wrong lesson is deleted by hand: the bots never delete one.
