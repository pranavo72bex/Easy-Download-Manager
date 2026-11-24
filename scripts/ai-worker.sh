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
#    the bot's pull requests, people's bugs, features, improvements; the oldest first.
# 4. Nothing queued: the nightly tests (ai-tests.yml), once a day; then the next row of
#    docs/gaps.md (scripts/ai-improve-pick.py), opened as an issue labelled bot:improve.
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

# Every label the bots use, there before any is put on (an edit naming a missing label fails
# whole: the site watch's #56 was not queued so).
for l in bot-fix bot:queue bot:working bot:pr bot:merged bot:no-fix bot:green-since \
    bot:not-reproduced bot:needs-votes bot:needs-info bot:needs-maintainer bot:reverted \
    bot:closed bot:automerge bot:round-1 bot:round-2 bot:improve bot:already-done \
    bot-task:ci bot-task:bug bot-task:feature bot-task:improve bot-task:revise; do
    [ "$DRY" = 1 ] || gh label create "$l" --color ededed --description "dm's bot" 2>/dev/null || true
done

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
# Votes from different people, the author's not counted, nor an account under 90 days old.
ninety_days_ago=$(date -u -d '90 days ago' +%FT%TZ 2>/dev/null || date -u -v-90d +%FT%TZ)
for n in $(gh issue list --state open --label bot:needs-votes --json number --jq '.[].number'); do
    author=$(gh issue view "$n" --json author --jq .author.login)
    votes=0
    for login in $(gh api "repos/$GH_REPO/issues/$n/reactions" --paginate --jq '.[] | select(.content == "+1") | .user.login' | sort -u); do
        [ "$login" = "$author" ] && continue
        made=$(gh api "users/$login" --jq .created_at 2>/dev/null || echo 9999)
        [[ "$made" < "$ninety_days_ago" ]] && votes=$((votes + 1))
    done
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
              | "\({"bot-task:ci": 0, "bot-task:bug": 2, "bot-task:feature": 3, "bot-task:improve": 4}[$t] // 5) \(.createdAt) \(.number) \($t | ltrimstr("bot-task:"))"' |
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
    exit 0
fi

# Then the next row of docs/gaps.md (scripts/ai-improve-pick.py): three in flight at most, a new
# one every six hours at most, no row tried in the last fortnight or still open.
gh label create bot:improve --color 1d76db --description "dm's bot: a row of docs/gaps.md" 2>/dev/null || true
open_count=$(gh issue list --state open --label bot:improve --json number --jq length)
six_hours_ago=$(date -u -d '6 hours ago' +%FT%TZ 2>/dev/null || date -u -v-6H +%FT%TZ)
fortnight_ago=$(date -u -d '14 days ago' +%FT%TZ 2>/dev/null || date -u -v-14d +%FT%TZ)
recent=$(gh issue list --state all --label bot:improve --limit 100 --json createdAt \
    --jq "[.[] | select(.createdAt > \"$six_hours_ago\")] | length")
if [ "$open_count" -ge 3 ] || [ "$recent" -gt 0 ]; then
    echo "Improvements in flight ($open_count open, $recent in six hours): nothing to do."
    exit 0
fi
mapfile -t tried < <(gh issue list --state all --label bot:improve --limit 200 --json title,state,updatedAt \
    --jq ".[] | select(.state == \"OPEN\" or .updatedAt > \"$fortnight_ago\") | .title" |
    grep -oE 'gaps row [A-Z]+[0-9]+[a-z]?' | cut -d' ' -f3)
row=$(python3 -I scripts/ai-improve-pick.py "${tried[@]}")
if [ -z "$row" ]; then
    echo "No row of docs/gaps.md a bot can take: nothing to do."
    exit 0
fi
IFS=$'\t' read -r id problem state detail <<<"$row"
title="Improvement: gaps row $id — $(cut -c1-80 <<<"$problem")"
body=$(printf 'gaps_row: %s\n\nFrom docs/gaps.md, the maintainer'"'"'s ranked work list:\n\n- **Problem:** %s\n- **State:** %s\n- **Done and left:** %s\n\nTaken by the bot (ai-worker.yml): the next step it can make complete, with its tests.\n' \
    "$id" "$problem" "$state" "$detail")
echo "Next: gaps row $id"
if [ "$DRY" = 1 ]; then echo "-> open \"$title\""; exit 0; fi
url=$(gh issue create --title "$title" --label bot:improve,bot:queue,bot-task:improve --body "$body")
run gh workflow run ai-fix.yml -f issue="${url##*/}" -f task=improve
