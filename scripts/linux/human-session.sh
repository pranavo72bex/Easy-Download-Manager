#!/bin/sh
# Inside a distribution's own image (see human-distro.sh): an X server, a
# window manager and a session bus, as a desktop has, and then the check
# with real input. The same session linux-chromium-human.sh and
# linux-firefox-check.sh set up in the Debian image.
set -e
browser=$1
rm -rf /shots/* && mkdir -p /opt/dm
cp /target/debug/dm-desktop /opt/dm/dm-desktop
export DM_BINARY=/opt/dm/dm-desktop DM_SHOTS=/shots
# The distribution's own Chromium is not always at /usr/bin/chromium
# (Fedora's is chromium-browser).
# Edge's image (DM_DISTRO=edge) has Microsoft Edge and no Chromium.
for c in /usr/bin/chromium /usr/bin/chromium-browser /usr/bin/microsoft-edge; do [ -x "$c" ] && export CHROMIUM="$c" && break; done
# What each check was written against: Chromium's screen is wider, for dm's
# window beside the browser's.
case "$browser" in
  chromium) export DISPLAY=:79; size=2400x1200 ;;
  *) export DISPLAY=:78; size=1280x800 ;;
esac
Xvfb $DISPLAY -screen 0 ${size}x24 >/dev/null 2>&1 &
sleep 1
# openbox takes Alt+click for moving windows; a person Alt-clicking a link
# in a browser means the browser, so that binding is left out.
sed "/<mousebind button=\"A-Left\"/,/<\/mousebind>/d" /etc/xdg/openbox/rc.xml > /tmp/openbox-rc.xml
openbox --config-file /tmp/openbox-rc.xml >/dev/null 2>&1 &
# A session bus for the length of the check. Not dbus-launch
# --exit-with-session, as the Debian runs use: where dbus is built without
# X11 (Arch), that ends the bus when stdin closes, which here is at once.
if [ "$browser" = chromium ]; then
  case "$CHROMIUM" in
    *microsoft-edge*) ;;
    *) exec dbus-run-session -- python3 scripts/chromium-human.py ;;
  esac
  # Edge: the real-input check, then chrome-check.py with CHROME_PATH
  # naming Edge (DM_EDGE_CHROME_CHECK=0 skips the second), on the same
  # display. Both are run; either failing fails the whole.
  status=0
  dbus-run-session -- python3 scripts/chromium-human.py || status=$?
  if [ "${DM_EDGE_CHROME_CHECK:-1}" != 0 ]; then
    echo; echo "== chrome-check.py, CHROME_PATH=$CHROMIUM"
    CHROME_PATH="$CHROMIUM" dbus-run-session -- python3 scripts/chrome-check.py \
      > /shots/chrome-check.txt 2>&1 || status=$?
    cat /shots/chrome-check.txt
  fi
  exit $status
fi
# firefox-check.py takes no screenshots of its own: the screen, every few
# seconds while it runs.
( n=0; while sleep 6; do n=$((n + 1))
    ffmpeg -loglevel quiet -y -f x11grab -video_size $size -i $DISPLAY -frames:v 1 \
      "/shots/$(printf %03d "$n")-screen.png" || true
  done ) &
DM_REAL_INPUT=1 dbus-run-session -- python3 scripts/firefox-check.py native
