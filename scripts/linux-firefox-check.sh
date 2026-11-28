#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# Firefox and EDM in a Linux container, with real X11 input:
#
#   sh scripts/package.sh                 # builds dist/firefox
#   sh scripts/linux-firefox-check.sh
#   EDM_DISTRO=ubuntu2204 sh scripts/linux-firefox-check.sh   # or fedora
#
# Runs `firefox-check.py native` with EDM_REAL_INPUT=1 under Xvfb with a window
# manager and xdotool, so besides everything that check drives, a real
# right-click on a link and real keys pick "Download link with EDM". Firefox
# is fetched into a Docker volume (edm-firefox-cache), not installed anywhere.
# Streamed in, not mounted, for the reason linux-check.sh gives; the built
# extension is ignored by git, so it is added to the stream.
set -e
cd "$(dirname "$0")/.."
# shellcheck source=scripts/linux/window.sh
. scripts/linux/window.sh
# EDM_DISTRO runs the same check in Ubuntu 22.04's or Fedora 41's own image,
# EDM built here in Debian and copied in (scripts/linux/human-distro.sh), with
# the screen shot every few seconds into ./human-shots/firefox-<distro>.
# Fedora's Firefox is its own package; Ubuntu's is a snap, which cannot run
# in a container, so there it is Mozilla's tarball, fetched as below.
case "${EDM_DISTRO:-debian}" in
  debian) ;;
  ubuntu2204|fedora) exec sh scripts/linux/human-distro.sh firefox "$EDM_DISTRO" ;;
  *) echo "EDM_DISTRO is debian, ubuntu2204 or fedora"; exit 2 ;;
esac
[ -f dist/firefox/pkg/edm_catcher_bg.wasm ] || { echo "no dist/firefox — run: sh scripts/package.sh"; exit 1; }
docker build -t edm-linux-check scripts/linux
# Screenshots (Firefox's downloads panel) come back through a
# volume of this check's own into EDM_SHOTS, or ./human-shots/firefox-panel.
SHOTS="${EDM_SHOTS:-$PWD/human-shots/firefox-panel}"
docker volume rm edm-firefox-shots >/dev/null 2>&1 || true
status=0
{ git ls-files -z --cached --others --exclude-standard; find dist/firefox -type f -print0; } \
  | tar --null -T - -cf - \
  | docker run --rm -i -e EDM_BINARY="/target/debug/$binary" -e EDM_LIVE_PAGE="${EDM_LIVE_PAGE:-}" \
      -v edm-linux-target:/target \
      -v edm-firefox-cache:/root/.cache/edm-firefox \
      -v edm-firefox-shots:/shots \
      -v "$HOME/.cargo/registry":/usr/local/cargo/registry \
      edm-linux-check sh -c '
        set -e
        mkdir -p /work && cd /work && tar -xf -
        apt-get update >/dev/null
        apt-get install -y --no-install-recommends openbox xdotool dbus-x11 xz-utils bzip2 \
          libasound2 libdbus-glib-1-2 libx11-xcb1 libpci3 libgtk-3-0 imagemagick >/dev/null
        cargo build -q -p "$(basename "$EDM_BINARY")"
        export DISPLAY=:78 EDM_REAL_INPUT=1 EDM_SHOTS=/shots
        Xvfb :78 -screen 0 1280x800x24 >/dev/null 2>&1 &
        sleep 1
        openbox >/dev/null 2>&1 &
        dbus-launch --exit-with-session python3 scripts/firefox-check.py native
      ' || status=$?
mkdir -p "$SHOTS"
docker run --rm -v edm-firefox-shots:/shots edm-linux-check tar -C /shots -cf - . | tar -xf - -C "$SHOTS"
echo "screenshots: $SHOTS"
exit $status
