#!/bin/bash
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# What the AI fixer (.github/workflows/ai-fix.yml) knows before a model is asked, and
# whether one is asked at all. Reads issue $ISSUE of $GH_REPO (with gh, as $GH_TOKEN)
# and, when it names a failed run (`run_id: N`), that run, then writes into .bot/:
#
#   issue.json          the issue and its comments
#   failure.log         the end of the failed run's log
#   failing-tests.txt   job, test binary, test and its first words, a line each
#   breaking.diff       what changed from the last green run to the failed one
#   reproduce.log       each failing test run here, on Linux
#
# and to $GITHUB_OUTPUT `skip` (empty, `green`: the workflow has passed on main since,
# or `not-reproduced`: it failed on Linux but every failing test passes here) and
# `os_only` (`true` when it failed only on Windows or macOS, which this Linux runner
# cannot see fail: the pull request's own runs on each system are the check).

set -euo pipefail
mkdir -p .bot
cd "$(git rev-parse --show-toplevel)"
: >.bot/failure.log
: >.bot/failing-tests.txt
: >.bot/breaking.diff
: >.bot/reproduce.log
out() { echo "$1=$2" >>"$GITHUB_OUTPUT"; }

gh issue view "$ISSUE" --json title,body,comments,labels >.bot/issue.json
run_id=$(jq -r '[.body, (.comments[].body)] | join("\n")' .bot/issue.json |
    grep -oE 'run_id: [0-9]+' | tail -n1 | cut -d' ' -f2 || true)
if [ -z "$run_id" ]; then
    echo "No failed run named: an issue a person wrote."
    exit 0
fi

run=$(gh run view "$run_id" --json workflowDatabaseId,workflowName,headSha,createdAt,jobs)
wid=$(jq -r .workflowDatabaseId <<<"$run")
sha=$(jq -r .headSha <<<"$run")
created=$(jq -r .createdAt <<<"$run")
gh run view "$run_id" --log-failed 2>/dev/null | tail -n 600 >.bot/failure.log || true

# Passed on main since? Then there is nothing to fix.
newest=$(gh api "repos/$GH_REPO/actions/workflows/$wid/runs?branch=main&status=completed&per_page=30" \
    --jq "[.workflow_runs[] | select(.created_at > \"$created\" and .conclusion != \"cancelled\")]
          | sort_by(.created_at) | last | if . == null then \"\" else .conclusion + \" \" + .html_url end")
if [ "${newest%% *}" = success ]; then
    echo "Green since: ${newest#* }"
    out skip green
    out green_url "${newest#* }"
    exit 0
fi

# The tests that failed, from the run's annotations (scripts/ci-failed-tests.py writes
# them as "binary test: first words").
linux_failed=false
other_failed=false
while IFS=$'\t' read -r jid job; do
    case "$job" in *ubuntu* | *Linux*) linux_failed=true ;; *) other_failed=true ;; esac
    gh api "repos/$GH_REPO/check-runs/$jid/annotations" \
        --jq '.[] | select(.annotation_level == "failure") | .message | split("\n")[0]' 2>/dev/null |
        sed -nE "s/^([^ ]+) ([^ ]+): (.*)\$/$job\t\1\t\2\t\3/p" >>.bot/failing-tests.txt || true
done < <(jq -r '.jobs[] | select(.conclusion == "failure") | "\(.databaseId)\t\(.name)"' <<<"$run")
sort -u -o .bot/failing-tests.txt .bot/failing-tests.txt
if ! $linux_failed && $other_failed; then out os_only true; fi

# What changed from the last green run of the same workflow to the failed one.
good=$(gh api "repos/$GH_REPO/actions/workflows/$wid/runs?branch=main&status=success&per_page=30" \
    --jq "[.workflow_runs[] | select(.created_at < \"$created\")] | first | if . == null then \"\" else .head_sha end")
if [ -n "$good" ] && git cat-file -e "$good^{commit}" 2>/dev/null && git cat-file -e "$sha^{commit}" 2>/dev/null; then
    {
        echo "# Last green: $good; failed: $sha"
        git log --oneline "$good..$sha"
        echo
        git diff --stat "$good" "$sha"
        echo
        git diff "$good" "$sha" -- . ':(exclude)*.png' ':(exclude)*.gif' ':(exclude)*.ico'
    } | head -n 4000 >.bot/breaking.diff || true # head stops git early (SIGPIPE): not a failure
fi

# Each failing test, here on Linux; not when it failed only elsewhere, where a pass here
# says nothing.
reproduced=0
tried=0
if [ "$linux_failed" = false ]; then
    echo "Failed only on Windows or macOS: not run here."
    exit 0
fi
while IFS=$'\t' read -r job binary test _; do
    [ -n "$test" ] || continue
    tried=$((tried + 1))
    echo "## $binary $test (failed on $job)" >>.bot/reproduce.log
    if cargo nextest run --workspace --profile ci --no-fail-fast \
        -E "binary_id(=$binary) & test(=$test)" >>.bot/reproduce.log 2>&1; then
        echo "=> passes here, on Linux" >>.bot/reproduce.log
    else
        echo "=> fails here too" >>.bot/reproduce.log
        reproduced=$((reproduced + 1))
    fi
done < <(cut -f1-3 .bot/failing-tests.txt | sort -u)
echo "Failing tests tried here: $tried, failing here too: $reproduced"
if $linux_failed && [ "$tried" -gt 0 ] && [ "$reproduced" = 0 ]; then
    out skip not-reproduced
fi
