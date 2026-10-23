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

None. A defect found in use is added here as a row, with how to reproduce it
and how it will be shown fixed.

| #  | Gap | Status | How it closes |
|----|-----|--------|---------------|
