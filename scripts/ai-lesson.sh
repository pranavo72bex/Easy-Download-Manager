#!/bin/bash
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# What one AI run teaches the next ones (docs/ai-skills/learning.md): a free model
# (scripts/ai-ask.py) reads what the run's checks printed (.bot/check.log), the failing tests
# and the run's summary, and gives one sentence a later run should know and the skill page it
# belongs to (desktop, android, engine, ui-ux, testing; general goes to docs/ai-notes.md). It
# is added under that page's "Lessons", dated, the oldest let go past 40 (120 in the notes),
# and committed to main by the GitHub API ($GH_TOKEN, $GH_REPO) only when
# scripts/ai-skills-check.py still finds the pages clean. Run by a trusted publish job, only
# for work whose input is the machine's own or the maintainer's (a CI failure, the nightly
# tests, a row of docs/gaps.md), never a person's issue. `$1` says how the run ended.

set -uo pipefail
how=${1:-no change passed}
{
    cat <<'EOF'
An unattended bot just worked on a Rust project (dm, a download manager with a desktop window and an Android app).
From what its checks printed below, write ONE fact (at most 200 characters) that would help a later bot working on
this codebase: about the code, its tests, its build, or a trap to avoid. Not about this one run's outcome; no advice
to a person. Say which page it belongs to: "desktop" (the GPUI window), "android" (the phone app), "engine"
(downloading), "ui-ux" (how the window looks and reads), "testing" (writing tests), or "general".
Answer with JSON only: {"page": "...", "lesson": "..."}, or {"page": "none", "lesson": ""} if nothing is worth keeping.
EOF
    echo "How it ended: $how"
    echo "--- failing tests"
    head -c 3000 .bot/failing-tests.txt 2>/dev/null
    echo "--- the bot's summary"
    head -c 2000 .bot/summary.md 2>/dev/null
    echo "--- the end of what the checks printed"
    tail -c 6000 .bot/check.log 2>/dev/null
} >.bot/lesson-prompt.txt
answer=$(python3 scripts/ai-ask.py ask --json <.bot/lesson-prompt.txt 2>/dev/null) || { echo "No model answered."; exit 0; }
page=$(jq -r '.page // "none"' <<<"$answer" 2>/dev/null)
lesson=$(jq -r '.lesson // ""' <<<"$answer" 2>/dev/null | head -n 1 | tr -d '\r' | cut -c1-220)
# A lesson is a fact, never a link, code or markup.
lesson=$(sed -E 's#https?://[^ ]+##g; s/[`<>]//g' <<<"$lesson")
case "$page" in
desktop | android | engine | ui-ux | testing) path="docs/ai-skills/$page.md"; keep=40 ;;
general) path=docs/ai-notes.md; keep=120 ;;
*) echo "No lesson."; exit 0 ;;
esac
[ -n "${lesson// /}" ] || { echo "No lesson."; exit 0; }

meta=$(gh api "repos/$GH_REPO/contents/$path" 2>/dev/null || echo '{}')
sha=$(jq -r '.sha // empty' <<<"$meta")
[ -n "$sha" ] || { echo "$path is not on main."; exit 0; }
jq -r .content <<<"$meta" | base64 -d >.bot/page.md
grep -qF "${lesson:0:60}" .bot/page.md && { echo "Known already."; exit 0; }
grep -q '^## Lessons' .bot/page.md || printf '\n## Lessons\n\n' >>.bot/page.md
echo "- $(date -u +%F): $lesson" >>.bot/page.md
# The oldest go past $keep.
awk -v keep="$keep" '/^- [0-9]{4}-/ { n++ } { line[NR] = $0 } END {
    drop = n - keep; for (i = 1; i <= NR; i++) { if (line[i] ~ /^- [0-9]{4}-/ && drop > 0) { drop--; continue } print line[i] } }' \
    .bot/page.md >.bot/page.trimmed && mv .bot/page.trimmed .bot/page.md
# Kept only if the pages are still clean with it.
cp "$path" .bot/page.before
cp .bot/page.md "$path"
if ! python3 -I scripts/ai-skills-check.py >/dev/null; then
    cp .bot/page.before "$path"
    echo "The lesson would make the pages fail ai-skills-check: not kept."
    exit 0
fi
cp .bot/page.before "$path"
body=$(jq -n --arg c "$(base64 <.bot/page.md | tr -d '\n')" --arg s "$sha" --arg m "$path: a lesson from a run [bot]" \
    '{message: $m, content: $c, sha: $s}')
gh api -X PUT "repos/$GH_REPO/contents/$path" --input - <<<"$body" >/dev/null && echo "Learned ($page): $lesson"
