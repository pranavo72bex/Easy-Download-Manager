#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# Builds the Linux packages in a container and drives what the .deb
# installs, from any machine with Docker:
#
#   sh scripts/linux-package.sh
#
# Streamed in, not mounted, for the reason `linux-check.sh` gives.
set -e
cd "$(dirname "$0")/.."
# EDM_PLATFORM=linux/amd64 builds and drives the x86_64 client on an arm64
# machine (or the other way), under Docker's emulation: slower, nothing to
# install. Each platform keeps its own image and build volume.
# EDM_DISTRO=ubuntu2204 runs it on Ubuntu 22.04, the system CI builds the
# release on, instead of Debian 12.
# EDM_OUT=folder only builds: the .deb, the AppImage and the .rpm are copied
# there, and nothing is installed or driven (what `make build-linux` uses).
platform="${EDM_PLATFORM:-}"
distro="${EDM_DISTRO:-}"
suffix="${platform:+-$(echo "$platform" | tr / -)}${distro:+-$distro}"
tag="edm-linux-check$suffix"
volume="edm-linux-target$suffix"
dockerfile="scripts/linux/Dockerfile${distro:+.$distro}"
docker build ${platform:+--platform "$platform"} -f "$dockerfile" -t "$tag" scripts/linux
git ls-files -z --cached --others --exclude-standard \
  | tar --null -T - -cf - \
  | exec docker run --rm -i ${platform:+--platform "$platform"} \
      -v "$volume":/target \
      -v "$HOME/.cargo/registry":/usr/local/cargo/registry \
      -e EDM_FORMATS="${EDM_FORMATS:-}" \
      -e EDM_BUILD_ONLY="${EDM_OUT:+1}" \
      ${EDM_OUT:+-v "$EDM_OUT":/out} \
      "$tag" sh -c 'mkdir -p /work && cd /work && tar -xf - && sh scripts/linux/package.sh'
