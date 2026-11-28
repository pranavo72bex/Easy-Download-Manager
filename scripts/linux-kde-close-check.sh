#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# Closing EDM's window under KDE's own compositor, in an Arch container:
#
#   sh scripts/linux-kde-close-check.sh
#
# KWin (Wayland, virtual output) with EDM's binary built in the Debian image
# (scripts/linux-check.sh's target volume). Runs scripts/linux/kde_close_check.py.
# Not Plasma's panel: the tray's watcher and host are the check's own.
set -e
cd "$(dirname "$0")/.."
# shellcheck source=scripts/linux/window.sh
. scripts/linux/window.sh
docker image inspect edm-kde >/dev/null 2>&1 || docker build -f scripts/linux/Dockerfile.kde -t edm-kde scripts/linux
docker volume create edm-wayland-shots >/dev/null
git ls-files -z --cached --others --exclude-standard \
  | tar --null -T - -cf - \
  | docker run --rm -i \
      -e EDM_BINARY="/target/debug/$binary" \
      -v edm-linux-target:/target:ro \
      -v edm-wayland-shots:/shots \
      edm-kde sh -c '
        set -e
        mkdir -p /work && cd /work && tar -xf -
        # kwin_wayland carries cap_sys_nice, which a container does not grant
        # and the kernel then refuses to run the file at all.
        setcap -r /usr/sbin/kwin_wayland || true
        useradd -m tester && chmod 777 /shots && chmod -R a+rX /work
        su tester -w EDM_BINARY -c "dbus-run-session -- sh /work/scripts/linux/kde_session.sh"
      '
