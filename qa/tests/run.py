"""Runs the QA scenarios and writes reports/qa-<run>.json and .xml (JUnit).

    python3 qa/tests/run.py                 # every scenario
    QA_SCENARIOS="pause_resume redirects" python3 qa/tests/run.py
"""
import json
import os
import pathlib
import sys
import time
import traceback
import xml.sax.saxutils as sx

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import harness  # noqa: E402
from scenarios import SCENARIOS  # noqa: E402

REPORTS = pathlib.Path(os.environ.get("QA_REPORTS") or ("/reports" if pathlib.Path("/reports").is_dir() else harness.QA / "reports"))


def main():
    wanted = (os.environ.get("QA_SCENARIOS") or " ".join(sys.argv[1:])).split()
    names = wanted or list(SCENARIOS)
    unknown = [n for n in names if n not in SCENARIOS]
    if unknown:
        sys.exit(f"unknown scenarios: {unknown}; known: {list(SCENARIOS)}")
    results = []
    for name in names:
        t0 = time.time()
        print(f"--- {name}", flush=True)
        try:
            note = SCENARIOS[name]()
            status, detail = "pass", note or ""
        except harness.Skip as e:
            status, detail = "skip", str(e)
        except harness.Fail as e:
            status, detail = "fail", str(e)
        except Exception:
            status, detail = "error", traceback.format_exc()
        took = time.time() - t0
        results.append({"name": name, "status": status, "seconds": round(took, 1), "detail": detail})
        print(f"{status.upper():5} {name} ({took:.1f}s) {detail[:400]}", flush=True)

    run = os.environ.get("QA_RUN", "1")
    REPORTS.mkdir(parents=True, exist_ok=True)
    (REPORTS / f"qa-{run}.json").write_text(json.dumps(results, indent=2))
    cases = "".join(
        f'<testcase classname="qa" name="{r["name"]}" time="{r["seconds"]}">'
        + (f'<failure message={sx.quoteattr(r["detail"][:200])}>{sx.escape(r["detail"])}</failure>' if r["status"] in ("fail", "error") else "")
        + ('<skipped/>' if r["status"] == "skip" else "")
        + "</testcase>" for r in results)
    bad = sum(r["status"] in ("fail", "error") for r in results)
    skipped = sum(r["status"] == "skip" for r in results)
    (REPORTS / f"qa-{run}.xml").write_text(
        f'<?xml version="1.0"?><testsuite name="dm-qa" tests="{len(results)}" failures="{bad}" skipped="{skipped}">{cases}</testsuite>\n')
    print(f"=== {len(results) - bad - skipped} passed, {bad} failed, {skipped} skipped", flush=True)
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
