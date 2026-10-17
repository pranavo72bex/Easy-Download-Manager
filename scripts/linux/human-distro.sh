#!/bin/sh
# The browser checks on a Linux other than Debian: what
# linux-chromium-human.sh and linux-firefox-check.sh run when DM_DISTRO
# names one.
#
#   sh scripts/linux/human-distro.sh chromium fedora|arch
#   sh scripts/linux/human-distro.sh firefox ubuntu2204|fedora
#
# dm is built once, in the Debian image, into the dm-linux-target volume, as
# the Debian runs build it. The check then runs in the distribution's own
# image (Dockerfile.human-<distro>), which has only what a person on that
# distribution would install: its browser, dm's libraries, and the test's X
# server, window manager and xdotool. The volume is mounted read-only and
# dm is copied out of it, so a build started meanwhile by another check
# cannot change the binary under this one. Screenshots come back through a
# volume of this check's own (dm-human-shots-<browser>-<distro>) into
# DM_SHOTS, or ./human-shots/<browser>-<distro>.
# Streamed in, not mounted, for the reason linux-check.sh gives.
set -e
cd "$(dirname "$0")/../.."
browser=$1 distro=$2
case "$browser" in
  chromium) dist=dist/chrome ;;
  firefox) dist=dist/firefox ;;
  *) echo "usage: $0 chromium|firefox <distro>"; exit 2 ;;
esac
[ -f scripts/linux/Dockerfile.human-$distro ] || { echo "no image for $distro (scripts/linux/Dockerfile.human-$distro)"; exit 2; }
[ -f $dist/pkg/dm_catcher_bg.wasm ] || { echo "no $dist — run: sh scripts/package.sh"; exit 1; }
SHOTS="${DM_SHOTS:-$PWD/human-shots/$browser-$distro}"
stream() { { git ls-files -z --cached --others --exclude-standard; find $dist -type f -print0; } | tar --null -T - -cf -; }

echo "== dm, built in the Debian image"
docker build -q -t dm-linux-check scripts/linux >/dev/null
stream | docker run --rm -i -v dm-linux-target:/target \
  -v "$HOME/.cargo/registry":/usr/local/cargo/registry dm-linux-check sh -c '
    set -e; mkdir -p /work && cd /work && tar -xf -
    cd desktop && cargo build -q -p dm-desktop'

echo "== $browser on $distro"
base=
# Arch's own images are x86_64; on arm64, Arch Linux ARM, as arch-check.sh.
[ "$distro" = arch ] && [ "$(uname -m)" = arm64 -o "$(uname -m)" = aarch64 ] && base=menci/archlinuxarm:base-devel
image="dm-human-$distro-$browser"
docker build -q -t "$image" --build-arg BROWSER="$browser" ${base:+--build-arg BASE=$base} \
  -f scripts/linux/Dockerfile.human-$distro scripts/linux >/dev/null
volume="dm-human-shots-$browser-$distro"
status=0
stream | docker run --rm -i -e DM_LIVE_PAGE="${DM_LIVE_PAGE:-}" \
  -v dm-linux-target:/target:ro -v "$volume":/shots \
  $([ "$browser" = firefox ] && echo -v dm-firefox-cache:/root/.cache/dm-firefox) \
  "$image" sh -c "set -e; mkdir -p /work && cd /work && tar -xf - && sh scripts/linux/human-session.sh $browser" \
  || status=$?
rm -rf "$SHOTS" && mkdir -p "$SHOTS"
docker run --rm -v "$volume":/shots "$image" tar -C /shots -cf - . | tar -xf - -C "$SHOTS"
echo "screenshots: $SHOTS"
exit $status
