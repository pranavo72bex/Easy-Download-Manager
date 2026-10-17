#!/bin/sh
# Builds the Arch package with makepkg from this checkout, installs it with
# pacman and asks the installed dm to answer a browser hand-off.
#
#   sh scripts/arch-check.sh
#
# Arch's own images are x86_64; on an arm64 machine this uses Arch Linux ARM
# (menci/archlinuxarm), which runs natively. The PKGBUILD is the release's,
# with its source pointed at this checkout instead of a tag on GitHub.
# Streamed in, not mounted, for the reason linux-check.sh gives.
set -e
cd "$(dirname "$0")/.."
image="${DM_ARCH_IMAGE:-menci/archlinuxarm:base-devel}"
git ls-files -z --cached --others --exclude-standard \
  | tar --null -T - -cf - \
  | exec docker run --rm -i \
      -v dm-arch-cargo:/home/builder/.cargo \
      "$image" sh -c 'mkdir -p /work && cd /work && tar -xf - && sh scripts/arch/inside.sh'
