#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# Puts EDM's Safari extension into an EDM.app, on a Mac (gaps row R4).
#
#   sh scripts/safari-extension.sh path/to/EDM.app           the extension's program built for release
#   sh scripts/safari-extension.sh path/to/EDM.app --debug   built for debugging (quicker)
#   sh scripts/safari-extension.sh path/to/EDM.app --dmg path/to/EDM.dmg
#                                   and that EDM.app put into the .dmg in place of the one there
#
# The .dmg: cargo-packager makes EDM.app and the .dmg from it in one run, so the .dmg is
# opened afterwards (read-write), its EDM.app replaced by this one and closed again as it
# was (compressed, signed as the packager signs it); its window's layout is the packager's.
# CARGO_BUILD_TARGET (the Intel Mac's build) builds the extension's program for that
# target too, and the program is held to EDM.app's own architectures.
#
# Safari loads a web extension only from inside an app: an app extension
# (`EDM Safari Extension.appex`, in EDM.app/Contents/PlugIns) whose Resources are the
# extension's files and whose program Safari sends the extension's native messages to.
# Xcode's converter makes one; this makes the same with the command line tools alone:
#
# - the extension's files, for Safari (`dist/safari`, from scripts/package.sh);
# - the program, Rust (crates/edm-safari), which hands each message to EDM's own
#   native host and gives Safari its answer;
# - its Info.plist and its sandbox (assets/macos/safari/);
# - signed: the extension with its sandbox, then EDM.app again around it. With no
#   EDM_SIGN_IDENTITY it is signed ad hoc ("-"), as EDM.app is, and Safari runs it only with
#   Develop > Allow Unsigned Extensions (docs/how-it-works.md, Safari).
#
# Once EDM.app has been opened (or registered: lsregister -f path/to/EDM.app), Safari lists
# the extension in Settings > Extensions, and `pluginkit -m -i com.edm.desktop.safari-extension`
# shows where macOS found it.

set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"

if [ "$(uname)" != Darwin ]; then
  echo "Safari's extension is built on a Mac" >&2
  exit 1
fi
usage() {
  echo "usage: sh scripts/safari-extension.sh path/to/EDM.app [--debug] [--dmg path/to/EDM.dmg]" >&2
  exit 1
}
if [ $# -lt 1 ] || [ ! -f "${1%/}/Contents/Info.plist" ]; then
  usage
fi
app=$(cd "${1%/}" && pwd)
shift
profile=release
cargo_profile=--release
dmg=
while [ $# -gt 0 ]; do
  case "$1" in
    --debug)
      profile=debug
      cargo_profile=
      ;;
    --dmg)
      [ $# -ge 2 ] && [ -f "$2" ] || {
        echo "--dmg names no .dmg: ${2:-}" >&2
        exit 1
      }
      dmg=$(cd "$(dirname "$2")" && pwd)/$(basename "$2")
      shift
      ;;
    *) usage ;;
  esac
  shift
done
target=${CARGO_BUILD_TARGET:-}
identity=${EDM_SIGN_IDENTITY:--}
assets="$root/assets/macos/safari"

# ------------------------------------------------------------------ build

# The extension's files, built as the stores' are.
sh "$root/scripts/package.sh"
[ -f "$root/dist/safari/manifest.json" ] || {
  echo "scripts/package.sh made no dist/safari" >&2
  exit 1
}

echo "building the Safari extension's program (edm-safari, $profile)"
# shellcheck disable=SC2086
(cd "$root" && cargo build -p edm-safari $cargo_profile --locked)
program="${CARGO_TARGET_DIR:-$root/target}/${target:+$target/}$profile/edm-safari"
[ -x "$program" ] || {
  echo "no edm-safari at $program" >&2
  exit 1
}

# ------------------------------------------------------------------ bundle

plist() { plutil -extract "$1" raw -o - "$app/Contents/Info.plist"; }
app_id=$(plist CFBundleIdentifier)
version=$(plist CFBundleShortVersionString 2>/dev/null || echo 0.0.0)
build=$(plist CFBundleVersion 2>/dev/null || echo "$version")

# The extension's program for the machines EDM.app runs on: an Intel EDM.app with an Apple
# silicon program in it would list the extension and never answer it.
app_archs=$(lipo -archs "$app/Contents/MacOS/$(plist CFBundleExecutable)")
program_archs=$(lipo -archs "$program")
[ "$app_archs" = "$program_archs" ] || {
  echo "EDM.app is $app_archs but edm-safari was built for $program_archs (CARGO_BUILD_TARGET)" >&2
  exit 1
}

appex="$app/Contents/PlugIns/EDM Safari Extension.appex"
rm -rf "$appex"
mkdir -p "$appex/Contents/MacOS" "$appex/Contents/Resources"
cp "$program" "$appex/Contents/MacOS/edm-safari"
cp -R "$root/dist/safari/." "$appex/Contents/Resources/"
sed -e "s|@ID@|$app_id.safari-extension|" -e "s|@VERSION@|$version|" -e "s|@BUILD@|$build|" \
  "$assets/Info.plist" >"$appex/Contents/Info.plist"
plutil -lint -s "$appex/Contents/Info.plist"

# -------------------------------------------------------------------- sign

# The extension first, with its sandbox; then EDM.app around it, keeping what EDM.app was
# signed with (its hardened runtime). Not --deep, which would sign the extension again
# without its sandbox, and macOS runs no extension outside one.
codesign --force --sign "$identity" --options runtime --timestamp=none \
  --entitlements "$assets/entitlements.plist" "$appex"
codesign --force --sign "$identity" --options runtime --timestamp=none \
  --preserve-metadata=entitlements,requirements,flags "$app"
codesign --verify --deep --strict "$app"

echo "  $appex"

# --------------------------------------------------------------------- dmg

if [ -n "$dmg" ]; then
  work=$(mktemp -d)
  trap 'hdiutil detach -quiet -force "$work/mnt" 2>/dev/null || true; rm -rf "$work"' EXIT
  hdiutil convert -quiet "$dmg" -format UDRW -o "$work/rw.dmg"
  # Room for this EDM.app beside what is there (the packager made the volume to fit its
  # own); compressed again below, the free space costs next to nothing.
  room=$(($(du -sk "$app" | cut -f1) * 2 + 20480))
  hdiutil resize -quiet -size "${room}k" "$work/rw.dmg"
  mkdir "$work/mnt"
  hdiutil attach -quiet -nobrowse -noverify -noautoopen -mountpoint "$work/mnt" "$work/rw.dmg"
  name=$(basename "$app")
  [ -d "$work/mnt/$name" ] || {
    echo "$dmg holds no $name" >&2
    exit 1
  }
  rm -rf "${work:?}/mnt/$name"
  ditto "$app" "$work/mnt/$name"
  codesign --verify --deep --strict "$work/mnt/$name"
  [ -d "$work/mnt/$name/Contents/PlugIns/EDM Safari Extension.appex" ]
  hdiutil detach -quiet "$work/mnt"
  hdiutil convert -quiet "$work/rw.dmg" -format UDZO -imagekey zlib-level=9 -o "$work/out.dmg"
  # Under the .dmg's own name, as the packager signs it (not "out").
  codesign --force --sign "$identity" --timestamp=none \
    --identifier "$(basename "$dmg" .dmg)" "$work/out.dmg"
  mv -f "$work/out.dmg" "$dmg"
  echo "  $dmg (its $name with the extension)"
fi
echo "  open EDM.app once (or: /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f \"$app\"),"
echo "  then Safari > Settings > Extensions lists EDM"
