#!/bin/sh
# Compile-checks dm's Windows-only modules for x86_64-pc-windows-msvc from any
# computer, with `rustup target add x86_64-pc-windows-msvc` done once.
#
#     sh scripts/windows-modules-check.sh
#
# `cargo check --target x86_64-pc-windows-msvc -p dm-desktop` does not get
# past the C build of aws-lc-sys without a Windows toolchain. These modules
# need only `windows-sys` and `windows`, which are plain Rust, so each is
# included in a small crate of its own and checked there; their logic is
# tested too (on this computer, where the system calls are not made).
# It does not run them: that is for a Windows computer.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
src="$root/desktop/crates/dm-desktop/src"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/src"
cat > "$work/Cargo.toml" <<TOML
[package]
name = "windows-modules-check"
version = "0.0.0"
edition = "2021"
[dependencies]
windows-sys = { version = "0.61", features = ["Win32_Foundation", "Win32_UI_Shell", "Win32_UI_WindowsAndMessaging", "Win32_UI_Accessibility", "Win32_Graphics_Gdi"] }
[target.'cfg(windows)'.dependencies]
windows = { version = "0.62", features = ["Win32_Foundation", "Win32_System_Com", "Win32_System_Com_StructuredStorage", "Win32_System_Variant", "Win32_UI_Shell", "Win32_UI_Shell_Common", "Win32_UI_Shell_PropertiesSystem", "Win32_Storage_EnhancedStorage"] }
TOML
: > "$work/src/lib.rs"
for module in quiet contrast jumplist; do
  printf '#[path = "%s/%s.rs"]\npub mod %s;\n' "$src" "$module" "$module" >> "$work/src/lib.rs"
done
cd "$work"
echo "== check for x86_64-pc-windows-msvc"
cargo check --target x86_64-pc-windows-msvc
echo "== their logic, here"
cargo test
