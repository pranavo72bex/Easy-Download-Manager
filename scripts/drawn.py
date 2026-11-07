# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""Where dm says it drew words, on screen: for the checks that press a real pointer
(scripts/linux/gui.py, scripts/chromium-human.py).

Each dialog of dm's is a window of its own (contract row B42). dm's control port says where
each dialog window is (`ui.windows`) and where each word was drawn inside its window
(`ui.texts`); the two together are a point to aim the real pointer at. Only where to aim:
the click is the pointer's, and what it did is judged from dm's state.

`call` is a control-port client: `call(method, **params)` gives the answer's result.
"""


def dialog(call):
    """The words drawn in dm's one open dialog and that window's inner rectangle on screen:
    ([], None) where not exactly one dialog is open, or dm cannot be asked."""
    if not call:
        return [], None
    try:
        dialogs = [d for d in (call("ui.windows") or {}).get("dialogs") or [] if d.get("inner")]
        if len(dialogs) != 1:
            return [], None
        texts = [t for t in (call("ui.texts") or {}).get("texts") or [] if t.get("window") == "dialog"]
    except (OSError, RuntimeError, ValueError):
        return [], None
    return texts, dialogs[0]["inner"]


def centre(origin, rect):
    """The middle of `rect` (in its window) on screen, the window's corner at `origin`."""
    return [int(origin[0] + (rect[0] + rect[2]) / 2), int(origin[1] + (rect[1] + rect[3]) / 2)]


def within(at, inner):
    """Whether a point on screen lies in the window rectangle `inner`."""
    return bool(at and inner) and inner[0] <= at[0] <= inner[2] and inner[1] <= at[1] <= inner[3]


def find(texts, words, contains=False):
    """The first text drawn that is `words` (or holds them, with `contains`)."""
    return next((t for t in texts if (words in t.get("text", "") if contains else t.get("text") == words)), None)


def point(texts, origin, words, contains=False):
    """Where `words` are drawn, on screen, or None."""
    found = find(texts, words, contains)
    return centre(origin, found["rect"]) if found else None


def label_of(texts, label):
    """The text drawn as `label`: whole, or, for a long label wrapped onto several lines, its
    first line."""
    return next((t for t in texts if t.get("text") == label
                 or (len(t.get("text", "")) > 30 and label.startswith(t["text"].rstrip()))), None)


def beside(texts, label):
    """What is drawn on `label`'s own row to its right, nearest first."""
    lx1 = label["rect"][2]
    middle = (label["rect"][1] + label["rect"][3]) / 2
    return sorted((t for t in texts if t["rect"][0] > lx1 and abs((t["rect"][1] + t["rect"][3]) / 2 - middle) < 8),
                  key=lambda t: t["rect"][0])


def value(texts, label):
    """What is drawn beside `label` (a number's value, a field's text), or None."""
    found = label_of(texts, label)
    row = beside(texts, found) if found else []
    return row[0]["text"] if row else None


def field_point(texts, origin, label):
    """Where to click into the field `label` names: the hint or text drawn on the label's own
    row to its right, where there is one (a form's row); a folder's box, just left of the
    Browse… under a long label; else just below the label, where dm's `field` puts the box."""
    found = label_of(texts, label)
    if not found:
        return None
    lx0, ly0, _, ly1 = found["rect"]
    row = beside(texts, found)
    if row:
        r = row[0]["rect"]
        return [int(origin[0] + r[0] + 6), int(origin[1] + (r[1] + r[3]) / 2)]
    # Only under a long label (a sentence over a folder's box): a short one may have its own
    # box with a folder's below it, as Move/Rename's Name pattern has.
    if len(found.get("text", "")) > 30:
        browse = sorted((t for t in texts if t.get("text") == "Browse…" and 0 <= t["rect"][1] - ly0 < 80),
                        key=lambda t: t["rect"][1])
        if browse:
            r = browse[0]["rect"]
            return [int(origin[0] + r[0] - 80), int(origin[1] + (r[1] + r[3]) / 2)]
    return [int(origin[0] + lx0 + 40), int(origin[1] + ly1 + 12)]


def dialog_point(call, words, contains=False):
    """Where `words` are drawn in dm's open dialog, on screen, or None."""
    texts, inner = dialog(call)
    return point(texts, inner, words, contains) if inner else None


def main_point(call, origin, words):
    """Where `words` are drawn in dm's main window, whose content's corner is at `origin`."""
    if not (call and origin):
        return None
    try:
        texts = [t for t in (call("ui.texts") or {}).get("texts") or [] if t.get("window") == "main"]
    except (OSError, RuntimeError, ValueError):
        return None
    return point(texts, origin, words)
