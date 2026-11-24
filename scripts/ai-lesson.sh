#!/bin/bash
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# What one AI run teaches the next ones: a free model (scripts/ai-ask.py) reads what the
# checks printed (.bot/check.log), the failing tests (.bot/failing-tests.txt) and the
# model's summary, and gives one sentence a later run should know ("test X fails on
# Windows only when …", "crate Y's tests need Z"). It goes under docs/ai-notes.md's
# lessons, committed to main by the GitHub API ($GH_TOKEN, $GH_REPO), the oldest let go
# past 120. Run only on a CI failure's run: its input is the machine's own output, never
# a person's text, which must not reach every later prompt. `$1` says how the run ended.

set -uo pipefail
how=${1:-no change passed}
path=docs/ai-notes.md
{
    cat <<'EOF'
An unattended bot just worked on a failure of a Rust project's CI. From what its checks printed below, write
ONE sentence (at most 200 characters) that would help a later bot working on this codebase: a fact about the
code, its tests, its build, or a trap to avoid. Not about this one run's outcome; no advice to the maintainer.
If there is nothing worth remembering, answer with exactly: NOTHING
EOF
    echo "How it ended: $how"
    echo "--- failing tests"
    head -c 3000 .bot/failing-tests.txt 2>/dev/null
    echo "--- the bot's summary"
    head -c 2000 .bot/summary.md 2>/dev/null
    echo "--- the end of what the checks printed"
    tail -c 6000 .bot/check.log 2>/dev/null
} >.bot/lesson-prompt.txt
lesson=$(python3 scripts/ai-ask.py ask <.bot/lesson-prompt.txt 2>/dev/null | head -n 1 | tr -d '\r' | cut -c1-220)
case "$lesson" in
"" | *NOTHING*) echo "No lesson."; exit 0 ;;
esac
# A lesson is a fact, never a link or an instruction to fetch something.
lesson=$(sed -E 's#https?://[^ ]+##g; s/[`<>]//g' <<<"$lesson")

meta=$(gh api "repos/$GH_REPO/contents/$path" 2>/dev/null || echo '{}')
sha=$(jq -r '.sha // empty' <<<"$meta")
if [ -n "$sha" ]; then
    jq -r .content <<<"$meta" | base64 -d >.bot/notes.md
else
    printf '# What dm'"'"'s bots have learned\n\n## Lessons\n' >.bot/notes.md
fi
grep -qF "${lesson:0:60}" .bot/notes.md && { echo "Known already."; exit 0; }
echo "- $(date -u +%F): $lesson" >>.bot/notes.md
# The oldest lessons go past 120.
awk '/^- [0-9]{4}-/ { n++ } { line[NR] = $0 } END {
    drop = n - 120; for (i = 1; i <= NR; i++) { if (line[i] ~ /^- [0-9]{4}-/ && drop > 0) { drop--; continue } print line[i] } }' \
    .bot/notes.md >.bot/notes.trimmed && mv .bot/notes.trimmed .bot/notes.md
body=$(jq -n --arg c "$(base64 <.bot/notes.md | tr -d '\n')" --arg s "$sha" \
    '{message: "docs/ai-notes.md: a lesson from a run [bot]", content: $c} + (if $s == "" then {} else {sha: $s} end)')
gh api -X PUT "repos/$GH_REPO/contents/$path" --input - <<<"$body" >/dev/null && echo "Learned: $lesson"
