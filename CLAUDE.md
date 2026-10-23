# dm - Development rules

`desktop/` is the product and does the downloading; the browser extension
(`browser/`, Rust compiled to WebAssembly) catches downloads and hands them
to it. What is done, with the evidence for each line, is in
[docs/feature-status.md](docs/feature-status.md); the behaviour checklist is
[docs/parity.md](docs/parity.md).

# Autonomous Development Rules

You are the primary developer of this project.

Your objective is to make this software a complete, production-quality download manager.

## NEVER WAIT FOR ME

Do not repeatedly ask me:

* "What should I implement next?"
* "Should I continue?"
* "Would you like me to fix this?"
* "What feature should I work on?"

Instead, determine the next task yourself from docs/gaps.md.

## DEVELOPMENT LOOP

Repeat this cycle continuously:

1. Read docs/gaps.md (the ranked work list).
2. Read docs/decisions.md.
3. Inspect the existing implementation.
4. Select the highest-priority unfinished task.
5. Implement it completely.
6. Run tests.
7. Build the application.
8. Fix all errors.
9. Test again.
10. Update docs/gaps.md.
11. Update docs/decisions.md if a decision was made.
12. Continue immediately with the next unfinished task.

Use this loop until the project is genuinely complete.

**Stop condition.** If docs/gaps.md has no open rows, do not invent work and do
not keep cycling: run `cargo test --workspace` once, confirm it passes and
`git status` is clean, report that, and stop. Only a defect found by running
something (a failing test or check, a reproduced bug) justifies a new row.
Keep changes to existing behaviour out of unrelated tasks, and run
`cargo test --workspace` before each commit (the privacy check reads comments).

## IMPORTANT

Never mark a task complete merely because code was written.

A task is complete only after:

* Implementation exists.
* It integrates with the existing architecture.
* It compiles/builds.
* Relevant tests pass.
* Error handling exists.
* Existing functionality still works.

## TOKEN EFFICIENCY

Do not waste tokens explaining obvious work.

Prefer:

IMPLEMENT → TEST → FIX → CONTINUE

rather than:

EXPLAIN → ASK → WAIT

Keep responses extremely concise.

Spend the available context primarily on:

* Reading code
* Writing code
* Running tests
* Debugging
* Improving architecture

## WHEN SOMETHING FAILS

Do not stop and ask me.

Investigate the failure yourself.

Try:

1. Read the error.
2. Inspect relevant source.
3. Identify the root cause.
4. Fix it.
5. Run the test again.
6. Try another approach if necessary.

Only stop if the problem genuinely requires information or credentials that you cannot obtain.

## WHERE THE STATE IS KEPT

No separate progress log. Keep these current instead:

* Architecture: docs/how-it-works.md and AGENTS.md
* Completed work: docs/feature-status.md, CHANGELOG.md and docs/parity.md (a
  closed gap's row is deleted from docs/gaps.md; git keeps its history)
* Current task, known bugs and next tasks: docs/gaps.md
* Important technical decisions, and approaches tried and dropped: docs/decisions.md

Before ending a session, make sure docs/gaps.md accurately describes the current state.

## GIT

Use git when available.

Make logical commits after stable milestones.

Do not create meaningless commits after every tiny change.

## DEFINITION OF DONE

Do not declare the project finished until:

* Core downloader works reliably.
* Downloads can resume.
* Multiple connections work correctly.
* Network failures recover correctly.
* Browser integration works.
* UI works end-to-end.
* Download queue works.
* Settings work.
* Tests cover critical functionality.
* Production builds succeed.
* Major known bugs are fixed.

If docs/gaps.md still contains important unfinished tasks, continue working.

## START

Immediately inspect the repository.

Create docs/gaps.md if it does not exist.

Then begin implementing the highest-priority task.

After completing it, automatically continue to the next task.

Do not wait for another user message.