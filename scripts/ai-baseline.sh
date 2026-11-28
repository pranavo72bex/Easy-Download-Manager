#!/bin/bash
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# The tests main already fails, for the bots ($GH_TOKEN, $GH_REPO): read from the annotations
# of the Linux tests job of main's last finished Desktop run (scripts/ci-failed-tests.py names
# each failing test there), one `binary<TAB>test` a line, into /opt/edm-bot/baseline.txt,
# owned by root, outside the checkout, so the model and its code, run as dmbot, can read it
# and never change it. A bot's change is then refused only for a test that fails beside it
# and not on main (scripts/ai-agent.sh's ai_tests): main being red stops no other work.
# Prints the path; writes an empty file when main's tests pass or nothing can be read.

set -uo pipefail
out=/opt/edm-bot/baseline.txt
sudo mkdir -p /opt/edm-bot
tmp=$(mktemp)
run=$(gh run list --workflow desktop.yml --branch main --status completed --limit 10 \
    --json databaseId,conclusion --jq '[.[] | select(.conclusion == "success" or .conclusion == "failure")][0].databaseId // empty' 2>/dev/null)
if [ -n "$run" ]; then
    job=$(gh run view "$run" --json jobs --jq '.jobs[] | select(.name == "Tests (ubuntu-latest)") | .databaseId' 2>/dev/null)
    if [ -n "$job" ]; then
        gh api "repos/$GH_REPO/check-runs/$job/annotations" \
            --jq '.[] | select(.annotation_level == "failure") | .message | split("\n")[0]' 2>/dev/null >"$tmp.ann" || true
        python3 -I scripts/ai-test-failures.py annotations "$tmp.ann" >"$tmp" 2>/dev/null || true
    fi
fi
sudo install -o root -g root -m 0644 "$tmp" "$out"
sudo chmod 0755 /opt/edm-bot
echo "Main's own failing tests (Desktop run ${run:-none}, Linux): $(wc -l <"$out")" >&2
sed 's/^/  /' "$out" >&2
echo "$out"
