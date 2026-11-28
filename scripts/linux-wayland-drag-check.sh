#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# A link dragged onto EDM's window under Wayland, in the Linux container:
#
#   sh scripts/linux-wayland-drag-check.sh      # screenshots into ./human-shots/wayland-drag
#   EDM_WAYLAND_DEBUG=1 sh scripts/linux-wayland-drag-check.sh   # with protocol logs
#
# Runs scripts/linux/wayland_drag_check.py under sway (headless backend):
# a GTK window offers a link, and a virtual pointer (scripts/linux/vpointer.c)
# drags it onto EDM's window. Screenshots are copied out of a Docker volume.
set -e
cd "$(dirname "$0")/.."
# shellcheck source=scripts/linux/window.sh
. scripts/linux/window.sh
docker volume rm edm-wayland-shots >/dev/null 2>&1 || true
docker build -q -t edm-linux-check scripts/linux >/dev/null
status=0
git ls-files -z --cached --others --exclude-standard \
  | tar --null -T - -cf - \
  | docker run --rm -i \
      -e EDM_BINARY="/target/debug/$binary" \
      -e EDM_WAYLAND_DEBUG="${EDM_WAYLAND_DEBUG:-}" \
      -v edm-linux-target:/target \
      -v edm-wayland-shots:/shots \
      -v "$HOME/.cargo/registry":/usr/local/cargo/registry \
      edm-linux-check sh -c '
        set -e
        mkdir -p /work && cd /work && tar -xf -
        apt-get update >/dev/null
        apt-get install -y --no-install-recommends sway grim python3-gi gir1.2-gtk-3.0 dbus-x11 \
          libwayland-dev libwayland-bin >/dev/null
        # The virtual pointer the drag is made with.
        (cd scripts/linux \
          && wayland-scanner client-header wlr-virtual-pointer-unstable-v1.xml /tmp/wlr-virtual-pointer-unstable-v1-client-protocol.h \
          && wayland-scanner private-code wlr-virtual-pointer-unstable-v1.xml /tmp/vp-protocol.c \
          && cc -I/tmp -o /tmp/vpointer vpointer.c /tmp/vp-protocol.c -lwayland-client)
        cargo build -q -p "$(basename "$EDM_BINARY")"
        # sway will not run as root: the session runs as an ordinary user.
        useradd -m tester && chmod 777 /shots && chmod -R a+rX /work
        su tester -w EDM_WAYLAND_DEBUG,EDM_BINARY -c "dbus-launch --exit-with-session sh /work/scripts/linux/wayland_session.sh"
      ' || status=$?
SHOTS="${EDM_SHOTS:-$PWD/human-shots/wayland-drag}"
mkdir -p "$SHOTS"
docker rm -f edm-wayland-shots-copy >/dev/null 2>&1 || true
docker create --name edm-wayland-shots-copy -v edm-wayland-shots:/shots edm-linux-check true >/dev/null
docker cp edm-wayland-shots-copy:/shots/. "$SHOTS" >/dev/null 2>&1 || true
docker rm -f edm-wayland-shots-copy >/dev/null 2>&1 || true
echo "screenshots: $SHOTS"
exit $status
