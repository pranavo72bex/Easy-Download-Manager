#!/bin/sh
# Builds the QA stack and runs it: every scenario in qa/tests against dm built
# from this checkout, and the extension hand-off in Chromium. Reports go to
# qa/reports (JSON and JUnit per run, and each container's log).
#
#   sh qa/run.sh                    # one run
#   QA_RUNS=3 sh qa/run.sh          # three in a row; stops at the first failing run
#   QA_SCENARIOS="pause_resume" sh qa/run.sh
set -u
cd "$(dirname "$0")"
mkdir -p reports
runs=${QA_RUNS:-1}
docker compose build || exit 1
status=0
i=1
while [ "$i" -le "$runs" ]; do
  echo "######## run $i of $runs"
  export QA_RUN=$i
  docker compose up -d --wait origin toxiproxy mockvideo >"reports/compose-$i.log" 2>&1 || { cat "reports/compose-$i.log"; exit 1; }
  docker compose run --rm -e QA_RUN="$i" -e QA_SCENARIOS="${QA_SCENARIOS:-}" app >"reports/app-$i.log" 2>&1
  app=$?
  docker compose run --rm -e QA_RUN="$i" browser >"reports/browser-$i.log" 2>&1
  browser=$?
  for s in origin toxiproxy mockvideo; do docker compose logs --no-color "$s" >"reports/$s-$i.log" 2>&1; done
  grep -E "^(PASS|FAIL|ERROR|SKIP)|^===" "reports/app-$i.log"
  grep -E "^  |^===" "reports/browser-$i.log" | grep -iE "NO|fail|===" | head -20
  echo "app exit: $app  browser exit: $browser"
  docker compose down --remove-orphans >/dev/null 2>&1
  if [ "$app" != 0 ] || [ "$browser" != 0 ]; then status=1; break; fi
  i=$((i + 1))
done
exit $status
