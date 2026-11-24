#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""One question to a free AI model, for the bots in .github/workflows/ai-*.yml.

    python3 scripts/ai-ask.py ask [--json] < PROMPT     # the answer on stdout
    python3 scripts/ai-ask.py pick gemini|kilo           # a model that answers now

Free models come and go, so none is fixed here: Gemini's are listed from Google AI
Studio's own catalogue (GEMINI_API_KEY; a model named by GEMINI_MODEL first), Kilo
Code's free ones from its gateway (no key; KILO_MODEL first), and each is tried in turn
until one answers. A model out of quota, retired or failing is passed over. `--json`
asks for JSON and refuses an answer that is not. Exits 1 when no model answered.
Standard library only.
"""

import json
import os
import re
import sys
import urllib.error
import urllib.request

GEMINI = "https://generativelanguage.googleapis.com/v1beta"
KILO = "https://api.kilo.ai/api/gateway"
TIMEOUT = 180


def call(url, body=None, headers=None):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, headers={"Content-Type": "application/json", **(headers or {})})
    with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
        return json.loads(r.read().decode("utf-8", "replace"))


def version_of(name):
    """gemini-3.8-flash -> (3, 8); for the newest first."""
    m = re.search(r"gemini-(\d+)(?:\.(\d+))?", name)
    return (int(m.group(1)), int(m.group(2) or 0)) if m else (0, 0)


def gemini_models():
    key = os.environ.get("GEMINI_API_KEY", "")
    if not key:
        return []
    named = [os.environ["GEMINI_MODEL"]] if os.environ.get("GEMINI_MODEL") else []
    try:
        listed = call(f"{GEMINI}/models?pageSize=1000", headers={"x-goog-api-key": key}).get("models", [])
    except (urllib.error.URLError, OSError, ValueError) as e:
        print(f"ai-ask: Gemini's catalogue: {e}", file=sys.stderr)
        listed = []
    found = []
    for m in listed:
        name = m.get("name", "").removeprefix("models/")
        if "generateContent" not in m.get("supportedGenerationMethods", []):
            continue
        if not name.startswith("gemini-") or re.search(r"image|tts|audio|live|embed|vision|robotics|computer", name):
            continue
        found.append(name)
    # Flash before Pro (Pro's free quota is the smallest), the newest first, stable before preview.
    found.sort(key=lambda n: ("flash" not in n, "lite" in n, -version_of(n)[0], -version_of(n)[1], "preview" in n or "exp" in n))
    return named + [n for n in found if n not in named]


def kilo_models():
    named = [os.environ["KILO_MODEL"]] if os.environ.get("KILO_MODEL") else []
    try:
        listed = [m.get("id", "") for m in call(f"{KILO}/models").get("data", [])]
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


def ask_gemini(model, prompt, as_json):
    config = {"responseMimeType": "application/json"} if as_json else {}
    r = call(
        f"{GEMINI}/models/{model}:generateContent",
        {"contents": [{"parts": [{"text": prompt}]}], "generationConfig": config},
        {"x-goog-api-key": os.environ["GEMINI_API_KEY"]},
    )
    parts = r["candidates"][0]["content"]["parts"]
    return "".join(p.get("text", "") for p in parts)


def ask_kilo(model, prompt, as_json):
    body = {"model": model, "messages": [{"role": "user", "content": prompt}]}
    if as_json:
        body["response_format"] = {"type": "json_object"}
    r = call(f"{KILO}/chat/completions", body)
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


def candidates(which=None):
    if which in (None, "gemini"):
        for m in gemini_models():
            yield "gemini", m
    if which in (None, "kilo"):
        for m in kilo_models():
            yield "kilo", m


def answer(provider, model, prompt, as_json):
    ask = ask_gemini if provider == "gemini" else ask_kilo
    text = ask(model, prompt, as_json)
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
    if len(argv) == 3 and argv[1] == "pick" and argv[2] in ("gemini", "kilo"):
        for provider, model in candidates(argv[2]):
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
