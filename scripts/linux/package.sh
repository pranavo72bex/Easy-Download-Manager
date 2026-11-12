#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# Runs inside the container: builds the Linux packages the release would,
# installs the .deb the way a person would, and drives what it installed.
#
# The release configuration, as CI uses it: cargo-packager reading the
# metadata in desktop/crates/dm-gpui/Cargo.toml (the GPUI window).
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
# The GPUI window (docs/gaps.md G9); the version is the workspace's own.
cd desktop/crates/dm-gpui
# AppImage tools are themselves AppImages, and a container has no FUSE.
APPIMAGE_EXTRACT_AND_RUN=1 CARGO_BUILD_RUSTFLAGS="$(sh "$(dirname "$0")/../release-flags.sh")" cargo packager --release -p dm-gpui --formats "$FORMATS" \
  --out-dir /target/release/bundle
cd /work

BUNDLE=/target/release/bundle
# Built only, for `make build-linux`: the packages the release has, the .rpm
# among them, copied out; nothing installed or driven.
if [ -n "${DM_BUILD_ONLY:-}" ]; then
  version=$(sed -n 's/^version = "\(.*\)"/\1/p' desktop/crates/dm-gpui/Cargo.toml | head -1)
  sh scripts/linux/nfpm.sh /target/release/dm-gpui "$version" "$BUNDLE"
  for f in "$BUNDLE"/*.deb "$BUNDLE"/*.AppImage "$BUNDLE"/*.rpm; do
    [ -e "$f" ] || continue
    name=$(basename "$f")
    # The GPUI window's; an earlier build's egui packages, left in the folder, are not.
    case "$name" in dm-gpui_*) name="dm_${name#dm-gpui_}" ;; dm-desktop_*) continue ;; esac
    cp "$f" "/out/$name"
  done
  ls -la /out
  exit 0
fi
DEB=''
APPIMAGE=''
# The GPUI window's, by name: an earlier build's dm-desktop_* may lie beside them.
for f in "$BUNDLE"/dm-gpui_*.deb; do [ -e "$f" ] && DEB=$f && break; done
for f in "$BUNDLE"/dm-gpui_*.AppImage; do [ -e "$f" ] && APPIMAGE=$f && break; done
[ -n "$DEB" ] || { echo "FAIL  no dm-gpui .deb in $BUNDLE"; exit 1; }
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
# The old name runs the window too.
test -x /usr/bin/dm-desktop || { echo "FAIL  no /usr/bin/dm-desktop"; exit 1; }
/usr/bin/dm-desktop --help >/dev/null || { echo "FAIL  /usr/bin/dm-desktop does not run dm"; exit 1; }
echo "dm-desktop runs dm-gpui"

echo
echo "== the installed binary, driven"
# The window itself, not the dm-desktop that runs it under its old name.
INSTALLED=$(dpkg -L "$(dpkg-deb -f "$DEB" Package)" | grep -E '/bin/dm-gpui$' | head -1)
[ -n "$INSTALLED" ] || { echo "FAIL  the .deb installs no dm-gpui"; exit 1; }
DM_BINARY="$INSTALLED" xvfb-run -a -s "-screen 0 1280x800x24" python3 scripts/app-check.py

echo
echo "== the installed binary keeps its secrets in the Secret Service (gaps row 39)"
# The keyring a desktop session runs, unlocked as a sign-in unlocks it (with a password made up
# for this run alone). Only the daemon is
# added here: secret-tool, which dm reaches it through, must come with the package.
apt-get install -y --no-install-recommends gnome-keyring >/dev/null
DM_BINARY="$INSTALLED" dbus-run-session -- sh -c '
  head -c 24 /dev/urandom | od -An -tx1 | tr -d " \n" | gnome-keyring-daemon --unlock --components=secrets >/dev/null
  xvfb-run -a -s "-screen 0 1280x800x24" python3 scripts/linux/secrets_check.py'

[ -z "$APPIMAGE" ] && exit 0
echo
echo "== the AppImage starts and answers as a native host"
chmod +x "$APPIMAGE"
printf '\053\000\000\000{"protocolVersion":1,"type":"engine.hello"}' \
  | APPIMAGE_EXTRACT_AND_RUN=1 "$APPIMAGE" --native-host | head -c 400 | tail -c +5
echo
