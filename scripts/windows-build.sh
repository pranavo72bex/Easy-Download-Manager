#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# Builds dm for Windows (x86_64) in a container, from any machine with
# Docker, and lays it out as the release's portable zip: dm-desktop.exe, the
# empty file named `portable`, and the README.
#
#   sh scripts/windows-build.sh [OUTDIR]        # default: build/windows
#
# The same image and volumes as windows-check.sh (clang, lld and Microsoft's
# CRT and SDK headers through xwin). The NSIS installer needs Windows itself;
# CI makes it (.github/workflows/desktop.yml).
#
# The egui window's program: the window dm ships on Windows is GPUI's
# (dm-gpui.exe, docs/gaps.md G9), whose release build compiles its Direct3D
# shaders with the Windows SDK's fxc.exe, which only a build on Windows has
# (docs/decisions.md, 2026-11-11). CI and a Windows computer build that one.
set -e
cd "$(dirname "$0")/.."
out=$(mkdir -p "${1:-build/windows}" && cd "${1:-build/windows}" && pwd)
version=$(sed -n 's/^version = "\(.*\)"/\1/p' desktop/crates/dm-desktop/Cargo.toml | head -1)
git ls-files -z --cached --others --exclude-standard \
  | tar --null -T - -cf - \
  | docker run --rm -i \
      -v dm-xwin-target:/target \
      -v dm-xwin-cache:/root/.cache \
      -v dm-xwin-rustup:/usr/local/rustup \
      -v "$HOME/.cargo/registry":/usr/local/cargo/registry \
      -v "$out":/out \
      -e CARGO_TARGET_DIR=/target \
      messense/cargo-xwin sh -c "mkdir -p /work && cd /work && tar -xf - && cd desktop \
        && rustup -q toolchain install \
        && rustup -q target add x86_64-pc-windows-msvc \
        && cargo xwin build --release --locked -p dm-desktop --target x86_64-pc-windows-msvc \
        && cp /target/x86_64-pc-windows-msvc/release/dm-desktop.exe /out/dm-desktop.exe"
portable="$out/dm_${version}_x64-portable"
rm -rf "$portable" "$portable.zip"
mkdir -p "$portable"
cp "$out/dm-desktop.exe" "$portable/"
: > "$portable/portable"
# The readme names the program the release zip holds (dm-gpui.exe); this
# zip holds the egui window's.
sed 's/dm-gpui\.exe/dm-desktop.exe/g' desktop/packaging/portable-readme.txt > "$portable/README.txt"
(cd "$out" && zip -qr "dm_${version}_x64-portable.zip" "dm_${version}_x64-portable")
rm -rf "$portable"
echo "built $out/dm_${version}_x64-portable.zip"
