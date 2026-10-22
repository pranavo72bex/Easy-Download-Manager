#!/bin/sh
# A link dragged onto dm's window under Wayland, in the Linux container:
#
#   sh scripts/linux-wayland-drag-check.sh      # screenshots into ./human-shots/wayland-drag
#   DM_WAYLAND_DEBUG=1 sh scripts/linux-wayland-drag-check.sh   # with protocol logs
#
# Runs scripts/linux/wayland_drag_check.py under sway (headless backend):
# a GTK window offers a link, and a virtual pointer (scripts/linux/vpointer.c)
# drags it onto dm's window. Screenshots are copied out of a Docker volume.
set -e
cd "$(dirname "$0")/.."
docker volume rm dm-wayland-shots >/dev/null 2>&1 || true
docker build -q -t dm-linux-check scripts/linux >/dev/null
status=0
git ls-files -z --cached --others --exclude-standard \
  | tar --null -T - -cf - \
  | docker run --rm -i \
      -e DM_WAYLAND_DEBUG="${DM_WAYLAND_DEBUG:-}" \
      -v dm-linux-target:/target \
      -v dm-wayland-shots:/shots \
      -v "$HOME/.cargo/registry":/usr/local/cargo/registry \
      dm-linux-check sh -c '
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
        cd desktop && cargo build -q -p dm-desktop && cd ..
        # sway will not run as root: the session runs as an ordinary user.
        useradd -m tester && chmod 777 /shots && chmod -R a+rX /work
        su tester -w DM_WAYLAND_DEBUG -c "dbus-launch --exit-with-session sh /work/scripts/linux/wayland_session.sh"
      ' || status=$?
SHOTS="${DM_SHOTS:-$PWD/human-shots/wayland-drag}"
mkdir -p "$SHOTS"
docker rm -f dm-wayland-shots-copy >/dev/null 2>&1 || true
docker create --name dm-wayland-shots-copy -v dm-wayland-shots:/shots dm-linux-check true >/dev/null
docker cp dm-wayland-shots-copy:/shots/. "$SHOTS" >/dev/null 2>&1 || true
docker rm -f dm-wayland-shots-copy >/dev/null 2>&1 || true
echo "screenshots: $SHOTS"
exit $status
