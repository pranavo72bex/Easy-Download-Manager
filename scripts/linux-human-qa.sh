#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# The whole of human-qa.py on Linux, in a Debian 12 container on an X display,
# from any machine with Docker:
#
#   DM_RELEASE=desktop-v0.9.0 sh scripts/linux-human-qa.sh   # a release as people get it
#   DM_DEB=dm_0.9.0_arm64.deb sh scripts/linux-human-qa.sh   # a .deb at hand
#   sh scripts/linux-human-qa.sh                             # a build of the checkout
#   sh scripts/linux-human-qa.sh --save-baseline             # the window's Linux pictures
#
# A build of the checkout is the GPUI window (dm-gpui), checked against its
# pictures in docs/behaviour-baseline/gpui/linux. A .deb is checked with the
# window it installs.
# DM_RELEASE downloads that release's .deb for Docker's own architecture and
# its Chrome extension zip with `gh` (a draft too, for whoever may see it),
# into a folder of their own, and checks each against the release's
# latest.json before anything is installed. DM_DEB takes a .deb from disk,
# and DM_EXTENSION_ZIP an extension zip (else dist/chrome, from
# `sh scripts/package.sh`). With neither, dm is built in the Debian image
# linux-check.sh uses, into its dm-linux-target volume.
#
# The .deb is installed by apt, so its dependencies are what a person gets.
# Chromium stands in for Edge, which is published for x86_64 alone
# (linux/human-distro.sh runs Edge under emulation). Nobody watches the X
# display: the screen is pictured every ten seconds, and the pictures and
# human-qa.py's words come back into DM_SHOTS (./human-shots/linux-qa).
# The window's pictures are compared with docs/behaviour-baseline/gpui/linux,
# which --save-baseline writes from this image; look at them before keeping
# them. Streamed in, not mounted, for the reason linux-check.sh gives.
set -e
cd "$(dirname "$0")/.."
SHOTS="${DM_SHOTS:-$PWD/human-shots/linux-qa}"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/work" "$stage/in"
arch=$(docker version --format '{{.Server.Arch}}')
# shellcheck source=scripts/linux/window.sh
. scripts/linux/window.sh

if [ -n "$DM_RELEASE" ]; then
  echo "== $DM_RELEASE's .deb ($arch) and Chrome extension, from GitHub"
  got="$stage/download"
  mkdir -p "$got"
  gh release download "$DM_RELEASE" -D "$got" -p "dm_*_${arch}.deb" -p 'dm-chrome-*.zip' -p latest.json
  python3 -I - "$got" <<'EOF'
import hashlib, json, pathlib, sys
got = pathlib.Path(sys.argv[1])
digests = json.loads((got / "latest.json").read_text())["sha256"]
for f in sorted(got.iterdir()):
    if f.name == "latest.json":
        continue
    if hashlib.sha256(f.read_bytes()).hexdigest() != digests.get(f.name):
        sys.exit(f"{f.name} is not the file latest.json names")
    print(f"  {f.name}: as latest.json names it")
EOF
  DM_DEB=$(ls "$got"/dm_*_"$arch".deb)
  DM_EXTENSION_ZIP=$(ls "$got"/dm-chrome-*.zip)
fi

if [ -n "$DM_DEB" ]; then
  cp "$DM_DEB" "$stage/in/dm.deb"
else
  echo "== dm, built in the Debian image"
  docker build -q -t dm-linux-check scripts/linux >/dev/null
  { git ls-files -z --cached --others --exclude-standard; } | tar --null -T - -cf - \
    | docker run --rm -i -v dm-linux-target:/target -v "$HOME/.cargo/registry":/usr/local/cargo/registry \
      dm-linux-check sh -c 'set -e; mkdir -p /work && cd /work && tar -xf - && cd desktop && cargo build -q -p "$1"' build "$binary"
fi
if [ -n "$DM_EXTENSION_ZIP" ]; then
  mkdir -p "$stage/in/chrome"
  (cd "$stage/in/chrome" && unzip -q "$DM_EXTENSION_ZIP")
elif [ ! -f dist/chrome/manifest.json ]; then
  echo "no extension: sh scripts/package.sh, or DM_EXTENSION_ZIP / DM_RELEASE"; exit 1
fi
{ git ls-files -z --cached --others --exclude-standard; find dist/chrome -type f -print0 2>/dev/null; } \
  | tar --null -T - -cf - | tar -xf - -C "$stage/work"

echo "== human-qa.py on Debian 12 ($arch), Chromium in Edge's place"
docker build -q -t dm-human-qa -f scripts/linux/Dockerfile.human-qa scripts/linux >/dev/null
volume=dm-human-shots-qa
docker volume rm "$volume" >/dev/null 2>&1 || true
status=0
tar -C "$stage" -cf - work in | docker run --rm -i -e DM_LINUX_BINARY="$binary" \
  -v dm-linux-target:/target:ro -v "$volume":/shots dm-human-qa \
  sh -c 'set -e; tar -xf - -C / && cd /work && sh scripts/linux/human-qa-session.sh "$@"' qa "$@" || status=$?
rm -rf "$SHOTS" && mkdir -p "$SHOTS"
docker run --rm -v "$volume":/shots dm-human-qa tar -C /shots -cf - . | tar -xf - -C "$SHOTS"
# The window checked, as the session found it (a .deb's own).
[ -f "$SHOTS/binary" ] && binary=$(cat "$SHOTS/binary")
pictures=docs/behaviour-baseline/gpui/linux
case " $* " in *" --save-baseline "*)
  mkdir -p "$pictures"
  cp "$SHOTS"/baseline/*.png "$pictures"/
  echo "baseline pictures: $pictures (look at them before keeping them)" ;;
esac
echo "screenshots and human-qa.py's words: $SHOTS"
exit $status
