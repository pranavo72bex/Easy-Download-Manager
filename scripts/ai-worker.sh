#!/bin/bash
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# The bots' queue (.github/workflows/ai-worker.yml; $GH_TOKEN, $GH_REPO; DRY=1 says what
# it would do and does nothing). One AI job at a time, so the free models' daily quota
# goes to one task, not two halves:
#
# 1. If ai-fix or ai-tests is running, nothing.
# 2. Housekeeping: a bot:working label left by a run that died goes back to the queue; a
#    feature waiting for thumbs-up that has them now is queued; an issue no model could do
#    is queued again after a week, three tries in all; each of the bot's pull requests that
#    waited is handed to ai-merge.yml.
# 3. The next task, the most urgent first: main's red runs (bot-task:ci), then revisions of
#    the bot's pull requests, people's bugs, features; the oldest first.
# 4. Nothing queued: the nightly tests (ai-tests.yml), once a day.
#
# And GitHub turns off a public repository's schedules after 60 days without a commit:
# past 45, the date is written to .github/keepalive.

set -uo pipefail
DRY=${DRY:-0}
VOTES=${BOT_FEATURE_VOTES:-3}
run() {
    echo "-> $*"
    [ "$DRY" = 1 ] || "$@" || true
}

# Kept alive.
last=$(gh api "repos/$GH_REPO/commits/main" --jq .commit.committer.date)
if [ $(($(date -u +%s) - $(date -u -d "$last" +%s 2>/dev/null || date -u -j -f '%Y-%m-%dT%H:%M:%SZ' "$last" +%s))) -gt $((45 * 86400)) ]; then
    meta=$(gh api "repos/$GH_REPO/contents/.github/keepalive" 2>/dev/null || echo '{}')
    body=$(jq -n --arg c "$(date -u +%F | base64)" --arg s "$(jq -r '.sha // empty' <<<"$meta")" \
        '{message: "Keep the schedules alive [bot]", content: $c} + (if $s == "" then {} else {sha: $s} end)')
    run gh api -X PUT "repos/$GH_REPO/contents/.github/keepalive" --input - <<<"$body"
fi

# One at a time.
for w in ai-fix.yml ai-tests.yml; do
    if [ -n "$(gh run list --workflow "$w" --limit 20 --json status --jq '.[] | select(.status != "completed") | .status')" ]; then
        echo "$w is running: the queue waits."
        exit 0
    fi
done

# Nothing is running, so a bot:working label is a run that died.
for n in $(gh issue list --state open --label bot:working --json number --jq '.[].number') \
    $(gh pr list --state open --label bot:working --json number --jq '.[].number'); do
    run gh issue edit "$n" --remove-label bot:working --add-label bot:queue
done

# The site watch's reports (sites.yml): a video site dm no longer reads, a bug to fix.
for n in $(gh issue list --state open --label site-watch --json number,labels \
    --jq '.[] | select([.labels[].name] | any(startswith("bot:")) | not) | .number'); do
    run gh issue edit "$n" --add-label bot:queue,bot-task:bug
done

# Features with their thumbs-up now.
for n in $(gh issue list --state open --label bot:needs-votes --json number --jq '.[].number'); do
    votes=$(gh api "repos/$GH_REPO/issues/$n/reactions" --paginate --jq '.[] | select(.content == "+1") | .user.login' | sort -u | wc -l)
    if [ "$votes" -ge "$VOTES" ]; then
        run gh issue edit "$n" --remove-label bot:needs-votes --add-label bot:queue,bot-task:feature
    fi
done

# Tried again after a week, with whatever models answer then: three tries in all.
week_ago=$(date -u -d '7 days ago' +%FT%TZ 2>/dev/null || date -u -v-7d +%FT%TZ)
gh issue list --state open --label bot:no-fix --json number,updatedAt,comments \
    --jq ".[] | select(.updatedAt < \"$week_ago\") | \"\(.number) \([.comments[] | select(.body | contains(\"No model made a change\"))] | length)\"" |
    while read -r n tries; do
        if [ "$tries" -lt 3 ]; then run gh issue edit "$n" --remove-label bot:no-fix --add-label bot:queue; fi
    done

# The bot's pull requests that waited for their checks or a model's review.
for n in $(gh pr list --state open --label bot:automerge --json number,labels \
    --jq '.[] | select([.labels[].name] | any(. == "bot:queue" or . == "bot:working" or . == "bot:needs-maintainer") | not) | .number'); do
    run gh workflow run ai-merge.yml -f pr="$n"
done

# The next task.
next() {
    # Revisions first among the pull requests; then issues by kind.
    local pr
    pr=$(gh pr list --state open --label bot:queue --json number,labels,createdAt \
        --jq '[.[] | select([.labels[].name] | index("bot-task:revise"))] | sort_by(.createdAt) | .[0].number // empty')
    # shellcheck disable=SC2016 # $t is jq's, not the shell's
    gh issue list --state open --label bot:queue --limit 100 --json number,labels,createdAt,body \
        --jq '.[] | (([.labels[].name] | map(select(startswith("bot-task:"))) | .[0]) // (if ((.body // "") | contains("run_id:")) then "bot-task:ci" else "bot-task:bug" end)) as $t
              | "\({"bot-task:ci": 0, "bot-task:bug": 2, "bot-task:feature": 3}[$t] // 4) \(.createdAt) \(.number) \($t | ltrimstr("bot-task:"))"' |
        { cat; [ -n "$pr" ] && echo "1 0000 $pr revise"; } | sort | head -n1
}
pick=$(next)
if [ -n "$pick" ]; then
    read -r _ _ n task <<<"$pick"
    echo "Next: #$n ($task)"
    run gh workflow run ai-fix.yml -f issue="$n" -f task="$task"
    exit 0
fi

# Nothing queued: the nightly tests, once a day.
since=$(date -u -d '20 hours ago' +%FT%TZ 2>/dev/null || date -u -v-20H +%FT%TZ)
if [ -z "$(gh run list --workflow ai-tests.yml --limit 5 --json createdAt,conclusion \
    --jq ".[] | select(.createdAt > \"$since\" and .conclusion != \"cancelled\") | .createdAt")" ]; then
    echo "Next: the nightly tests"
    run gh workflow run ai-tests.yml
else
    echo "Nothing to do."
fi
