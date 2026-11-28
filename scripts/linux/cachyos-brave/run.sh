#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# Builds the CachyOS + Brave image and runs the video hand-off three ways:
# with /tmp as a default system has it, with /tmp mounted noexec (hardened and
# tuned systems), and with noexec and TMPDIR pointed at a folder programs may
# run from. Needs docker and a network. Prints what EDM recorded and what its
# window says in the Status column for each.
#
#   sh scripts/linux/cachyos-brave/run.sh
#   EDM=0.8.4 sh scripts/linux/cachyos-brave/run.sh    # another release
#   EDM_APPIMAGE=path/edm_0.8.4_x86_64.AppImage sh scripts/linux/cachyos-brave/run.sh   # not yet released
#   EDM_BINARY=local/edm-desktop.new ...   # a Linux x86-64 edm-desktop put in the released AppImage's place
#   PAGE=https://... ...                 # a real page instead of the local one
set -eu
here=$(cd "$(dirname "$0")" && pwd)
EDM="${EDM:-0.8.3}"
mkdir -p "$here/local"
touch "$here/local/.keep"
rm -f "$here/local/EDM.AppImage"
if [ -n "${EDM_APPIMAGE:-}" ]; then cp "$EDM_APPIMAGE" "$here/local/EDM.AppImage"; fi
docker build --platform linux/amd64 --build-arg "EDM=$EDM" -t edm-cachyos-brave "$here"
for scenario in ${SCENARIOS:-"exec" "noexec" "noexec-tmpdir"}; do
  echo
  case "$scenario" in
    exec)          opts="rw,exec,nosuid"; extra="" ;;
    noexec)        opts="rw,noexec,nosuid"; extra="" ;;
    noexec-tmpdir) opts="rw,noexec,nosuid"; extra="-e TMPDIR=/root/exec-tmp" ;;
  esac
  echo "######## $scenario"
  # shellcheck disable=SC2086
  bin=""; [ -z "${EDM_BINARY:-}" ] || bin="-v $(cd "$(dirname "$EDM_BINARY")" && pwd)/$(basename "$EDM_BINARY"):/opt/EDM/usr/bin/edm-desktop:ro"
  # shellcheck disable=SC2086
  docker run --rm --platform linux/amd64 $bin -e PAGE="${PAGE:-}" --tmpfs "/tmp:$opts,size=1g" $extra \
    -v "$here:/scripts:ro" edm-cachyos-brave \
    sh -c 'mkdir -p /root/exec-tmp && python3 /scripts/repro.py' 2>&1 \
    | grep -E "== /tmp|hands the page|\] (running|completed|failed)|what EDM recorded|Status column|the file"
done
