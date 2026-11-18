#!/bin/sh
# Makes desktop/vendor/librqbit and desktop/vendor/librqbit-sha1-wrapper again
# (gaps row P3): each is the crate published on crates.io, checked against its
# SHA-256, with the files dm's build does not use removed and dm's patch in
# patches/ applied. Nothing from the crates is run. Needs curl, tar, git and
# sha256sum (or shasum). The crates are unpacked in $DM_VENDOR_WORK, a new
# folder of the system's own when that is not set.
#
#   sh desktop/vendor/vendor.sh
#
# To move to another version: change VERSION and the SHA-256s (crates.io's
# API lists them, https://crates.io/api/v1/crates/librqbit/versions), run
# this, mend any hunk git apply refuses, and write the patch again with
# `git diff` between the published crate (with the same files removed) and the
# mended copy.
set -eu

VERSION=9.1.0
LIBRQBIT_SHA256=4e35a06e481e854f69141928d067406397b3ad7a47bfdc095bc31325f44f5d13
SHA1W_SHA256=4633bfa4c77dd7153be92584947d669a0ff8f61b82bb8bf283ff5f11ddee75be

here=$(cd "$(dirname "$0")" && pwd)
if [ -n "${DM_VENDOR_WORK:-}" ]; then
    mkdir -p "$DM_VENDOR_WORK"
    work=$(cd "$DM_VENDOR_WORK" && pwd)
else
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
fi

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

# vendor NAME SHA256: fetch, check, unpack, trim, patch and put in place.
vendor() {
    name=$1
    crate="$work/$name-$VERSION.crate"
    curl -sSfL "https://static.crates.io/crates/$name/$name-$VERSION.crate" -o "$crate"
    got=$(sha256 "$crate")
    if [ "$got" != "$2" ]; then
        echo "$name $VERSION: SHA-256 $got, expected $2" >&2
        exit 1
    fi
    rm -rf "${work:?}/$name-$VERSION"
    tar -xzf "$crate" -C "$work"
    dir="$work/$name-$VERSION"
    # Not used by dm's build: the web interface (the `webui` feature), test
    # torrents, examples, the crate's own lock file (dm's is
    # desktop/Cargo.lock), the manifest as written before publishing and
    # cargo's marks.
    rm -rf "$dir/webui" "$dir/resources" "$dir/examples" "$dir/Cargo.lock" \
        "$dir/Cargo.toml.orig" "$dir/.cargo_vcs_info.json" "$dir/.cargo-ok"
    (cd "$dir" && git -c core.autocrlf=false apply --whitespace=nowarn "$here/patches/$name.patch")
    rm -rf "${here:?}/$name"
    cp -R "$dir" "$here/$name"
}

vendor librqbit "$LIBRQBIT_SHA256"
vendor librqbit-sha1-wrapper "$SHA1W_SHA256"
echo "desktop/vendor: librqbit and librqbit-sha1-wrapper $VERSION, patched"
