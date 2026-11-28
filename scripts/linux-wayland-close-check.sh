#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# Closing EDM's window under Wayland with a tray present, in the Linux
# container, under sway (headless):
#
#   sh scripts/linux-wayland-close-check.sh
#
# Runs scripts/linux/wayland_close_check.py. Not a KDE Plasma session, so a
# minimize is not honoured the way KDE honours it; it shows what EDM does:
# it stays running, is not parked as hidden, and keeps drawing.
set -e
cd "$(dirname "$0")/.."
# shellcheck source=scripts/linux/window.sh
. scripts/linux/window.sh
docker build -q -t edm-linux-check scripts/linux >/dev/null
docker volume create edm-wayland-shots >/dev/null
git ls-files -z --cached --others --exclude-standard \
  | tar --null -T - -cf - \
  | docker run --rm -i \
      -e EDM_BINARY="/target/debug/$binary" \
      -v edm-linux-target:/target \
      -v edm-wayland-shots:/shots \
      -v "$HOME/.cargo/registry":/usr/local/cargo/registry \
      edm-linux-check sh -c '
        set -e
        mkdir -p /work && cd /work && tar -xf -
        apt-get update >/dev/null
        apt-get install -y --no-install-recommends sway python3-gi gir1.2-gtk-3.0 dbus-x11 dbus-daemon \
          libglib2.0-bin >/dev/null
        cargo build -q -p "$(basename "$EDM_BINARY")"
        useradd -m tester && chmod 777 /shots && chmod -R a+rX /work
        su tester -w EDM_BINARY -c "dbus-run-session -- sh /work/scripts/linux/wayland_close_session.sh"
      '
