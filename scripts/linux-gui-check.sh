#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# Real mouse and keyboard on EDM's window and tray, in a Linux container:
#
#   sh scripts/linux-gui-check.sh
#   EDM_ORCA=1 sh scripts/linux-gui-check.sh    # with Orca, a real screen reader
#
# Builds the client in the Debian image linux-check.sh uses, then runs
# scripts/linux/gui.py under Xvfb with a window manager (openbox), a
# StatusNotifierWatcher (the tray service GNOME, KDE and waybar speak) and
# xdotool: closing into the tray, the tray menu's
# Show, Speed limiter and Quit, a real drag and drop of a file of links, and
# the window read over AT-SPI, as Orca reads it.
# Streamed in, not mounted, for the reason linux-check.sh gives.
set -e
cd "$(dirname "$0")/.."
# shellcheck source=scripts/linux/window.sh
. scripts/linux/window.sh
docker build -t edm-linux-check scripts/linux
git ls-files -z --cached --others --exclude-standard \
  | tar --null -T - -cf - \
  | exec docker run --rm -i \
      -e EDM_ORCA="${EDM_ORCA:-}" \
      -e EDM_BINARY="/target/debug/$binary" \
      -v edm-linux-target:/target \
      -v "$HOME/.cargo/registry":/usr/local/cargo/registry \
      edm-linux-check sh -c '
        set -e
        mkdir -p /work && cd /work && tar -xf -
        apt-get update >/dev/null
        apt-get install -y --no-install-recommends openbox xdotool x11-utils wmctrl dbus-x11 dex procps xclip \
          python3-gi gir1.2-gtk-3.0 \
          at-spi2-core python3-pyatspi libglib2.0-bin orca speech-dispatcher \
          haskell-status-notifier-item-utils >/dev/null
        cargo build -q -p "$(basename "$EDM_BINARY")"
        export DISPLAY=:77
        Xvfb :77 -screen 0 1280x800x24 >/dev/null 2>&1 &
        sleep 1
        openbox >/dev/null 2>&1 &
        # The tray service GNOME, KDE and waybar provide: the tray item of
        # EDM registers with it, and gui.py drives it over D-Bus.
        if [ -z "$EDM_ORCA" ]; then
          dbus-launch --exit-with-session sh -c "
            status-notifier-watcher >/dev/null 2>&1 &
            python3 scripts/linux/tray_host.py >/dev/null 2>&1 &
            sleep 1
            python3 scripts/linux/gui.py"
        else
          # Orca, a real screen reader, runs through every check and logs
          # what it would have said.
          dbus-launch --exit-with-session sh -c "
            python3 -c \"import sys; sys.argv=[0]; exec(open(\x27scripts/linux/gui.py\x27).read().replace(\x27\\nmain()\\n\x27, \x27\\n\x27)); screen_reader_on()\"
            status-notifier-watcher >/dev/null 2>&1 &
            python3 scripts/linux/tray_host.py >/dev/null 2>&1 &
            orca --replace --debug-file=/tmp/orca.debug >/dev/null 2>&1 &
            sleep 5
            EDM_ORCA_LOG=/tmp/orca.debug python3 scripts/linux/gui.py"
        fi
      '
