#!/bin/sh
# Makes the crates in desktop/vendor again (gaps rows P3, A17 and X5):
# librqbit, librqbit-sha1-wrapper, gpui-pre-linux and gpui-pre-windows. Each is
# the crate published on crates.io, checked against its SHA-256, with the files
# dm's build does not use removed and dm's patch in patches/ applied. Nothing
# from the crates is run. Needs curl, tar, git and sha256sum (or shasum). The
# crates are unpacked in $DM_VENDOR_WORK, a new folder of the system's own when
# that is not set.
#
#   sh desktop/vendor/vendor.sh              all four
#   sh desktop/vendor/vendor.sh gpui-pre-linux gpui-pre-windows
#
# To move to another version: change its version and SHA-256 below (crates.io's
# API lists them, https://crates.io/api/v1/crates/<name>/versions), run this,
# mend any hunk git apply refuses, and write the patch again with `git diff`
# between the published crate (with the same files removed) and the mended
# copy, keeping the text at the patch's head. Every `dm(<tag>):` mark is a row
# of the table in desktop/vendor/README.md (`cargo xtask layers` checks it).
set -eu

LIBRQBIT_VERSION=9.1.0
LIBRQBIT_SHA256=4e35a06e481e854f69141928d067406397b3ad7a47bfdc095bc31325f44f5d13
SHA1W_SHA256=4633bfa4c77dd7153be92584947d669a0ff8f61b82bb8bf283ff5f11ddee75be
# The version dm-gpui pins (`gpui-pre =0.3.12` in desktop/Cargo.toml).
GPUI_VERSION=0.3.12
GPUI_LINUX_SHA256=54bab418d0b7c027af9495f278619b46887362b3bd6446e246c87c5b0fa167df
GPUI_WINDOWS_SHA256=7201762420222d5b5ce5bdf0056996199212a3349bfb2457a3a0238c688681f3

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

# vendor NAME VERSION SHA256 [PATH...]: fetch, check, unpack, remove each PATH
# (relative to the crate), patch and put in place.
vendor() {
    name=$1
    version=$2
    expected=$3
    shift 3
    crate="$work/$name-$version.crate"
    curl -sSfL "https://static.crates.io/crates/$name/$name-$version.crate" -o "$crate"
    got=$(sha256 "$crate")
    if [ "$got" != "$expected" ]; then
        echo "$name $version: SHA-256 $got, expected $expected" >&2
        exit 1
    fi
    dir="$work/$name-$version"
    rm -rf "$dir"
    tar -xzf "$crate" -C "$work"
    for path in "$@"; do
        rm -rf "${dir:?}/$path"
    done
    (cd "$dir" && git -c core.autocrlf=false apply --whitespace=nowarn "$here/patches/$name.patch")
    rm -rf "${here:?}/$name"
    cp -R "$dir" "$here/$name"
}

# What dm's build does not use, removed from every crate: its own lock file
# (dm's is desktop/Cargo.lock) and cargo's marks.
COMMON="Cargo.lock .cargo_vcs_info.json .cargo-ok"

# remake NAME: one crate, with what is removed from it.
remake() {
    case $1 in
        # Also not used: the web interface (the `webui` feature), test
        # torrents, examples and the manifest as written before publishing.
        librqbit | librqbit-sha1-wrapper)
            if [ "$1" = librqbit ]; then sum=$LIBRQBIT_SHA256; else sum=$SHA1W_SHA256; fi
            # shellcheck disable=SC2086
            vendor "$1" "$LIBRQBIT_VERSION" "$sum" $COMMON webui resources examples \
                Cargo.toml.orig
            ;;
        gpui-pre-linux)
            # shellcheck disable=SC2086
            vendor "$1" "$GPUI_VERSION" "$GPUI_LINUX_SHA256" $COMMON
            ;;
        gpui-pre-windows)
            # shellcheck disable=SC2086
            vendor "$1" "$GPUI_VERSION" "$GPUI_WINDOWS_SHA256" $COMMON
            ;;
        *)
            echo "vendor.sh: no crate $1 here" >&2
            exit 1
            ;;
    esac
    echo "desktop/vendor/$1: published crate, patched"
}

if [ "$#" -eq 0 ]; then
    set -- librqbit librqbit-sha1-wrapper gpui-pre-linux gpui-pre-windows
fi
for crate_name in "$@"; do
    remake "$crate_name"
done
