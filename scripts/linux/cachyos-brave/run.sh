#!/bin/sh
# Builds the CachyOS + Brave image and runs the video hand-off three ways:
# with /tmp as a default system has it, with /tmp mounted noexec (hardened and
# tuned systems), with noexec and TMPDIR pointed at a folder programs may
# run from, and with noexec and a single-file yt-dlp of the person's own. Needs docker and a network. Prints what dm recorded and what its
# window says in the Status column for each.
#
#   sh scripts/linux/cachyos-brave/run.sh
#   DM=0.8.4 sh scripts/linux/cachyos-brave/run.sh    # another release
#   DM_APPIMAGE=path/dm_0.8.4_x86_64.AppImage sh scripts/linux/cachyos-brave/run.sh   # not yet released
set -eu
here=$(cd "$(dirname "$0")" && pwd)
dm="${DM:-0.8.3}"
mkdir -p "$here/local"
touch "$here/local/.keep"
rm -f "$here/local/dm.AppImage"
if [ -n "${DM_APPIMAGE:-}" ]; then cp "$DM_APPIMAGE" "$here/local/dm.AppImage"; fi
docker build --platform linux/amd64 --build-arg "DM=$dm" -t dm-cachyos-brave "$here"
for scenario in ${SCENARIOS:-"exec" "noexec" "noexec-tmpdir" "noexec-single-file"}; do
  echo
  case "$scenario" in
    exec)          opts="rw,exec,nosuid"; extra="" ;;
    noexec)        opts="rw,noexec,nosuid"; extra="" ;;
    noexec-tmpdir) opts="rw,noexec,nosuid"; extra="-e TMPDIR=/root/exec-tmp" ;;
    # yt-dlp as a person installs it: the one file, which unpacks itself into /tmp.
    noexec-single-file) opts="rw,noexec,nosuid"; extra="-e DM_YTDLP=/root/yt-dlp-one" ;;
  esac
  echo "######## $scenario"
  # shellcheck disable=SC2086
  docker run --rm --platform linux/amd64 --tmpfs "/tmp:$opts,size=1g" $extra \
    -v "$here:/scripts:ro" dm-cachyos-brave \
    sh -c 'mkdir -p /root/exec-tmp && { [ -z "${DM_YTDLP:-}" ] || { curl -fsSL -o "$DM_YTDLP" https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux && chmod +x "$DM_YTDLP"; }; } && python3 /scripts/repro.py' 2>&1 \
    | grep -E "== /tmp|hands the page|\] (running|completed|failed)|what dm recorded|Status column|the file"
done
