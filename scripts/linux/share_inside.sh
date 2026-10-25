#!/bin/sh
# Runs inside the container: shares a file on the container's own network
# address (not the loopback) and fetches it back the way a neighbour's
# download manager does: the whole file, a range, eight ranges at once, a
# wrong link, a stopped share.
set -e
cd /work/desktop
command -v curl >/dev/null || { apt-get update -qq >/dev/null && apt-get install -y -qq curl >/dev/null; }

echo "== build the share"
cargo test -p dm-core --test lanshare --locked --no-run 2>&1 | tail -1

SIZE=64000000
head -c $SIZE /dev/urandom > /tmp/shared.bin
WANT=$(sha256sum /tmp/shared.bin | cut -c1-64)

echo "== share it"
DM_SHARE_FILE=/tmp/shared.bin DM_SHARE_SECS=90 \
  cargo test -p dm-core --test lanshare --locked -- --ignored --nocapture lend > /tmp/lend.log 2>&1 &
i=0
until grep -q '^LINK ' /tmp/lend.log 2>/dev/null; do
  i=$((i + 1)); [ $i -gt 120 ] && { echo "no link"; cat /tmp/lend.log; exit 1; }
  sleep 1
done
URL=$(grep '^LINK ' /tmp/lend.log | cut -d' ' -f2)
echo "link: $URL"
case "$URL" in http://127.*) echo "FAIL the link is the loopback"; exit 1;; esac

fail=0
check() { if [ "$2" = "$3" ]; then echo "  ok    $1"; else echo "  FAIL  $1 (got '$2', wanted '$3')"; fail=1; fi; }

echo "== what a neighbour meets"
check "HEAD says 200" "$(curl -sSI "$URL" | head -1 | tr -d '\r' | cut -d' ' -f2)" 200
check "HEAD gives the size" "$(curl -sSI "$URL" | tr -d '\r' | awk -F': ' 'tolower($1)=="content-length"{print $2}')" $SIZE
check "ranges are offered" "$(curl -sSI "$URL" | tr -d '\r' | awk -F': ' 'tolower($1)=="accept-ranges"{print $2}')" bytes
check "a range answers 206" "$(curl -sS -o /dev/null -w '%{http_code}' -r 10-19 "$URL")" 206
check "and is those bytes" "$(curl -sS -r 10-19 "$URL" | sha256sum | cut -c1-64)" "$(dd if=/tmp/shared.bin bs=1 skip=10 count=10 2>/dev/null | sha256sum | cut -c1-64)"
check "past the end is 416" "$(curl -sS -o /dev/null -w '%{http_code}' -r $SIZE- "$URL")" 416
BASE=${URL%/*}; ROOT=${BASE%/*}
check "a wrong token is 404" "$(curl -sS -o /dev/null -w '%{http_code}' "$ROOT/00000000000000000000000000000000/shared.bin")" 404
check "the root is 404" "$(curl -sS -o /dev/null -w '%{http_code}' "$ROOT/")" 404
check "a POST is 405" "$(curl -sS -o /dev/null -w '%{http_code}' -X POST "$URL")" 405

echo "== the whole file"
s=$(date +%s); curl -sS -o /tmp/one.bin "$URL"; e=$(date +%s)
check "one connection: byte for byte" "$(sha256sum /tmp/one.bin | cut -c1-64)" "$WANT"
echo "  ($((e - s)) s on one connection)"

echo "== eight connections at once"
P=$((SIZE / 8)); pids=""; s=$(date +%s)
for n in 0 1 2 3 4 5 6 7; do
  a=$((n * P)); b=$((a + P - 1)); [ $n = 7 ] && b=$((SIZE - 1))
  curl -sS -r $a-$b -o /tmp/p$n "$URL" &
  pids="$pids $!"
done
wait $pids; e=$(date +%s)
check "eight ranges joined: byte for byte" "$(cat /tmp/p0 /tmp/p1 /tmp/p2 /tmp/p3 /tmp/p4 /tmp/p5 /tmp/p6 /tmp/p7 | sha256sum | cut -c1-64)" "$WANT"
echo "  ($((e - s)) s on eight connections)"

echo "== it ends"
wait
grep '^SENT' /tmp/lend.log || true
after=$(curl -sS --max-time 5 -o /dev/null -w '%{http_code}' "$URL" 2>/dev/null || true)
check "after the share's time, nothing answers" "$after" 000
[ $fail = 0 ] && echo "the share does what it says" || { echo "SOME CHECKS FAILED"; exit 1; }
