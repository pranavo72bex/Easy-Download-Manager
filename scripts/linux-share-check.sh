#!/bin/sh
# The network share, fetched from outside the process that serves it, in a
# Linux container:
#
#   sh scripts/linux-share-check.sh
#
# Shares a file on the container's own network address and fetches it back
# with plain tools: whole, by range, eight ranges at once, wrongly, and after
# the share has ended (scripts/linux/share_inside.sh). Uses linux-check.sh's
# image and build volume, and streams the tree in rather than mounting it, for
# the reason linux-check.sh gives.
set -e
cd "$(dirname "$0")/.."
docker build -f scripts/linux/Dockerfile -t dm-linux-check scripts/linux >/dev/null
git ls-files -z --cached --others --exclude-standard \
  | tar --null -T - -cf - \
  | exec docker run --rm -i \
      -v dm-linux-target:/target \
      -v "$HOME/.cargo/registry":/usr/local/cargo/registry \
      dm-linux-check sh -c 'mkdir -p /work && cd /work && tar -xf - && sh scripts/linux/share_inside.sh'
