#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# Stress run of the extension in Brave on CachyOS against real YouTube, for the
# old (use_dynamic_url) and the fixed manifest. ROUNDS, SEED choose the run.
#   ROUNDS=60 SEED=3 sh scripts/linux/cachyos-brave/stress.sh
here=$(cd "$(dirname "$0")" && pwd); root=$(cd "$here/../../.." && pwd)
for v in ${VARIANTS:-old fixed}; do
  docker run --rm --platform linux/amd64 -e VARIANT=$v -e LOCAL=${LOCAL:-} -e SW=${SW:-} -e ROUNDS=${ROUNDS:-40} -e SEED=${SEED:-1} \
    -v "$here:/scripts:ro" -v "$root/extension:/ext:ro" ${EXT_OLD:+-v "$EXT_OLD:/ext-old:ro"} dm-cachyos-brave python3 /scripts/stress.py
done
