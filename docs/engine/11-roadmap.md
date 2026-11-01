# 11. Roadmap

Ordered work packages. Size: S is up to about two days, M up to about two
weeks, L more than two weeks, for one engineer who knows the code. "Rows" are
rows of `docs/gaps.md`. A package marked **new row** closes work that gaps.md
does not list yet; when you start it, add the row (the stop condition in
CLAUDE.md allows a row only for a defect found by running something, so first
reproduce the missing behaviour with a failing test).

The maintainer's order stays in force: features first, then QA, comparison
and engine measurement; rows 37, 11, 14, 62, 63 last, with the maintainer
guiding (memory note, 2026-11-01). The packages below are ordered by
dependency inside that frame.

## Packages

| WP | Title | What it delivers (requirement IDs) | Size | Depends on | Gaps rows |
|---|---|---|---|---|---|
| WP-01 | Test servers in one place | T1 to T8 of file 10 as reusable test tooling with JSON-lines logs; scenario files; existing `tests/common` folded in | M | none | 20, 24, 65 (setup) |
| WP-02 | Reference A measurement harness | E1 to E11 as scripts in the repository, results into `docs/reference-behavior.md` | M | WP-01 | 65 |
| WP-03 | Wire gaps | N12 cookie jar for `Set-Cookie`; N14 referer decision; N22 IPv6 test; N25 wake-lock check | S | WP-01 | new row |
| WP-04 | Transfer decisions from the measurements | S6 slow-connection replacement; S12 outage probing schedule; S10 one-connection fallback and message; S7 seam width; S1 minimum piece (only where the maintainer approves "exactly like reference A") | M | WP-02 | 65 |
| WP-05 | Limits | Q11 per-download speed limit; Q13 hourly limit table; Q18 sync timer | S | none | new row |
| WP-06 | Part cutting built and proven | C1 to C4 built and tested; C4 whole-file fallback; WebM exact trim | M | WP-01 | 45 |
| WP-07 | Streams breadth | H3 discontinuity, H5 packed audio and subtitle segments, H8 low-latency parts, D5 multi-period, D6 live dynamic manifests, D8 TTML | L | WP-01 | new row |
| WP-08 | Static page reader | X6 | M | WP-01 | new row |
| WP-09 | Site rules and answer profiles | X7, X8; the first real profiles from live pages; hook earlier start | M | WP-08 | 68, 69, 62, 63, 64 |
| WP-10 | Browser live checks | B-5, B-11 and gaps checks on real pages; Brave policy; Firefox listing | S | WP-09 | 30, 51, 64 |
| WP-11 | Secrets sealed on Windows built and tested | Q5 | S | none | 39 |
| WP-12 | Stream challenge without a helper | Choose among options A, D, E of file 06 (DECISION); implement; remove B (`tools.rs` fetch of an engine, fetched solver scripts) | L | maintainer's decision; WP-01 for the corpus | 37 (related), new row |
| WP-13 | Linux AAC proven | F2 on Linux built and compared with reference output | M | none | 53 |
| WP-14 | Linux H.264 decoder for GIF | F4 and the Linux half of F3 | L | WP-13 optional | 54 |
| WP-15 | Expired link on a real site | S21 against a signed address that expires | S | WP-09 | 57 |
| WP-16 | PAC scripts and Kerberos | N9 (needs WP-12's interpreter if option A or E), N10 Kerberos | M each | WP-12 | new row |
| WP-17 | Other archive listings | F14 (decision: list, or none) | M | maintainer's decision | new row |
| WP-18 | Interface rows owed by the engine | Rows 66, 67, 70 as far as the engine supplies data (connections per server shown, per-piece speed) | M | WP-04 | 66, 67, 70 |
| WP-19 | Documentation of public items | `missing_docs`, `#[must_use]` | M | none (any time) | 27 |
| WP-20 | Human checks on real computers | Windows and Linux runs: high contrast, held notifications, dialogs as windows, clipboard offer, sleep | S (people) | builds | 11, 14, 20, 60 |

Row 5 (translations read by native speakers) and row 47's remaining parts do
not belong to the engine and are not packages here.

## Order

```
WP-01 -> WP-02 -> WP-04 -> WP-18
   |        \-> (feeds decisions in docs/decisions.md)
   +-> WP-03
   +-> WP-06
   +-> WP-07
   +-> WP-08 -> WP-09 -> WP-10, WP-15
WP-05, WP-11, WP-13, WP-19 : any time
WP-12 : after the maintainer's choice -> WP-16
WP-14 : after WP-13 if it shares code; otherwise independent
WP-20 : last, with the maintainer
```

## Per-package routine

1. Write the failing tests from the requirement's Test column.
2. Build until they pass.
3. `cargo test --workspace`, `cargo clippy --all-targets -- -D warnings`,
   `cargo fmt --check`.
4. Because the work touches `desktop/`, `browser/` or `extension/`, run
   `python3 scripts/human-qa.py` whole, in a visible window. Report every row
   it prints.
5. Update the status mark in this folder, delete the closed gaps row,
   add a `docs/decisions.md` entry for any decision and `CHANGELOG.md` line.
6. One logical commit; the commit names any maintainer approval.

## Decisions waiting for the maintainer

| ID | Question | Where |
|---|---|---|
| DEC-1 | Which option for the stream challenge: A (own interpreter), D (browser computes), E (a linked engine library), or none; and whether B stays during a transition | file 06 |
| DEC-2 | Copy reference A's behaviour where measured, row by row (seam width, minimum piece, referer, outage wait)? Approval per row | file 10 |
| DEC-3 | Kerberos and PAC: wanted? | file 02 N9, N10 |
| DEC-4 | HTTP/2 for non-ranged requests | file 02 N23 |
| DEC-5 | Archive formats beyond ZIP | file 07 F14 |
| DEC-6 | Own-app video button (row 70) | file 08 B-13 |
| DEC-7 | Who owns upkeep of site readers and the challenge step, and how often they are checked | file 06 |
