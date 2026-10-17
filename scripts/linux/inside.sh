#!/bin/sh
# Runs inside the container: builds and drives the client without a desktop
# tray host. StatusNotifier registration and menu actions are checked in a
# real desktop session, not simulated by Xvfb.
set -e
cd /work

echo "== what dm needs, as make doctor sees it"
make doctor || true

echo
echo "== the client, without a desktop tray host"
xvfb-run -a -s "-screen 0 1280x800x24" python3 scripts/app-check.py
