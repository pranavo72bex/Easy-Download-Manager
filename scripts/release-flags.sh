#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# The compiler flags a release build is made with, printed for
# CARGO_BUILD_RUSTFLAGS:
#
#   CARGO_BUILD_RUSTFLAGS="$(sh scripts/release-flags.sh)" cargo build --release
#
# They take the building computer's own paths out of what ships. A panic
# message names the source file it came from, and without these the app and
# the extension's WebAssembly carried the builder's home folder, user name and
# checkout (/Users/<name>/…/.cargo/registry/src/…) a thousand times over.
# Each prefix is written as a neutral one: the checkout as `dm`, the crates
# Cargo fetched as `crates`, any other path under the home folder as `~`.
#
# CARGO_BUILD_RUSTFLAGS is build.rustflags, which Cargo leaves aside where a
# target has rustflags of its own: the Windows target's +crt-static
# (desktop/.cargo/config.toml) is kept, and Windows builds are as before.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
home=${HOME:-}
cargo_home=${CARGO_HOME:-$home/.cargo}
flags=""
add() {
  # A path with a space cannot ride in CARGO_BUILD_RUSTFLAGS, which splits on
  # spaces: left as it is rather than breaking the build.
  case "$1" in *" "*) return ;; esac
  [ -n "$1" ] && flags="$flags --remap-path-prefix=$1=$2"
}
# The last flag that matches a path is the one used: the most specific last.
add "$home" "~"
add "$cargo_home/registry/src" "crates"
add "$cargo_home/git/checkouts" "crates"
add "$root" "dm"
printf '%s\n' "${flags# }"
