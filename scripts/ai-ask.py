#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""One question to a free AI model, for the bots in .github/workflows/ai-*.yml.

    python3 scripts/ai-ask.py ask [--json] < PROMPT     # the answer on stdout
    python3 scripts/ai-ask.py pick kilo                  # a model that answers now

Free models come and go, so none is fixed here: Kilo Code's free ones are listed from
its gateway (KILO_MODEL first), and each is tried in turn until one answers. They need
no key; KILO_API_KEY (a free Kilo account) is sent when set, as the keyless quota is
small. A model out of quota, retired or failing is passed over. `--json`
asks for JSON and refuses an answer that is not. Exits 1 when no model answered.
Standard library only.
"""

import json
import os
import re
import sys
import urllib.error
import urllib.request

KILO = "https://api.kilo.ai/api/gateway"
TIMEOUT = 180


def call(url, body=None, headers=None):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, headers={"Content-Type": "application/json", **(headers or {})})
    with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
        return json.loads(r.read().decode("utf-8", "replace"))


def kilo_headers():
    key = os.environ.get("KILO_API_KEY", "")
    return {"Authorization": f"Bearer {key}"} if key else {}


def kilo_models():
    named = [os.environ["KILO_MODEL"]] if os.environ.get("KILO_MODEL") else []
    try:
        listed = [m.get("id", "") for m in call(f"{KILO}/models", headers=kilo_headers()).get("data", [])]
    except (urllib.error.URLError, OSError, ValueError) as e:
        print(f"ai-ask: Kilo's catalogue: {e}", file=sys.stderr)
        listed = []
    free = [m for m in listed if m.endswith(":free") or m.endswith("/free")]
    # The largest first: a coding agent needs the strongest that is free.
    def rank(m):
        size = re.search(r"(\d+)b", m)
        return (0 if "ultra" in m else 1, -(int(size.group(1)) if size else 0), m == "kilo-auto/free")
    free.sort(key=rank)
    return named + [m for m in free if m not in named]


def ask_kilo(model, prompt, as_json):
    body = {"model": model, "messages": [{"role": "user", "content": prompt}]}
    if as_json:
        body["response_format"] = {"type": "json_object"}
    r = call(f"{KILO}/chat/completions", body, kilo_headers())
    return r["choices"][0]["message"].get("content") or ""


def as_json_text(text):
    """The JSON in an answer (a model may wrap it in a fence), or None."""
    text = text.strip()
    fenced = re.search(r"```(?:json)?\s*(.*?)```", text, re.S)
    if fenced:
        text = fenced.group(1).strip()
    start, end = text.find("{"), text.rfind("}")
    if start < 0 or end < start:
        return None
    try:
        return json.dumps(json.loads(text[start : end + 1]))
    except ValueError:
        return None


def candidates():
    for m in kilo_models():
        yield "kilo", m


def answer(provider, model, prompt, as_json):
    text = ask_kilo(model, prompt, as_json)
    if as_json:
        text = as_json_text(text)
        if text is None:
            raise ValueError("the answer was not JSON")
    if not text.strip():
        raise ValueError("an empty answer")
    return text


def main(argv):
    if len(argv) >= 2 and argv[1] == "ask":
        as_json = "--json" in argv
        prompt = sys.stdin.read()
        for provider, model in candidates():
            try:
                text = answer(provider, model, prompt, as_json)
            except (urllib.error.URLError, OSError, ValueError, KeyError, IndexError) as e:
                print(f"ai-ask: {provider} {model}: {e}", file=sys.stderr)
                continue
            print(f"ai-ask: answered by {provider} {model}", file=sys.stderr)
            print(text)
            return 0
        print("ai-ask: no free model answered", file=sys.stderr)
        return 1
    if len(argv) == 3 and argv[1] == "pick" and argv[2] == "kilo":
        for provider, model in candidates():
            try:
                answer(provider, model, "Reply with the word OK.", False)
            except (urllib.error.URLError, OSError, ValueError, KeyError, IndexError) as e:
                print(f"ai-ask: {provider} {model}: {e}", file=sys.stderr)
                continue
            print(model)
            return 0
        return 1
    sys.exit(__doc__)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
