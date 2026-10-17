#!/bin/sh
# Builds and drives the client on Linux, in a container, from any machine.
#
#   sh scripts/linux-check.sh
#
# What the Rust and JS suites cannot see: the client as a Linux process —
# the tray library it loads at startup, the instance lock, the inbox, a
# browser's native-messaging hand-off, yt-dlp found by its Linux home. The
# build's target directory lives in a named volume so a second run is
# minutes, not the first run's half hour; the cargo registry is the host's.
#
# The working tree is streamed in rather than mounted: a checkout under a
# folder macOS guards (Downloads, Desktop, Documents) cannot be bind-mounted
# by Docker Desktop without a permission it may not have, and the failure is
# "Operation not permitted" on the first file read. The docker CLI itself
# runs from this shell, which can read them.
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
      "$tag" sh -c 'mkdir -p /work && cd /work && tar -xf - && sh scripts/linux/inside.sh'
