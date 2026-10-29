#!/bin/sh
# The extension hand-off, end to end: Chromium with the unpacked extension
# (dist/chrome, built in the image) catches downloads and hands them to dm over
# native messaging, keeps them when dm is not running, and follows the app's
# catch rules (scripts/chrome-check.py, run unchanged). Writes
# /reports/browser-<run>.txt and .xml.
run=${QA_RUN:-1}
out=/reports/browser-$run
CI=1 xvfb-run -a python3 -u scripts/chrome-check.py >"$out.txt" 2>&1
status=$?
cat "$out.txt"
python3 - "$out" "$status" <<'PY'
import sys, xml.sax.saxutils as sx
out, status = sys.argv[1], int(sys.argv[2])
text = open(out + ".txt", errors="replace").read()
fail = f'<failure message="chrome-check exited {status}">{sx.escape(text[-4000:])}</failure>' if status else ""
open(out + ".xml", "w").write(f'<?xml version="1.0"?><testsuite name="dm-qa-browser" tests="1" failures="{1 if status else 0}"><testcase classname="qa" name="extension_handoff">{fail}</testcase></testsuite>\n')
PY
exit $status
