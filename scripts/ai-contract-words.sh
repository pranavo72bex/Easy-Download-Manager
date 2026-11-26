#!/bin/bash
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# What human-qa.py said in one run of contract-linux.yml, for the bots' contract gate
# (scripts/ai-merge.sh, bug-report.yml): `bash scripts/ai-contract-words.sh RUN_ID OUT`
# ($GH_TOKEN, $GH_REPO) writes its human-qa.txt to OUT. From the small `contract-words`
# artifact, else (a run from before it existed) the whole `contract-linux` one. Exits 1,
# writing nothing, when the run left neither: a run that broke is no measure.

set -uo pipefail
run=$1 out=$2
[[ "$run" =~ ^[0-9]+$ ]] || { echo "not a run id: $run" >&2; exit 1; }
rm -f "$out"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for name in contract-words contract-linux; do
    if gh run download "$run" -R "$GH_REPO" -n "$name" -D "$tmp/$name" >/dev/null 2>&1 &&
        [ -f "$tmp/$name/human-qa.txt" ] && [ ! -L "$tmp/$name/human-qa.txt" ]; then
        cp "$tmp/$name/human-qa.txt" "$out"
        exit 0
    fi
done
echo "run $run left no human-qa.txt" >&2
exit 1
