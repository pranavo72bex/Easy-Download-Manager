#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# Runs inside the container: builds and drives the client without a desktop
# tray host. StatusNotifier registration and menu actions are checked in a
# real desktop session, not simulated by Xvfb.
set -e
cd /work

echo "== what dm needs, as make doctor sees it"
make doctor || true

echo
echo "== dm's own JavaScript (dm-js, gaps row F15)"
(cd desktop && cargo test -p dm-js)

echo
echo "== the client, without a desktop tray host"
xvfb-run -a -s "-screen 0 1280x800x24" python3 scripts/app-check.py
