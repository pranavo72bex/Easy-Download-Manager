#!/bin/sh
# Real mouse and keyboard on dm's window and tray, in a Linux container:
#
#   sh scripts/linux-gui-check.sh
#   DM_ORCA=1 sh scripts/linux-gui-check.sh    # with Orca, a real screen reader
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
docker build -t dm-linux-check scripts/linux
git ls-files -z --cached --others --exclude-standard \
  | tar --null -T - -cf - \
  | exec docker run --rm -i \
      -e DM_ORCA="${DM_ORCA:-}" \
      -v dm-linux-target:/target \
      -v "$HOME/.cargo/registry":/usr/local/cargo/registry \
      dm-linux-check sh -c '
        set -e
        mkdir -p /work && cd /work && tar -xf -
        apt-get update >/dev/null
        apt-get install -y --no-install-recommends openbox xdotool x11-utils wmctrl dbus-x11 dex procps xclip \
          python3-gi gir1.2-gtk-3.0 \
          at-spi2-core python3-pyatspi libglib2.0-bin orca speech-dispatcher \
          haskell-status-notifier-item-utils >/dev/null
        cd desktop && cargo build -q -p dm-desktop && cd ..
        export DISPLAY=:77
        Xvfb :77 -screen 0 1280x800x24 >/dev/null 2>&1 &
        sleep 1
        openbox >/dev/null 2>&1 &
        # The tray service GNOME, KDE and waybar provide: the tray item of
        # dm registers with it, and gui.py drives it over D-Bus.
        if [ -z "$DM_ORCA" ]; then
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
            DM_ORCA_LOG=/tmp/orca.debug python3 scripts/linux/gui.py"
        fi
      '
