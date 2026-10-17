#!/bin/sh
# Builds the extension and the store zip for each browser.
#
#   sh scripts/package.sh            build, stage and zip
#   sh scripts/package.sh --build    build extension/pkg only, for loading
#                                    extension/ unpacked
#
# The extension is Rust (browser/), compiled to WebAssembly, plus the loader
# and manifests in extension/. wasm-bindgen turns the build into the module
# the loader imports, and its CLI must be exactly the version of the
# wasm-bindgen crate in browser/Cargo.lock — a mismatch is refused at load
# time, in the browser, with an error nobody sees — so it is checked here and
# installed if it is missing or different.

set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"
version=$(sed -n 's/.*"version": "\([^"]*\)".*/\1/p' "$root/extension/manifest.json" | head -1)
dist="$root/dist"
pkg="$root/extension/pkg"

# ------------------------------------------------------------- toolchain

if ! rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown; then
  echo "adding the wasm32-unknown-unknown target"
  rustup target add wasm32-unknown-unknown
fi

[ -f "$root/browser/Cargo.lock" ] || (cd "$root/browser" && cargo generate-lockfile)
wanted=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print; exit }' \
  "$root/browser/Cargo.lock")
if [ -z "$wanted" ]; then
  echo "could not read the wasm-bindgen version from browser/Cargo.lock" >&2
  exit 1
fi
have=$(wasm-bindgen --version 2>/dev/null | awk '{ print $2 }' || true)
if [ "$have" != "$wanted" ]; then
  echo "installing wasm-bindgen-cli $wanted (found: ${have:-none})"
  cargo install wasm-bindgen-cli --version "$wanted" --locked
fi

# ----------------------------------------------------------------- build

echo "building the extension (dm-catcher, wasm32)"
(cd "$root/browser" && cargo build --release --target wasm32-unknown-unknown --locked)
rm -rf "$pkg"
wasm-bindgen --target web --no-typescript --out-dir "$pkg" \
  "$root/browser/target/wasm32-unknown-unknown/release/dm_catcher.wasm"
echo "  extension/pkg: $(du -k "$pkg/dm_catcher_bg.wasm" | cut -f1) KB of WebAssembly"

[ "${1:-}" = "--build" ] && exit 0

# ------------------------------------------------------------------ pack

# Everything the browser loads, listed rather than globbed so nothing stray
# ships; the check below compares it with what the manifests name.
files="background.js content.js popup.html popup.js pkg icons"

rm -rf "$dist"
mkdir -p "$dist"

pack() {
  browser=$1
  manifest=$2
  stage="$dist/$browser"

  mkdir -p "$stage"
  # shellcheck disable=SC2086
  (cd "$root/extension" && cp -R $files "$stage/")
  rm -f "$stage/pkg/.gitignore"
  cp "$root/extension/$manifest" "$stage/manifest.json"

  # Every file a manifest names must be in what ships, and so must the
  # module the loader imports and the WebAssembly that module fetches.
  for named in $(sed -n 's/.*"\(icons\/[^"]*\)".*/\1/p; s/.*"\(background\.js\)".*/\1/p; s/.*"\(content\.js\)".*/\1/p; s/.*"\(popup\.html\)".*/\1/p' "$stage/manifest.json") \
               popup.js pkg/dm_catcher.js pkg/dm_catcher_bg.wasm; do
    if [ ! -f "$stage/$named" ]; then
      echo "the $browser build is missing $named" >&2
      exit 1
    fi
  done
  if find "$stage" -name '*.js' | grep -v -e '/background.js$' -e '/content.js$' -e '/popup.js$' -e '/pkg/dm_catcher.js$' >/dev/null; then
    echo "the $browser build carries JavaScript that is neither the loader nor wasm-bindgen's" >&2
    exit 1
  fi

  (cd "$stage" && zip -qr "$dist/dm-$browser-$version.zip" .)
  echo "  dist/$browser (unpacked)  dist/dm-$browser-$version.zip"
}

echo "packaging dm $version"
pack chrome manifest.json
pack firefox manifest.firefox.json
