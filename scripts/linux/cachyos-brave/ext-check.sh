#!/bin/sh
# Loads the repository's extension into Brave on CachyOS (docker) and checks the
# panel's WebAssembly loads on a page with a video. Run run.sh once first, or
# this builds the image itself.
#
#   sh scripts/linux/cachyos-brave/ext-check.sh
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
mkdir -p "$here/local"; touch "$here/local/.keep"
docker build --platform linux/amd64 -t dm-cachyos-brave "$here" >/dev/null
docker run --rm --platform linux/amd64 -v "$here:/scripts:ro" -v "$root/extension:/ext:ro" ${EXT_OLD:+-v "$EXT_OLD:/ext-old:ro"} dm-cachyos-brave \
  python3 /scripts/ext-check.py
