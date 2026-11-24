# Writing a test

- Run one test: `cargo nextest run --workspace -E 'test(~NAME)'`; one crate:
  `cargo nextest run -p CRATE`; everything, as CI does: `cargo xtask test --profile ci`.
- Where a test goes: a module's tests in `tests.rs` beside it (`foo.rs` with `foo/tests.rs`), or
  `<what>_tests.rs` for a second set; a crate's integration tests in its `tests/`; the window's
  in `crates/dm-gpui/src/window_tests/`. An inline `#[cfg(test)] mod` over 100 lines fails
  `cargo xtask layers`.
- What tests share is in `crates/dm-test-support` (named only in `[dev-dependencies]`).
- A test names what it shows, in words (`a_range_of_no_stated_end_still_has_bytes_to_come`),
  and checks what the code is meant to do (its doc comment, `docs/`, the contract), not just
  what it happens to return.
- A test reaches only 127.0.0.1, writes only in its own temporary directory, and never sleeps
  on a guess: wait for the thing itself, with a named timeout.
- A bug's test fails without the fix: run it before the fix and see it fail.
- For anything the window shows, test the worst cases `.claude/skills/break/scenarios.md`
  lists that can reach it (empty, one word, an unbreakable string, right to left, emoji, no
  items and many, the narrowest and widest window, loading, error, disabled), through
  `crates/dm-gpui/src/window_tests/`: its layout sweep already runs four widths, two text
  sizes, German (long words) and Hebrew (right to left).
- Never weaken, skip, `#[ignore]` or delete a test to make a change pass.

## Lessons

- 2026-11-24: Headless tests in dm-desktop-base enforce strict lints like -D warnings, failing on unused variables, unnecessary mut keywords, and len()  0 comparisons.
