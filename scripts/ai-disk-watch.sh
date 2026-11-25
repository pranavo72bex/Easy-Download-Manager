#!/bin/bash
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# The runner's disk, watched while the models work (.github/workflows/ai-fix.yml and
# ai-tests.yml). Run by the runner's own user, never by dmbot. On #60 the disk filled during
# the models' step: the runner died, and with it the step's log, the patch and check.log,
# so nobody could see why. This stops the models first instead.
#
#   bash scripts/ai-disk-watch.sh start LOG   watches in the background, echoes its pid
#   bash scripts/ai-disk-watch.sh report      what takes the disk, now
#
# Every AI_DISK_EVERY seconds (30) it reads the free space on /. When less than
# AI_DISK_FLOOR_GB (8) is left, it writes what takes the disk to LOG, then kills every dmbot
# process, so the step ends with room left for the steps after it to upload what there is.
# LOG is kept outside the checkout, where dmbot cannot write.

set -uo pipefail

AI_DISK_FLOOR_GB=${AI_DISK_FLOOR_GB:-8}
AI_DISK_EVERY=${AI_DISK_EVERY:-30}

free_gb() { df -BG --output=avail / | tail -n1 | tr -dc '0-9'; }

report() {
    df -h /
    echo "What takes the disk (GB):"
    sudo du -x -BG -d 1 /home/dmbot /tmp /home/runner/actions-runner 2>/dev/null | sort -rn | head -n 15
    if [ -n "${GITHUB_WORKSPACE:-}" ]; then
        sudo du -x -BG -d 2 "$GITHUB_WORKSPACE/target" "$GITHUB_WORKSPACE/.bot" 2>/dev/null | sort -rn | head -n 15
    fi
    echo "The largest files:"
    sudo find / -xdev -type f -size +1G -printf '%s\t%p\n' 2>/dev/null | sort -rn | head -n 10 |
        awk -F'\t' '{ printf "%6.1f GB  %s\n", $1 / 1073741824, $2 }'
}

case "${1:-}" in
start)
    log=${2:?the log file}
    : >"$log"
    (
        while sleep "$AI_DISK_EVERY"; do
            left=$(free_gb)
            if [ -n "$left" ] && [ "$left" -lt "$AI_DISK_FLOOR_GB" ]; then
                {
                    echo "The disk had ${left} GB left (the floor is ${AI_DISK_FLOOR_GB} GB): the models were stopped."
                    report
                } >>"$log" 2>&1
                sudo pkill -9 -u dmbot || true
                sleep 2
                sudo pkill -9 -u dmbot || true
                exit 0
            fi
        done
    ) </dev/null >/dev/null 2>&1 &
    echo $!
    ;;
report) report ;;
*)
    echo "usage: $0 start LOG | report" >&2
    exit 2
    ;;
esac
