#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# Builds the extension and the store zip for each browser.
#
#   sh scripts/package.sh            build, stage and zip
#   sh scripts/package.sh --build    build extension/pkg only, for loading
#                                    extension/ unpacked
#
# The extension is Rust (browser/), compiled to WebAssembly, plus the page
# scripts and manifests in extension/. wasm-bindgen turns the build into the
# module the background and the popup import, and its CLI must be exactly the version of the
# wasm-bindgen crate in browser/Cargo.lock — a mismatch is refused at load
# time, in the browser, with an error nobody sees — so it is checked here and
# installed if it is missing or different.

set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"
version=$(sed -n 's/.*"version": "\([^"]*\)".*/\1/p' "$root/extension/manifest.json" | head -1)
dist="$root/dist"
pkg="$root/extension/pkg"
# A Python that runs: on Windows `python3` can be the Microsoft Store's
# placeholder, which only prints how to install Python, and every check
# below then failed as though it had found something wrong.
py=
for candidate in python3 python; do
  if "$candidate" -c "import sys; sys.exit(sys.version_info < (3, 8))" >/dev/null 2>&1; then
    py=$candidate
    break
  fi
done
if [ -z "$py" ]; then
  echo "package.sh needs Python 3.8 or newer (python3 or python)" >&2
  exit 1
fi

