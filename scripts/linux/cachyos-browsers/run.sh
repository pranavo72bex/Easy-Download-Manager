#!/bin/sh
# The extension hand-off on CachyOS in Brave, Google Chrome, Microsoft Edge
# (scripts/chrome-check.py) and Firefox (scripts/firefox-check.py native),
# with dm and the extension built from this checkout. Needs docker and a
# network. Reports go to qa/reports/cachyos-<browser>.txt.
#
#   sh scripts/linux/cachyos-browsers/run.sh
#   BROWSERS="brave firefox" sh scripts/linux/cachyos-browsers/run.sh
set -u
root=$(cd "$(dirname "$0")/../../.." && pwd)
cd "$root"
mkdir -p qa/reports
docker build --platform linux/amd64 -t dm-cachyos-browsers -f scripts/linux/cachyos-browsers/Dockerfile . || exit 1
status=0
for b in ${BROWSERS:-brave chrome edge firefox}; do
  case "$b" in
    brave)   cmd="CHROME_PATH=/opt/brave.com/brave/brave CI=1 xvfb-run -a python3 -u scripts/chrome-check.py" ;;
    chrome)  cmd="CHROME_PATH=/opt/google/chrome/chrome CI=1 xvfb-run -a python3 -u scripts/chrome-check.py" ;;
    edge)    cmd="CHROME_PATH=/opt/microsoft/msedge/msedge CI=1 xvfb-run -a python3 -u scripts/chrome-check.py" ;;
    firefox) cmd="mkdir -p /opt/ff/firefox && ln -sf /usr/lib/firefox/firefox /opt/ff/firefox/firefox && CI=1 xvfb-run -a python3 -u scripts/firefox-check.py native" ;;
    *) echo "unknown browser $b"; exit 2 ;;
  esac
  echo "######## $b"
  docker run --rm --platform linux/amd64 --shm-size=1g dm-cachyos-browsers sh -c "$cmd" >"qa/reports/cachyos-$b.txt" 2>&1
  rc=$?
  grep -cE "^  PASS" "qa/reports/cachyos-$b.txt" | sed 's/^/  passed: /'
  grep -E "^  FAIL" "qa/reports/cachyos-$b.txt" | head -20
  echo "  exit: $rc"
  [ "$rc" = 0 ] || status=1
done
exit $status
