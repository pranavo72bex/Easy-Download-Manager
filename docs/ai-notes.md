# What EDM's bots have learned

Read by every free-model bot in `.github/workflows/ai-*.yml` before it works, and written by
them: `scripts/ai-lesson.sh` adds one line after a CI failure's run (from what the checks
printed, never from a person's text), and the maintainer's comments that say "remember …"
are added by `scripts/ai-triage.py`. These are notes, and may be wrong: they never override
`CLAUDE.md`, `docs/behaviour-contract.md` or the checks. A wrong one is deleted by hand.

## Lessons

- 2026-11-24: The workspace is one Cargo workspace at the repository's root; run cargo from there, and `cargo xtask test --profile ci` for the whole test run (nextest, then the doctests).
- 2026-11-24: GitHub's Linux runner has too little free disk for the tests' build, coverage's instrumented build and an agent's own builds together; the workflows free ~30 GB first.
- 2026-11-24: Some edm-gpui window tests fail only on Windows (fonts, text measuring, timing); a fix reasoned on Linux is checked by the pull request's Windows run.
- 2026-11-24: `cargo llvm-cov` runs the tests too: a test that fails only under coverage's slower, instrumented build (the zoo's chunked case, an overflow) is a real bug, not a flaky test.
- 2026-11-24: edm-js (the JavaScript interpreter) refuses generator functions and `eval` on purpose; test that they are refused, never that they work.
- 2026-11-24: A range of no stated length ends at `u64::MAX`; measure ranges with saturating arithmetic (`end.saturating_sub(start).saturating_add(1)`).
- 2026-11-24: In edm-gpui, the test harness method step() in crates/edm-gpui/src/harness.rs is private.
- 2026-11-25: Inline test modules must stay under 100 lines; larger modules, such as those in edm-shell/src/check.rs, must be moved to a standalone 'mod tests;' file to pass linting.
- 2026-11-26 (pranavo72bex): C5b was completed in ae899990: added `limit_only_present` setting and `Settings::speed_limit_bytes_for` with presence-aware limiting; the gap row is removed from docs/gaps.md.
- 2026-11-26: cargo xtask layers enforces 800-line file and 150-line function limits; edm-gpui/src/options.rs (844 lines, render fn 165 lines), edm-gpui/src/window/table.rs (804 lines), and edm-core/src/youtube/player.rs (850 lines) alre
- 2026-11-26: The fuzz workspace (fuzz/Cargo.toml) must include the root's [patch.crates-io] section to use vendored librqbit-* crates; otherwise fuzz targets pull unpatched crates.io versions that overflow on large bencode integers.
- 2026-11-27: In edm-gpui, modules with children use foo.rs beside foo/ directory, never foo/mod.rs (see window/mod.rs and titlebar/mod.rs)
