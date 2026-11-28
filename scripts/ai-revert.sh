#!/bin/bash
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# The bots' safety net, for bug-report.yml: `bash scripts/ai-revert.sh SHA WHAT URL` ($GH_TOKEN,
# $GH_REPO, in a checkout of main with history) undoes the bot's merge SHA, because WHAT (a
# workflow's name, or what it found) went wrong on main after it, as the run URL shows: the
# revert is pushed to main, CI, Desktop and the contract run again there, and the merge's issue
# is reopened and queued to be tried again.
#
# Exits 0 when it reverted; 1 when SHA is not a merge ai-merge.yml made, is not on main, or the
# revert does not apply cleanly (then nothing is pushed, and the caller reports a failure as
# usual). The caller decides that SHA is to blame (the run before it held).

set -uo pipefail
sha=$1 what=$2 url=$3
[[ "$sha" =~ ^[0-9a-f]{40}$ ]] || { echo "not a commit: $sha" >&2; exit 1; }
message=$(git log -1 --format=%B "$sha" 2>/dev/null || true)
grep -q 'Merged by ai-merge.yml' <<<"$message" || { echo "$sha is not the bot's merge."; exit 1; }
git fetch -q origin main
git merge-base --is-ancestor "$sha" origin/main 2>/dev/null || { echo "$sha is not on main."; exit 1; }
git config user.name 'github-actions[bot]'
git config user.email '41898282+github-actions[bot]@users.noreply.github.com'
git checkout -q main && git pull -q --ff-only origin main
if ! git revert --no-edit "$sha"; then
    git revert --abort || true
    echo "The revert did not apply cleanly."
    exit 1
fi
git commit --amend -q -m "$(printf 'Revert the bot'"'"'s merge: %s on main after it [bot-revert]\n\n%s\n\nIts run: %s' "$what" "$(git log -1 --format=%s "$sha")" "$url")"
git push -q origin HEAD:main || { echo "The revert could not be pushed."; exit 1; }
gh workflow run ci.yml --ref main || true
gh workflow run desktop.yml --ref main -f bundles=false || true
gh workflow run contract-linux.yml --ref main || true
issue=$(grep -oE '^issue: #?[0-9]+' <<<"$message" | grep -oE '[0-9]+' | head -n1 || true)
task=$(grep -oE '^task: [a-z]+' <<<"$message" | cut -d' ' -f2 | head -n1 || true)
if [ -n "$issue" ]; then
    gh issue reopen "$issue" || true
    gh issue edit "$issue" --add-label "bot:reverted,bot:queue,bot-task:${task:-bug}" --remove-label bot:merged || true
    gh issue comment "$issue" --body "$(printf '<!-- edm-bot -->\nThe bot'"'"'s change was reverted: after it was merged, %s on main ([run](%s)). Queued to be tried again.\n\n— EDM'"'"'s bot' "$what" "$url")" || true
fi
gh workflow run ai-worker.yml || true
echo "Reverted $sha."
