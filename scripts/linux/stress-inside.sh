#!/bin/sh
# Runs inside a container with no network (only loopback): the workspace
# tests, then the stress phases that need nothing but local servers.
set -u
cd /work/desktop || exit 1
# Four jobs: more links at once ran Docker's 8 GB out of memory (ld killed, 2026-11-06).
export CARGO_TARGET_DIR=/target CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
echo "== cargo test --workspace"
# On the container's own disk: /stress is the host's folder, which on a Mac ignores
# case, and a case-only rename test then judged a disk no Linux has (2026-11-06).
[ -n "${DM_ONLY:-}" ] || TMPDIR=/tmp cargo test --workspace --no-fail-fast 2>&1 | grep -E "^test result|FAILED|panicked|error(\[|:)|failures:" | sort | uniq -c | sort -rn | head -40
echo "== build dm-desktop (release)"
cargo build --release -p dm-desktop 2>&1 | tail -2
export DM_BINARY=/target/release/dm-desktop
for phase in ${DM_ONLY:-throughput many_small concurrency flaky crash pause_storm names hostile control bigqueue \
             expired urls limits outage speedlimit hls leak lossy manifests scheduler watch mcp mirrors \
             holds hidden details}; do
  xvfb-run -a python3 ../scripts/stress-check.py "$phase" 2>&1 | grep -E "^== |^  (ok|FAIL)|MEASURE|FINDING|Traceback|Error" | head -40
done
[ -n "${DM_ONLY:-}" ] && exit 0
for phase in socks proxy ftp torrent; do
  python3 ../scripts/stress-protocols.py "$phase" 2>&1 | grep -E "^== |MEASURE|FINDING|Traceback|Error" | head -30
done
echo "== done"
