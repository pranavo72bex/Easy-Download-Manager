#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# Puts dm's Safari extension into a dm.app, on a Mac (gaps row R4).
#
#   sh scripts/safari-extension.sh path/to/dm.app           the extension's program built for release
#   sh scripts/safari-extension.sh path/to/dm.app --debug   built for debugging (quicker)
#
# Safari loads a web extension only from inside an app: an app extension
# (`dm Safari Extension.appex`, in dm.app/Contents/PlugIns) whose Resources are the
# extension's files and whose program Safari sends the extension's native messages to.
# Xcode's converter makes one; this makes the same with the command line tools alone:
#
# - the extension's files, for Safari (`dist/safari`, from scripts/package.sh);
# - the program, Rust (desktop/crates/dm-safari), which hands each message to dm's own
#   native host and gives Safari its answer;
# - its Info.plist and its sandbox (desktop/assets/macos/safari/);
# - signed: the extension with its sandbox, then dm.app again around it. With no
#   DM_SIGN_IDENTITY it is signed ad hoc ("-"), as dm.app is, and Safari runs it only with
#   Develop > Allow Unsigned Extensions (docs/how-it-works.md, Safari).
#
# Once dm.app has been opened (or registered: lsregister -f path/to/dm.app), Safari lists
# the extension in Settings > Extensions, and `pluginkit -m -i com.dm.desktop.safari-extension`
# shows where macOS found it.

set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"

if [ "$(uname)" != Darwin ]; then
  echo "Safari's extension is built on a Mac" >&2
  exit 1
fi
if [ $# -lt 1 ] || [ ! -f "${1%/}/Contents/Info.plist" ]; then
  echo "usage: sh scripts/safari-extension.sh path/to/dm.app [--debug]" >&2
  exit 1
fi
app=$(cd "${1%/}" && pwd)
profile=release
cargo_profile=--release
if [ "${2:-}" = "--debug" ]; then
  profile=debug
  cargo_profile=
fi
identity=${DM_SIGN_IDENTITY:--}
assets="$root/desktop/assets/macos/safari"

# ------------------------------------------------------------------ build

# The extension's files, built as the stores' are.
sh "$root/scripts/package.sh"
[ -f "$root/dist/safari/manifest.json" ] || {
  echo "scripts/package.sh made no dist/safari" >&2
  exit 1
}

echo "building the Safari extension's program (dm-safari, $profile)"
# shellcheck disable=SC2086
(cd "$root/desktop" && cargo build -p dm-safari $cargo_profile --locked)
program="${CARGO_TARGET_DIR:-$root/desktop/target}/$profile/dm-safari"
[ -x "$program" ] || {
  echo "no dm-safari at $program" >&2
  exit 1
}

# ------------------------------------------------------------------ bundle

plist() { plutil -extract "$1" raw -o - "$app/Contents/Info.plist"; }
app_id=$(plist CFBundleIdentifier)
version=$(plist CFBundleShortVersionString 2>/dev/null || echo 0.0.0)
build=$(plist CFBundleVersion 2>/dev/null || echo "$version")

appex="$app/Contents/PlugIns/dm Safari Extension.appex"
rm -rf "$appex"
mkdir -p "$appex/Contents/MacOS" "$appex/Contents/Resources"
cp "$program" "$appex/Contents/MacOS/dm-safari"
cp -R "$root/dist/safari/." "$appex/Contents/Resources/"
sed -e "s|@ID@|$app_id.safari-extension|" -e "s|@VERSION@|$version|" -e "s|@BUILD@|$build|" \
  "$assets/Info.plist" >"$appex/Contents/Info.plist"
plutil -lint -s "$appex/Contents/Info.plist"

# -------------------------------------------------------------------- sign

# The extension first, with its sandbox; then dm.app around it, keeping what dm.app was
# signed with (its hardened runtime). Not --deep, which would sign the extension again
# without its sandbox, and macOS runs no extension outside one.
codesign --force --sign "$identity" --options runtime --timestamp=none \
  --entitlements "$assets/entitlements.plist" "$appex"
codesign --force --sign "$identity" --options runtime --timestamp=none \
  --preserve-metadata=entitlements,requirements,flags "$app"
codesign --verify --deep --strict "$app"

echo "  $appex"
echo "  open dm.app once (or: /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f \"$app\"),"
echo "  then Safari > Settings > Extensions lists dm"
