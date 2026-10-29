#!/bin/sh
# Stress run of the extension in Brave on CachyOS against real YouTube, for the
# old (use_dynamic_url) and the fixed manifest. ROUNDS, SEED choose the run.
#   ROUNDS=60 SEED=3 sh scripts/linux/cachyos-brave/stress.sh
here=$(cd "$(dirname "$0")" && pwd); root=$(cd "$here/../../.." && pwd)
for v in ${VARIANTS:-old fixed}; do
  docker run --rm --platform linux/amd64 -e VARIANT=$v -e ROUNDS=${ROUNDS:-40} -e SEED=${SEED:-1} \
    -v "$here:/scripts:ro" -v "$root/extension:/ext:ro" dm-cachyos-brave python3 /scripts/stress.py
done
