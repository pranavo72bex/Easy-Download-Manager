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

As far as code and automated checks can say, yes, as of 2026-10-24. Nothing is
open here. Every row this page held on 2026-10-23 was either built, with
tests and a run in the Linux container, or dropped with its reason in
[decisions.md](decisions.md) ("The work list is empty").

What the checks cannot stand in for is a person at a real computer: dm has
run on Windows only on CI's machines (`docs/windows-checklist.md` lists the
17 checks for a real PC), nothing is signed with a paid certificate, and no
release has been cut, which is the owner's call. None of that is a gap in the
program; it is what has not yet been watched.

## Open work

None. A defect found in use is added here as a row, with how to reproduce it
and how it will be shown fixed.

| #  | Gap | Status | How it closes |
|----|-----|--------|---------------|
