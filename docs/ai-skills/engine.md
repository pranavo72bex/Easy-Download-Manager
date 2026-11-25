# The download engine

`crates/dm-core` downloads (HTTP ranges over several connections, FTP, HLS and DASH video,
torrents, archives), `crates/dm-net` speaks HTTP and TLS, `crates/dm-shell` is the
application's rules around them. Read `AGENTS.md` ("Rules for a change", "Glossary") and
`docs/how-it-works.md` for the part you touch.

## Rules that are never bent

- Never report a download `Completed` for bytes that were not all received and finalised.
- Never let a name from a server or a page escape the download folder (path traversal).
- Where a person's files live is asked of `dm_core::folders` alone.
- No `unwrap`, `expect` or `panic` in shipped code; a new reader of outside input gets a
  property test in its crate's `tests/fuzz.rs`.
- Log with `dm_core::note!(error | warn | info, "…", values)`, never `eprintln!`. A wait or a
  timeout is a named `const` with a doc comment.

## How to test

- Reproduce first: a failing test against the local server in `crates/dm-core/tests/common`
  (shared helpers in `crates/dm-test-support`), never a real site.
- `cargo nextest run -p dm-core -E 'test(~WORDS)'`; the server zoo:
  `cargo test -p dm-core --test zoo`.
- Tests write only in their own temporary directories and reach only 127.0.0.1.

## Traps

- A range of no stated length ends at `u64::MAX`: measure with
  `end.saturating_sub(start).saturating_add(1)`, never `end - start + 1`.
- A test that fails only under coverage's slower, instrumented build (`cargo llvm-cov`) is a
  real bug, usually a race or an overflow, not a flaky test.

## Lessons

- 2026-11-24: watch_estimating takes 5 arguments including downloaded_per_network: Option[ArcAtomicU64; 3], and Progress struct requires the per_network field.
- 2026-11-25: In dm-js, Interp stores program: VecStmt and temporarily takes ownership via std::mem::take during evaluation to satisfy borrow checking without leaking memory via Box::leak.
