#!/bin/sh
# The workspace tests and the stress phases on Linux, in a container whose
# network is switched off (loopback only), from any machine:
#
#   sh scripts/linux-stress.sh
#
# Same image, build volume and tree streaming as linux-check.sh.
set -e
# The stress phases write gigabytes: to a folder of the host (DM_STRESS_DIR,
# default under $TMPDIR), not to the container's disk, which is Docker's
# small virtual disk shared with everything else it runs.
scratch="${DM_STRESS_DIR:-${TMPDIR:-/tmp}/dm-stress-linux}"
mkdir -p "$scratch"
cd "$(dirname "$0")/.."
docker build -f scripts/linux/Dockerfile -t dm-linux-check scripts/linux
git ls-files -z --cached --others --exclude-standard \
  | tar --null -T - -cf - \
  | exec docker run --rm -i --network none -e TMPDIR=/stress ${DM_ONLY:+-e DM_ONLY="$DM_ONLY"} -v "$scratch":/stress \
      -v dm-linux-target:/target \
      -v "$HOME/.cargo/registry":/usr/local/cargo/registry \
      dm-linux-check sh -c 'mkdir -p /work && cd /work && tar -xf - && sh scripts/linux/stress-inside.sh'