# The stores refuse a description over 132 characters (Edge Add-ons said
# so on a manual upload); refused here first. The manifests take it from
# each language's catalog (extension/_locales), so each is checked.
for messages in "$root"/extension/_locales/*/messages.json; do
  if ! "$py" -c "import json,sys; d=json.load(open(sys.argv[1], encoding='utf-8'))['extDescription']['message']; sys.exit(len(d) > 132)" "$messages"; then
    echo "$messages: the description is over the stores' 132 characters" >&2
    exit 1
  fi
done

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
# Without the building computer's paths in the WebAssembly the stores publish
# (scripts/release-flags.sh).
(cd "$root/browser" && CARGO_BUILD_RUSTFLAGS="$(sh "$root/scripts/release-flags.sh")" \
  cargo build --release --target wasm32-unknown-unknown --locked)
rm -rf "$pkg"
wasm-bindgen --target web --no-typescript --out-dir "$pkg" \
  "${CARGO_TARGET_DIR:-$root/browser/target}/wasm32-unknown-unknown/release/dm_catcher.wasm"
# The panel's copy, which the background puts into a page's frame with
# scripting.executeScript: a script of its own (no module), its WebAssembly
# written into it, so the page's frame fetches nothing from the extension.
panel=$(mktemp -d)
wasm-bindgen --target no-modules --no-typescript --out-dir "$panel" \
  "${CARGO_TARGET_DIR:-$root/browser/target}/wasm32-unknown-unknown/release/dm_catcher.wasm"
"$py" - "$panel" "$pkg/panel.js" <<'PY'
import base64, pathlib, sys
src = pathlib.Path(sys.argv[1])
glue = (src / "dm_catcher.js").read_text()
data = base64.b64encode((src / "dm_catcher_bg.wasm").read_bytes()).decode()
pathlib.Path(sys.argv[2]).write_text(
    "// Built by scripts/package.sh: wasm-bindgen's glue and the WebAssembly it loads.\n"
    + "var __dmPanelCode;\n(() => {\n" + glue + "\n"
    + "const bytes = Uint8Array.from(atob('" + data + "'), (c) => c.charCodeAt(0));\n"
    + "__dmPanelCode = wasm_bindgen({ module_or_path: bytes }).then(() => wasm_bindgen);\n"
    # Handled now as well as when content.js awaits it: on a page whose policy
    # forbids WebAssembly it rejects first, and Chrome logged that as uncaught.
    + "__dmPanelCode.catch(() => {});\n})();\n")
PY
rm -rf "$panel"
# The background and the popup, started by two scripts written here: the extension has no
# JavaScript of its own for them (gaps row 169). The background's WebAssembly is written into its
# script and started at once (initSync), so the listeners Rust registers (web/listen.rs) are there
# on the worker's first turn, as Manifest V3 requires; and a rebuild in place cannot pair this glue
# with another build's WebAssembly, which the background once fetched apart from it (gaps row 114).
"$py" - "$pkg" <<'PY'
import base64, pathlib, sys
pkg = pathlib.Path(sys.argv[1])
data = base64.b64encode((pkg / "dm_catcher_bg.wasm").read_bytes()).decode()
(pkg / "background.js").write_text(
    "// Built by scripts/package.sh: starts the extension's background, which is Rust (browser/src)\n"
    + "// compiled to WebAssembly, with the WebAssembly written in here.\n"
    + "import * as dm from './dm_catcher.js';\n"
    + "const text = atob('" + data + "');\n"
    + "const bytes = new Uint8Array(text.length);\n"
    + "for (let i = 0; i < text.length; i += 1) bytes[i] = text.charCodeAt(i);\n"
    + "dm.initSync({ module: bytes });\n"
    + "dm.background_start();\n"
    + "// For a test harness attached to the worker; nothing in the extension uses it.\n"
    + "globalThis.dmReady = Promise.resolve(dm);\n")
(pkg / "popup.js").write_text(
    "// Built by scripts/package.sh: starts the toolbar popup, which is Rust (browser/src/popup.rs).\n"
    + "import init, { popup_start } from './dm_catcher.js';\n"
    + "init().then(() => popup_start()).catch((e) => console.error('dm:', e));\n")
PY
echo "  extension/pkg: $(du -k "$pkg/dm_catcher_bg.wasm" | cut -f1) KB of WebAssembly"

[ "${1:-}" = "--build" ] && exit 0

# ------------------------------------------------------------------ pack

# Everything the browser loads, listed rather than globbed so nothing stray
# ships; the check below compares it with what the manifests name.
files="content.js document.js popup.html pkg icons _locales"

# Built beside the folder and swapped in whole: a browser with dist/chrome
# loaded as an unpacked extension (a developer's, or a tester's) restarts the
# extension's worker whenever it likes, and one that started while the folder
# was being deleted and refilled found no file to import and stayed dead,
# every download left to the browser, until the extension was reloaded by hand.
final="$dist"
dist="$final.new"
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
  # The licence and the attribution it requires go with every copy (NOTICE).
  cp "$root/LICENSE" "$root/NOTICE" "$stage/"

  # Every file a manifest names must be in what ships, and so must the
  # module the loader imports and the WebAssembly that module fetches.
  for named in $(sed -n 's/.*"\(icons\/[^"]*\)".*/\1/p; s/.*"\(pkg\/background\.js\)".*/\1/p; s/.*"\(content\.js\)".*/\1/p; s/.*"\(document\.js\)".*/\1/p; s/.*"\(popup\.html\)".*/\1/p' "$stage/manifest.json") \
               pkg/popup.js pkg/dm_catcher.js pkg/dm_catcher_bg.wasm pkg/panel.js pkg/background.js \
               _locales/en/messages.json; do
    if [ ! -f "$stage/$named" ]; then
      echo "the $browser build is missing $named" >&2
      exit 1
    fi
  done
  if find "$stage" -name '*.js' | grep -v -e '/content.js$' -e '/document.js$' -e '/pkg/background.js$' -e '/pkg/popup.js$' -e '/pkg/dm_catcher.js$' -e '/pkg/panel.js$' >/dev/null; then
    echo "the $browser build carries JavaScript that is neither the loader nor wasm-bindgen's" >&2
    exit 1
  fi

  # Git Bash on Windows has no `zip`: there the store zips came out empty
  # (0 bytes) and nothing said so. Python's zipfile does the same job.
  out="$dist/dm-$browser-$version.zip"
  rm -f "$out"
  if command -v zip >/dev/null 2>&1; then
    (cd "$stage" && zip -qr "$out" .)
  else
    "$py" - "$stage" "$out" <<'PY'
import pathlib, sys, zipfile
stage, out = pathlib.Path(sys.argv[1]), sys.argv[2]
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    for f in sorted(p for p in stage.rglob("*") if p.is_file()):
        z.write(f, f.relative_to(stage).as_posix())
PY
  fi
  if ! "$py" -c "import sys, zipfile; sys.exit('manifest.json' not in zipfile.ZipFile(sys.argv[1]).namelist())" "$out"; then
    echo "the $browser zip ($out) is empty or has no manifest.json" >&2
    exit 1
  fi
  echo "  dist/$browser (unpacked)  dist/dm-$browser-$version.zip"
}

echo "packaging dm $version"
pack chrome manifest.json
pack firefox manifest.firefox.json

# The finished folders put where the browser looks, in two renames: the old
# ones are never half there.
rm -rf "$final.old"
[ -d "$final" ] && mv "$final" "$final.old"
mv "$dist" "$final"
rm -rf "$final.old"
dist="$final"
