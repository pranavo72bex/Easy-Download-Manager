#!/bin/bash
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# What human-qa.py said and pictured in one run of contract-linux.yml, for the bots' gates
# (scripts/ai-merge.sh, bug-report.yml): `bash scripts/ai-contract-words.sh RUN_ID OUT [PICTURES]`
# ($GH_TOKEN, $GH_REPO) writes its human-qa.txt to OUT, and with PICTURES the window's three
# pictures (row B25) into that folder. From the small `contract-words` and `contract-looks`
# artifacts, else (a run from before they existed) the whole `contract-linux` one. Exits 1,
# writing no words, when the run left none: a run that broke is no measure. Pictures it did
# not leave are simply not there (scripts/ai-looks-gate.py says so).

set -uo pipefail
run=$1 out=$2 pictures=${3:-}
[[ "$run" =~ ^[0-9]+$ ]] || { echo "not a run id: $run" >&2; exit 1; }
mkdir -p "$(dirname "$out")" && rm -f "$out" || exit 1
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fetch() { gh run download "$run" -R "$GH_REPO" -n "$1" -D "$tmp/$1" >/dev/null 2>&1; }
plain() { [ -f "$1" ] && [ ! -L "$1" ]; } # a regular file, never a link a run planted

if fetch contract-words && plain "$tmp/contract-words/human-qa.txt"; then
    cp "$tmp/contract-words/human-qa.txt" "$out" || exit 1
    from=$tmp/contract-looks
    [ -z "$pictures" ] || fetch contract-looks || true
elif fetch contract-linux && plain "$tmp/contract-linux/human-qa.txt"; then
    cp "$tmp/contract-linux/human-qa.txt" "$out" || exit 1
    from=$tmp/contract-linux/record/pictures
else
    echo "run $run left no human-qa.txt" >&2
    exit 1
fi
if [ -n "$pictures" ]; then
    mkdir -p "$pictures"
    for s in empty-window add-url download-details; do
        if plain "$from/$s.png"; then cp "$from/$s.png" "$pictures/$s.png"; fi
    done
fi
exit 0
