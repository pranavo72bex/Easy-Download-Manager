#!/bin/sh
# Builds the CachyOS + Brave image and runs the video hand-off three ways:
# with /tmp as a default system has it, with /tmp mounted noexec (hardened and
# tuned systems), and with noexec and TMPDIR pointed at a folder programs may
# run from. Needs docker and a network. Prints what dm recorded and what its
# window says in the Status column for each.
#
#   sh scripts/linux/cachyos-brave/run.sh
#   DM=0.8.4 sh scripts/linux/cachyos-brave/run.sh    # another release
set -eu
here=$(cd "$(dirname "$0")" && pwd)
dm="${DM:-0.8.3}"
docker build --platform linux/amd64 --build-arg "DM=$dm" -t dm-cachyos-brave "$here"
for scenario in "exec" "noexec" "noexec-tmpdir"; do
  echo
  case "$scenario" in
    exec)          opts="rw,exec,nosuid"; extra="" ;;
    noexec)        opts="rw,noexec,nosuid"; extra="" ;;
    noexec-tmpdir) opts="rw,noexec,nosuid"; extra="-e TMPDIR=/root/exec-tmp" ;;
  esac
  echo "######## $scenario"
  # shellcheck disable=SC2086
  docker run --rm --platform linux/amd64 --tmpfs "/tmp:$opts,size=1g" $extra \
    -v "$here:/scripts:ro" dm-cachyos-brave \
    sh -c 'mkdir -p /root/exec-tmp && python3 /scripts/repro.py' 2>&1 \
    | grep -E "== /tmp|hands the page|\] (running|completed|failed)|what dm recorded|Status column|the file"
done
