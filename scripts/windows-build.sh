#!/bin/sh
# Builds dm for Windows (x86_64) in a container, from any machine with
# Docker, and lays it out as the release's portable zip: dm-desktop.exe, the
# empty file named `portable`, and the README.
#
#   sh scripts/windows-build.sh [OUTDIR]        # default: build/windows
#
# The same image and volumes as windows-check.sh (clang, lld and Microsoft's
# CRT and SDK headers through xwin). The NSIS installer needs Windows itself;
# CI makes it (.github/workflows/desktop.yml).
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
        && rustup -q toolchain install stable --profile minimal -t x86_64-pc-windows-msvc \
        && rustup -q default stable \
        && cargo xwin build --release --locked -p dm-desktop --target x86_64-pc-windows-msvc \
        && cp /target/x86_64-pc-windows-msvc/release/dm-desktop.exe /out/dm-desktop.exe"
portable="$out/dm_${version}_x64-portable"
rm -rf "$portable" "$portable.zip"
mkdir -p "$portable"
cp "$out/dm-desktop.exe" "$portable/"
: > "$portable/portable"
cp desktop/packaging/portable-readme.txt "$portable/README.txt"
(cd "$out" && zip -qr "dm_${version}_x64-portable.zip" "dm_${version}_x64-portable")
rm -rf "$portable"
echo "built $out/dm_${version}_x64-portable.zip"
