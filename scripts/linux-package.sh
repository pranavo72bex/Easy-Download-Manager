#!/bin/sh
# Builds the Linux packages in a container and drives what the .deb
# installs, from any machine with Docker:
#
#   sh scripts/linux-package.sh
#
# Streamed in, not mounted, for the reason `linux-check.sh` gives.
set -e
cd "$(dirname "$0")/.."
# DM_PLATFORM=linux/amd64 builds and drives the x86_64 client on an arm64
# machine (or the other way), under Docker's emulation: slower, nothing to
# install. Each platform keeps its own image and build volume.
# DM_DISTRO=ubuntu2204 runs it on Ubuntu 22.04, the system CI builds the
# release on, instead of Debian 12.
platform="${DM_PLATFORM:-}"
distro="${DM_DISTRO:-}"
suffix="${platform:+-$(echo "$platform" | tr / -)}${distro:+-$distro}"
tag="dm-linux-check$suffix"
volume="dm-linux-target$suffix"
dockerfile="scripts/linux/Dockerfile${distro:+.$distro}"
docker build ${platform:+--platform "$platform"} -f "$dockerfile" -t "$tag" scripts/linux
git ls-files -z --cached --others --exclude-standard \
  | tar --null -T - -cf - \
  | exec docker run --rm -i ${platform:+--platform "$platform"} \
      -v "$volume":/target \
      -v "$HOME/.cargo/registry":/usr/local/cargo/registry \
      -e DM_FORMATS="${DM_FORMATS:-}" \
      "$tag" sh -c 'mkdir -p /work && cd /work && tar -xf - && sh scripts/linux/package.sh'
