#!/bin/sh
# Builds the .rpm from a release binary with nfpm, pinned and checked by its
# published SHA-256 before it runs:
#
#   sh scripts/linux/nfpm.sh BINARY VERSION OUTDIR
#
# Used by the release workflow and by scripts/rpm-check.sh. The package's
# layout is desktop/packaging/nfpm.yaml.
set -eu
# Absolute, since the script changes directory below.
BINARY=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
VERSION=$2
mkdir -p "$3"
OUT=$(cd "$3" && pwd)
NFPM_VERSION=2.47.0
case "$(uname -m)" in
  x86_64) asset=x86_64 arch=amd64 sum=0660ca602b2d2d2ae4781a06c692b3eeb9d437ffea05b831d76e41f4a3188783 ;;
  aarch64 | arm64) asset=arm64 arch=arm64 sum=1c0f5f2999b9a974bfb04fdb0cc3306096de530ac5dbb25d739cc5f5219c919c ;;
  *) echo "no nfpm for $(uname -m)" >&2; exit 1 ;;
esac

TOOLS=${NFPM_DIR:-$(mktemp -d)}
if [ ! -x "$TOOLS/nfpm" ]; then
  curl -fsSL -o "$TOOLS/nfpm.tar.gz" \
    "https://github.com/goreleaser/nfpm/releases/download/v$NFPM_VERSION/nfpm_${NFPM_VERSION}_Linux_$asset.tar.gz"
  echo "$sum  $TOOLS/nfpm.tar.gz" | sha256sum -c -
  tar -xzf "$TOOLS/nfpm.tar.gz" -C "$TOOLS" nfpm
fi

cd "$(dirname "$0")/../.."
# nfpm reads the binary from where nfpm.yaml says; it does not expand
# variables in a source path.
mkdir -p desktop/target/release
if ! cmp -s "$BINARY" desktop/target/release/dm-desktop; then
  cp "$BINARY" desktop/target/release/dm-desktop
fi
mkdir -p "$OUT"
VERSION=$VERSION ARCH=$arch "$TOOLS/nfpm" package -f desktop/packaging/nfpm.yaml -p rpm -t "$OUT/"
ls -la "$OUT"/*.rpm
