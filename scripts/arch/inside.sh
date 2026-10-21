#!/bin/sh
# Runs inside an Arch container: makepkg, pacman -U, and a hand-off.
set -e
# pacman 7 sandboxes downloads (Landlock, seccomp, a download user), which a
# container's kernel or an emulated one may not offer; this container is
# thrown away after the run.
grep -q '^DisableSandbox$' /etc/pacman.conf || sed -i '0,/^\[options\]/s//[options]\nDisableSandbox/' /etc/pacman.conf
sed -i 's/^DownloadUser/#DownloadUser/' /etc/pacman.conf
pacman -Syu --noconfirm >/dev/null
pacman -S --noconfirm --needed rust pkgconf git gtk3 xdotool libayatana-appindicator \
  libxkbcommon-x11 libglvnd hicolor-icon-theme ffmpeg >/dev/null
id builder >/dev/null 2>&1 || useradd -m builder
mkdir -p /home/builder/.cargo && chown -R builder:builder /home/builder

version=$(sed -n 's/^pkgver=//p' desktop/packaging/PKGBUILD)
top="Easy-Download-Manager-desktop-v$version"
build=/home/builder/pkg
rm -rf "$build" && mkdir -p "$build/$top"
cp -r . "$build/$top/"
(cd "$build" && tar -czf "dm-$version.tar.gz" "$top" && rm -rf "$top")
# The release PKGBUILD, fetching this checkout instead of a GitHub tag.
# Its digest is the release tarball's, which this tarball made just now is
# not: skipped for it, and kept in the PKGBUILD a person builds.
sed -e "s|^source=.*|source=(\"dm-$version.tar.gz\")|" \
    -e "s|^sha256sums=.*|sha256sums=('SKIP')|" desktop/packaging/PKGBUILD > "$build/PKGBUILD"
cp desktop/packaging/dm.desktop "$build/" 2>/dev/null || true
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
for candidate in "$build"/dm-*.pkg.tar.*; do
  case $candidate in *debug*) continue ;; esac
  [ -e "$candidate" ] && { pkg=$candidate; break; }
done
[ -n "$pkg" ] || { echo "== makepkg made no package"; exit 1; }
echo "== built: $pkg"
echo "== namcap-free install with pacman"
pacman -U --noconfirm "$pkg" >/dev/null
pacman -Ql dm | grep -E 'bin/|\.desktop$|icons'
echo "== the installed dm answers a browser hand-off"
printf '\053\000\000\000{"protocolVersion":1,"type":"engine.hello"}' \
  | DM_HOME=/tmp/dm-home /usr/bin/dm --native-host | head -c 300 | tail -c +5
echo
echo "== arch package ok"
