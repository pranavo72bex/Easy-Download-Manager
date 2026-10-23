#!/bin/sh
# Closing dm's window under KDE's own compositor, in an Arch container:
#
#   sh scripts/linux-kde-close-check.sh
#
# KWin (Wayland, virtual output) with dm's binary built in the Debian image
# (scripts/linux-check.sh's target volume). Runs scripts/linux/kde_close_check.py.
# Not Plasma's panel: the tray's watcher and host are the check's own.
set -e
cd "$(dirname "$0")/.."
docker image inspect dm-kde >/dev/null 2>&1 || docker build -f scripts/linux/Dockerfile.kde -t dm-kde scripts/linux
docker volume create dm-wayland-shots >/dev/null
git ls-files -z --cached --others --exclude-standard \
  | tar --null -T - -cf - \
  | docker run --rm -i \
      -v dm-linux-target:/target:ro \
      -v dm-wayland-shots:/shots \
      dm-kde sh -c '
        set -e
        mkdir -p /work && cd /work && tar -xf -
        # kwin_wayland carries cap_sys_nice, which a container does not grant
        # and the kernel then refuses to run the file at all.
        setcap -r /usr/sbin/kwin_wayland || true
        useradd -m tester && chmod 777 /shots && chmod -R a+rX /work
        su tester -c "dbus-run-session -- sh /work/scripts/linux/kde_session.sh"
      '
