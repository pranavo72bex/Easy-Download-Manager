#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# Runs inside an Arch container: makepkg, pacman -U, and a hand-off.
set -e
# pacman 7 sandboxes downloads (Landlock, seccomp, a download user), which a
# container's kernel or an emulated one may not offer; this container is
# thrown away after the run.
grep -q '^DisableSandbox$' /etc/pacman.conf || sed -i '0,/^\[options\]/s//[options]\nDisableSandbox/' /etc/pacman.conf
sed -i 's/^DownloadUser/#DownloadUser/' /etc/pacman.conf
pacman -Syu --noconfirm >/dev/null
pacman -S --noconfirm --needed rust pkgconf git fontconfig freetype2 libxcb wayland vulkan-icd-loader \
  libxkbcommon-x11 libglvnd hicolor-icon-theme >/dev/null
id builder >/dev/null 2>&1 || useradd -m builder
mkdir -p /home/builder/.cargo && chown -R builder:builder /home/builder

version=$(sed -n 's/^pkgver=//p' packaging/PKGBUILD)
top="Easy-Download-Manager-desktop-v$version"
build=/home/builder/pkg
rm -rf "$build" && mkdir -p "$build/$top"
cp -r . "$build/$top/"
(cd "$build" && tar -czf "edm-$version.tar.gz" "$top" && rm -rf "$top")
# The release PKGBUILD, fetching this checkout instead of a GitHub tag.
# Its digest is the release tarball's, which this tarball made just now is
# not: skipped for it, and kept in the PKGBUILD a person builds.
sed -e "s|^source=.*|source=(\"edm-$version.tar.gz\")|" \
    -e "s|^sha256sums=.*|sha256sums=('SKIP')|" packaging/PKGBUILD > "$build/PKGBUILD"
cp packaging/EDM.desktop "$build/" 2>/dev/null || true
chown -R builder:builder "$build"

echo "== makepkg (build, check, package)"
# The whole log on a failure: the compiler's error is far above the last
# lines, and a pipe into tail would hide makepkg's exit status from set -e.
if ! su builder -c "cd $build && makepkg --noconfirm --nodeps" >"$build/makepkg.log" 2>&1; then
  grep -E -B2 -A12 '^error(\[|:)|panicked|^---- |FAILED|timed out' "$build/makepkg.log" \
    || tail -80 "$build/makepkg.log"
  echo "== makepkg failed"
  exit 1
fi
tail -5 "$build/makepkg.log"
pkg=
for candidate in "$build"/edm-*.pkg.tar.*; do
  case $candidate in *debug*) continue ;; esac
  [ -e "$candidate" ] && { pkg=$candidate; break; }
done
[ -n "$pkg" ] || { echo "== makepkg made no package"; exit 1; }
echo "== built: $pkg"
echo "== namcap-free install with pacman"
pacman -U --noconfirm "$pkg" >/dev/null
pacman -Ql edm | grep -E 'bin/|\.desktop$|icons'
echo "== the installed EDM answers a browser hand-off"
printf '\053\000\000\000{"protocolVersion":1,"type":"engine.hello"}' \
  | EDM_HOME=/tmp/edm-home /usr/bin/EDM --native-host | head -c 300 | tail -c +5
echo
echo "== arch package ok"
