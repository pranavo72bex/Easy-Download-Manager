#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# The workspace tests and the stress phases on Linux, in a container whose
# network is switched off (loopback only), from any machine:
#
#   sh scripts/linux-stress.sh
#
# Same image, build volume and tree streaming as linux-check.sh.
set -e
# The stress phases write gigabytes: to a folder of the host (EDM_STRESS_DIR,
# default under $TMPDIR), not to the container's disk, which is Docker's
# small virtual disk shared with everything else it runs.
scratch="${EDM_STRESS_DIR:-${TMPDIR:-/tmp}/edm-stress-linux}"
mkdir -p "$scratch"
cd "$(dirname "$0")/.."
docker build -f scripts/linux/Dockerfile -t edm-linux-check scripts/linux
git ls-files -z --cached --others --exclude-standard \
  | tar --null -T - -cf - \
  | exec docker run --rm -i --network none -e TMPDIR=/stress ${EDM_ONLY:+-e EDM_ONLY="$EDM_ONLY"} -v "$scratch":/stress \
      -v edm-linux-target:/target \
      -v "$HOME/.cargo/registry":/usr/local/cargo/registry \
      edm-linux-check sh -c 'mkdir -p /work && cd /work && tar -xf - && sh scripts/linux/stress-inside.sh'
