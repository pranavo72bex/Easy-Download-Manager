#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""Whether the free-model bots are working: `python3 scripts/ai-health.py` (gh, logged in).

Run at the start of every session (docs/gaps.md, "Every session first: are the bots
working?"). It only reads (GitHub's runs, issues, pull requests and commits, and one question
to Kilo Code's free gateway) and prints one line per check:

    OK    working as it should
    WARN  worth a look, the bots still work
    FAIL  the bots are stopped or broken: fix it before other work (what to do is said)

Exits 1 when any check FAILs, 0 otherwise. What each FAIL means and how to fix it is in
docs/gaps.md under the same heading.
"""

import json
import subprocess
import sys
from datetime import datetime, timedelta, timezone

NOW = datetime.now(timezone.utc)
FAILS = []
WARNS = []
BOT_WORKFLOWS = ["AI worker", "AI triage", "AI fix", "AI tests", "AI merge", "Bug report"]


def gh(*args):
    r = subprocess.run(["gh", *args], capture_output=True, text=True)
    if r.returncode != 0:
        return None
    try:
        return json.loads(r.stdout) if r.stdout.strip() else None
    except ValueError:
        return r.stdout


def when(stamp):
    try:
        return datetime.fromisoformat(stamp.replace("Z", "+00:00"))
    except (AttributeError, ValueError):
        return None


def ago(stamp):
    t = when(stamp)
    if not t:
        return "never"
    h = (NOW - t).total_seconds() / 3600
    return f"{h:.0f} h ago" if h < 48 else f"{h / 24:.0f} days ago"


def ok(msg):
    print(f"OK    {msg}")


def warn(msg, todo=""):
    WARNS.append(msg)
    print(f"WARN  {msg}" + (f"\n        → {todo}" if todo else ""))


def fail(msg, todo):
    FAILS.append(msg)
    print(f"FAIL  {msg}\n        → {todo}")


def runs(workflow, limit=30):
    return gh("run", "list", "--workflow", workflow, "--limit", str(limit),
              "--json", "databaseId,status,conclusion,createdAt,url,event,headBranch") or []


def main():
    if gh("auth", "status") is None and subprocess.run(["gh", "auth", "status"], capture_output=True).returncode != 0:
        print("FAIL  gh is not logged in: run `gh auth login` first")
        return 1
    repo = (gh("repo", "view", "--json", "nameWithOwner") or {}).get("nameWithOwner", "")
    print(f"The bots of {repo}, {NOW:%Y-%m-%d %H:%M} UTC\n")

    # 1. The workflows are on (GitHub turns a quiet public repository's schedules off at 60 days).
    states = {w["name"]: w["state"] for w in (gh("workflow", "list", "--all", "--json", "name,state") or [])}
    off = [f"{n} ({states.get(n, 'missing')})" for n in BOT_WORKFLOWS if states.get(n) != "active"]
    if off:
        fail("bot workflows not active: " + ", ".join(off),
             "gh workflow enable <name> for each (disabled_inactivity: also check .github/keepalive is written)")
    else:
        ok("every bot workflow is active")

    # 2. The free Kilo key is there. Kilo's free models answer without one, but the keyless
    # quota runs out part way through a task here ("Sign up for free to continue").
    secrets = {s["name"] for s in (gh("secret", "list", "--json", "name") or [])}
    if "KILO_API_KEY" in secrets:
        ok("KILO_API_KEY is set")
    else:
        warn("KILO_API_KEY is not set: the keyless quota runs out before a task's change is done",
             "a free account at app.kilo.ai (no card), then gh secret set KILO_API_KEY")

    # 3. A free model answers now (Kilo needs no key; scripts/ai-ask.py finds one that answers).
    r = subprocess.run([sys.executable, "-I", "scripts/ai-ask.py", "pick", "kilo"], capture_output=True, text=True)
    if r.returncode == 0 and r.stdout.strip():
        ok(f"a free model answers: kilo {r.stdout.strip()}")
    else:
        warn("no Kilo free model answered from here just now (a passing outage, or the gateway changed)",
             "python3 scripts/ai-ask.py pick kilo; if it keeps failing, check api.kilo.ai's free models and scripts/ai-ask.py")

    # 4. The queue runs (every two hours, and after each task).
    w = [x for x in runs("ai-worker.yml", 20) if x["status"] == "completed"]
    last_ok = next((x for x in w if x["conclusion"] == "success"), None)
    if not w:
        fail("the worker (AI worker) has never run", "gh workflow run ai-worker.yml, then read its log")
    elif not last_ok or NOW - when(last_ok["createdAt"]) > timedelta(hours=5):
        fail(f"the worker has not succeeded in 5 hours (last success {ago(last_ok['createdAt']) if last_ok else 'never'})",
             f"read the last run's log ({w[0]['url']}), fix scripts/ai-worker.sh, then gh workflow run ai-worker.yml")
    else:
        ok(f"the worker ran {ago(last_ok['createdAt'])}")

    # 5. The guardrails hold on main (the CI job that runs scripts/ai-selftest.sh).
    ci = [x for x in runs("ci.yml", 20) if x["status"] == "completed" and x["headBranch"] == "main"]
    if ci:
        jobs = (gh("run", "view", str(ci[0]["databaseId"]), "--json", "jobs") or {}).get("jobs", [])
        g = next((j for j in jobs if j["name"].startswith("The bots")), None)
        if g is None:
            warn("main's last CI run has no guardrail job", "check .github/workflows/ci.yml's bots-skills job")
        elif g["conclusion"] == "success":
            ok("the guardrails hold (CI's skills check and scripts/ai-selftest.sh, on main)")
        else:
            fail(f"the guardrail job failed on main ({g['conclusion']}): a guardrail or a skill page is broken",
                 f"read {ci[0]['url']}; run python3 scripts/ai-skills-check.py and bash scripts/ai-selftest.sh, fix, push")

    # 6. The bots' own jobs, as they run now: a run that FAILS is the pipeline broken (a model
    #    that only could not make a passing change ends its run in success, and says so on
    #    the issue). The last two finished runs both failed: broken; the last one: a look.
    for wf, name in (("ai-fix.yml", "AI fix"), ("ai-tests.yml", "AI tests"), ("ai-merge.yml", "AI merge"),
                     ("ai-triage.yml", "AI triage")):
        done = [x for x in runs(wf, 30) if x["status"] == "completed"
                and x["conclusion"] not in ("skipped", "cancelled")]
        bad = ("failure", "startup_failure", "timed_out")
        if len(done) >= 2 and done[0]["conclusion"] in bad and done[1]["conclusion"] in bad:
            fail(f"{name}: its last two runs failed (the pipeline, not the model)",
                 f"read {done[0]['url']}: the step that failed says why; fix the workflow or script, "
                 "run bash scripts/ai-selftest.sh, push")
        elif done and done[0]["conclusion"] in bad:
            warn(f"{name}: its last run failed ({ago(done[0]['createdAt'])})", f"look at {done[0]['url']}")
        elif done:
            ok(f"{name}: its last run worked ({ago(done[0]['createdAt'])})")

    # 7. Work is moving: a task taken in the last day (or nothing to take).
    fixes = [x for x in runs("ai-fix.yml", 30) if NOW - when(x["createdAt"]) < timedelta(hours=26)]
    tests = [x for x in runs("ai-tests.yml", 10) if NOW - when(x["createdAt"]) < timedelta(hours=26)]
    if not fixes and not tests:
        fail("no AI fix and no AI tests run in 26 hours: the queue is stuck or starved",
             "gh workflow run ai-worker.yml and read its log (labels, the busy check, the picker); "
             "python3 scripts/ai-improve-pick.py should name a row")
    else:
        ok(f"work taken in 26 h: {len(fixes)} AI fix, {len(tests)} AI tests run(s)")

    # 8. Stuck: a task marked working with nothing running, an automerge pull request a day old.
    running = any(x["status"] != "completed" for x in runs("ai-fix.yml", 10) + runs("ai-tests.yml", 5))
    stale = gh("issue", "list", "--state", "open", "--label", "bot:working", "--json", "number,updatedAt") or []
    stale = [i for i in stale if NOW - when(i["updatedAt"]) > timedelta(hours=8)]
    if stale and not running:
        warn("labelled bot:working with nothing running: " + ", ".join(f"#{i['number']}" for i in stale),
             "the worker puts them back in the queue on its next round; if it does not, gh workflow run ai-worker.yml")
    prs = gh("pr", "list", "--state", "open", "--label", "bot:automerge", "--json", "number,createdAt,labels,url") or []
    held = [p for p in prs if any(l["name"] == "bot:needs-maintainer" for l in p["labels"])]
    old = [p for p in prs if p not in held and NOW - when(p["createdAt"]) > timedelta(hours=24)]
    if held:
        warn("bot pull requests waiting for a person (bot:needs-maintainer): "
             + ", ".join(f"#{p['number']}" for p in held), "read each; merge or close it")
    if old:
        fail("bot pull requests open over a day without merging: " + ", ".join(f"#{p['number']}" for p in old),
             "gh workflow run ai-merge.yml -f pr=N and read its log: the checks, the review, or ai-merge.sh itself")
    elif prs and not held:
        ok(f"{len(prs)} bot pull request(s) on their way")

    # 9. The bots deliver: a pull request of theirs merged in the last week.
    merged = gh("pr", "list", "--state", "merged", "--search", "head:bot/", "--limit", "50",
                "--json", "number,mergedAt,title") or []
    week = [p for p in merged if NOW - when(p["mergedAt"]) < timedelta(days=7)]
    if week:
        ok(f"{len(week)} bot pull request(s) merged in 7 days (last: #{week[0]['number']} {week[0]['title'][:60]})")
    elif merged:
        warn(f"no bot pull request merged in 7 days (the last {ago(merged[0]['mergedAt'])})",
             "read the last AI fix and AI tests issues' comments: what failed most; mark small rows · bot in docs/gaps.md")
    else:
        warn("no bot pull request has ever merged",
             "expected in the first days; after a week, read the AI fix and AI tests runs' reports")

    # 10. The bots learn: lessons committed in the last week.
    since = (NOW - timedelta(days=7)).strftime("%Y-%m-%dT%H:%M:%SZ")
    learned = 0
    for path in ("docs/ai-notes.md", "docs/ai-skills"):
        c = gh("api", f"repos/{repo}/commits?path={path}&since={since}&per_page=100")
        learned += len([x for x in (c or []) if "a lesson from" in x.get("commit", {}).get("message", "")])
    if learned:
        ok(f"{learned} lesson(s) learned in 7 days (docs/ai-notes.md, docs/ai-skills/)")
    else:
        warn("no lesson learned in 7 days", "lessons come after CI-failure, improvement and nightly-test runs; "
             "read the 'What this run teaches' step of the last such run")

    # 11. Main itself (the bots work around a red main, but someone should know).
    for wf, name in (("desktop.yml", "Desktop"), ("ci.yml", "CI"), ("android.yml", "Android")):
        done = [x for x in runs(wf, 15) if x["status"] == "completed" and x["headBranch"] == "main"
                and x["conclusion"] in ("success", "failure")]
        if done and done[0]["conclusion"] == "failure":
            warn(f"{name} is red on main ({ago(done[0]['createdAt'])})",
                 f"{done[0]['url']}; its issue (CI failure on main) is the bots' first task")
    issues = gh("issue", "list", "--state", "open", "--label", "bot-fix", "--json", "number,title,labels,createdAt") or []
    for i in issues:
        names = {l["name"] for l in i["labels"]}
        if "bot:no-fix" in names:
            warn(f"#{i['number']} {i['title']}: no model could fix it yet (tried again weekly)",
                 "a session can fix it by hand; the bots stop when main goes green")

    print()
    if FAILS:
        print(f"{len(FAILS)} FAIL, {len(WARNS)} WARN: fix the FAILs before other work (docs/gaps.md says how).")
        return 1
    print(f"The bots are working ({len(WARNS)} WARN).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
