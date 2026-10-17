#!/bin/sh
# Firefox and dm in a Linux container, with real X11 input:
#
#   sh scripts/package.sh                 # builds dist/firefox
#   sh scripts/linux-firefox-check.sh
#   DM_DISTRO=ubuntu2204 sh scripts/linux-firefox-check.sh   # or fedora
#
# Runs `firefox-check.py native` with DM_REAL_INPUT=1 under Xvfb with a window
# manager and xdotool, so besides everything that check drives, a real
# right-click on a link and real keys pick "Download link with dm". Firefox
# is fetched into a Docker volume (dm-firefox-cache), not installed anywhere.
# Streamed in, not mounted, for the reason linux-check.sh gives; the built
# extension is ignored by git, so it is added to the stream.
set -e
cd "$(dirname "$0")/.."
# DM_DISTRO runs the same check in Ubuntu 22.04's or Fedora 41's own image,
# dm built here in Debian and copied in (scripts/linux/human-distro.sh), with
# the screen shot every few seconds into ./human-shots/firefox-<distro>.
# Fedora's Firefox is its own package; Ubuntu's is a snap, which cannot run
# in a container, so there it is Mozilla's tarball, fetched as below.
case "${DM_DISTRO:-debian}" in
  debian) ;;
  ubuntu2204|fedora) exec sh scripts/linux/human-distro.sh firefox "$DM_DISTRO" ;;
  *) echo "DM_DISTRO is debian, ubuntu2204 or fedora"; exit 2 ;;
esac
[ -f dist/firefox/pkg/dm_catcher_bg.wasm ] || { echo "no dist/firefox — run: sh scripts/package.sh"; exit 1; }
docker build -t dm-linux-check scripts/linux
# Screenshots (Firefox's downloads panel, gap 137) come back through a
# volume of this check's own into DM_SHOTS, or ./human-shots/firefox-panel.
SHOTS="${DM_SHOTS:-$PWD/human-shots/firefox-panel}"
docker volume rm dm-firefox-shots >/dev/null 2>&1 || true
status=0
{ git ls-files -z --cached --others --exclude-standard; find dist/firefox -type f -print0; } \
  | tar --null -T - -cf - \
  | docker run --rm -i -e DM_LIVE_PAGE="${DM_LIVE_PAGE:-}" \
      -v dm-linux-target:/target \
      -v dm-firefox-cache:/root/.cache/dm-firefox \
      -v dm-firefox-shots:/shots \
      -v "$HOME/.cargo/registry":/usr/local/cargo/registry \
      dm-linux-check sh -c '
        set -e
        mkdir -p /work && cd /work && tar -xf -
        apt-get update >/dev/null
        apt-get install -y --no-install-recommends openbox xdotool dbus-x11 xz-utils bzip2 \
          libasound2 libdbus-glib-1-2 libx11-xcb1 libpci3 libgtk-3-0 imagemagick >/dev/null
        # Real sites change faster than Debian packages yt-dlp: for a live
        # page, the current release, as a person installs it.
        if [ -n "$DM_LIVE_PAGE" ]; then
          curl -fsSL -o /usr/local/bin/yt-dlp https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp
          chmod +x /usr/local/bin/yt-dlp
          yt-dlp --version
        fi
        cd desktop && cargo build -q -p dm-desktop && cd ..
        export DISPLAY=:78 DM_BINARY=/target/debug/dm-desktop DM_REAL_INPUT=1 DM_SHOTS=/shots
        Xvfb :78 -screen 0 1280x800x24 >/dev/null 2>&1 &
        sleep 1
        openbox >/dev/null 2>&1 &
        dbus-launch --exit-with-session python3 scripts/firefox-check.py native
      ' || status=$?
mkdir -p "$SHOTS"
docker run --rm -v dm-firefox-shots:/shots dm-linux-check tar -C /shots -cf - . | tar -xf - -C "$SHOTS"
echo "screenshots: $SHOTS"
exit $status
