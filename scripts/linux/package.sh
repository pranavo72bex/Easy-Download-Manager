#!/bin/sh
# Runs inside the container: builds the Linux packages the release would,
# installs the .deb the way a person would, and drives what it installed.
#
# The release configuration, as CI uses it: cargo-packager reading the
# metadata in desktop/crates/dm-desktop/Cargo.toml.
set -e
cd /work

TOOLS=/target/tools
if [ ! -x "$TOOLS/bin/cargo-packager" ]; then
  echo "== cargo-packager (once; kept in the target volume)"
  cargo install cargo-packager --version 0.11.8 --locked --root "$TOOLS"
fi
export PATH="$TOOLS/bin:$PATH"

# AppImage tooling. The Linux tray uses the desktop session's StatusNotifier
# host and does not bundle a system tray library.
echo "== packaging tools"
apt-get update >/dev/null
apt-get install -y --no-install-recommends \
  patchelf desktop-file-utils >/dev/null

# DM_FORMATS=deb leaves the AppImage out: its tools cannot run under QEMU
# (they read their own /proc/self/exe), which is how an x86_64 build on an
# arm64 machine runs.
FORMATS="${DM_FORMATS:-deb,appimage}"
echo "== building: $FORMATS"
cd desktop/crates/dm-desktop
# AppImage tools are themselves AppImages, and a container has no FUSE.
APPIMAGE_EXTRACT_AND_RUN=1 cargo packager --release --formats "$FORMATS" \
  --out-dir /target/release/bundle
cd /work

BUNDLE=/target/release/bundle
DEB=''
APPIMAGE=''
for f in "$BUNDLE"/*.deb; do [ -e "$f" ] && DEB=$f && break; done
for f in "$BUNDLE"/*.AppImage; do [ -e "$f" ] && APPIMAGE=$f && break; done
echo
ls -la "$DEB" ${APPIMAGE:+"$APPIMAGE"}

echo
echo "== what the deb says it needs"
dpkg-deb -f "$DEB" Package Version Architecture Depends

echo
echo "== installing it, dependencies resolved by apt"
apt-get update >/dev/null
apt-get install -y --no-install-recommends "$DEB" >/dev/null
dpkg -L "$(dpkg-deb -f "$DEB" Package)" | grep -E 'bin/|\.desktop$'

echo
echo "== the installed binary, driven"
INSTALLED=$(dpkg -L "$(dpkg-deb -f "$DEB" Package)" | grep -E '/bin/' | head -1)
DM_BINARY="$INSTALLED" xvfb-run -a -s "-screen 0 1280x800x24" python3 scripts/app-check.py

[ -z "$APPIMAGE" ] && exit 0
echo
echo "== the AppImage starts and answers as a native host"
chmod +x "$APPIMAGE"
printf '\053\000\000\000{"protocolVersion":1,"type":"engine.hello"}' \
  | APPIMAGE_EXTRACT_AND_RUN=1 "$APPIMAGE" --native-host | head -c 400 | tail -c +5
echo
