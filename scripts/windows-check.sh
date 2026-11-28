#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# Compiles the client for Windows, in a container, from any machine.
#
#   sh scripts/windows-check.sh            # cargo check, every target
#   sh scripts/windows-check.sh clippy     # and clippy, warnings as errors
#
# Nothing Windows is installed: the messense/cargo-xwin image carries clang,
# lld and Microsoft's CRT and SDK headers (fetched by xwin on first use, then
# kept in a volume), which is what the C inside the TLS stack needs; Rust is
# the version rust-toolchain.toml pins, kept in a volume too. It finds what does not
# compile on Windows. Running EDM there needs Windows itself,
# which is CI's job (.github/workflows).
#
# The tree is streamed in rather than mounted, as in linux-check.sh: Docker
# cannot read a checkout under a folder macOS guards.
set -e
cd "$(dirname "$0")/.."
job="check"
[ "$1" = "clippy" ] && job="clippy"
flags=""
[ "$job" = "clippy" ] && flags="-- -D warnings"
git ls-files -z --cached --others --exclude-standard \
  | tar --null -T - -cf - \
  | exec docker run --rm -i \
      -v edm-xwin-target:/target \
      -v edm-xwin-cache:/root/.cache \
      -v edm-xwin-rustup:/usr/local/rustup \
      -v "$HOME/.cargo/registry":/usr/local/cargo/registry \
      -e CARGO_TARGET_DIR=/target \
      messense/cargo-xwin sh -c "mkdir -p /work && cd /work && tar -xf - \
        && rustup -q toolchain install \
        && rustup -q target add x86_64-pc-windows-msvc \
        && cargo xwin $job --workspace --all-targets --target x86_64-pc-windows-msvc $flags"
