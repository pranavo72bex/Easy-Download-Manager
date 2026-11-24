#!/bin/bash
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# Sourced by .github/workflows/ai-fix.yml for a bug a person reported: `ai_fails_before`
# echoes `pass` only when the change adds a test that reproduces the bug, that is, a test
# that fails on the code as it was. The tests the change adds in test files (tests/,
# tests.rs, *_tests.rs, window_tests/) are run with every other file put back as it was;
# they must fail (or not build: they call what the fix adds) there, and they already
# passed with the fix (ai_check). Otherwise it echoes `no-reproducing-test` (none added in
# a test file) or `passes-without-the-fix`. The tree is left as the model left it.

AI_TEST_FILES='(^|/)tests?/|(^|/)tests\.rs$|_tests?\.rs$|(^|/)window_tests/'

ai_fails_before() {
    local changed tests names filter rc n
    git add -N . 2>/dev/null
    changed=$(git diff --name-only HEAD)
    tests=$(grep -E "$AI_TEST_FILES" <<<"$changed" | grep -E '\.rs$' || true)
    if [ -z "$tests" ]; then
        echo "No test added in a test file (tests/, tests.rs, *_tests.rs, window_tests/)." >>.bot/check.log
        echo no-reproducing-test
        return
    fi
    # The names of the tests added: a `fn` on a line after an added #[test].
    # shellcheck disable=SC2086 # one path a word
    names=$(git diff -U0 HEAD -- $tests | awk '
        /^\+[[:space:]]*#\[(tokio::)?test/ { pending = 1; next }
        pending && /^\+[[:space:]]*(pub )?(async )?fn [a-z_0-9]+/ {
            match($0, /fn [a-z_0-9]+/); print substr($0, RSTART + 3, RLENGTH - 3); pending = 0; next }
        /^\+[[:space:]]*#\[/ { next }
        { pending = 0 }' | sort -u)
    if [ -z "$names" ]; then
        echo "No #[test] added in the test files." >>.bot/check.log
        echo no-reproducing-test
        return
    fi
    filter=""
    while read -r n; do filter+="${filter:+ | }test(~$n)"; done <<<"$names"
    echo "Tests added: $(paste -sd' ' - <<<"$names"); run without the fix:" >>.bot/check.log

    # Every changed file but the tests, as it was.
    git diff --binary HEAD >.bot/full.patch
    while read -r f; do
        [ -n "$f" ] || continue
        grep -qE "$AI_TEST_FILES" <<<"$f" && continue
        if git cat-file -e "HEAD:$f" 2>/dev/null; then git checkout -q HEAD -- "$f"; else rm -f "$f"; fi
    done <<<"$changed"
    # With no secret in reach (ai_clean, scripts/ai-agent.sh), when sourced beside it.
    if declare -F ai_clean >/dev/null; then
        ai_clean cargo nextest run --workspace --profile ci --no-fail-fast -E "$filter" >>.bot/check.log 2>&1
    else
        cargo nextest run --workspace --profile ci --no-fail-fast -E "$filter" >>.bot/check.log 2>&1
    fi
    rc=$?
    # Back to the change as the model made it.
    git reset -q && git checkout -q -- . && git clean -qfd
    git apply --whitespace=nowarn .bot/full.patch
    git add -N . 2>/dev/null

    case "$rc" in
    0)
        echo "The added tests pass without the fix: they do not reproduce the bug." >>.bot/check.log
        echo passes-without-the-fix
        ;;
    4) # nextest: no test matched
        echo "The added tests were not found without the fix." >>.bot/check.log
        echo no-reproducing-test
        ;;
    *) echo pass ;;
    esac
}
